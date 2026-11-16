//! The sender rules on a real store: a hand-written set of cases, and the
//! score on the synthetic mailbox with the quality test set.

use std::path::PathBuf;

use iris_eval::{run, Answer};
use iris_senders::{decide, Decision, Kind};
use iris_store::{Header, NewMessage, Store};
use iris_synth::{Mailbox, Sender};
use rusqlite::OptionalExtension;

const OWNER: &str = "owner@example.com";

/// A new empty data folder for one test, removed at the end.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-senders-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open(dir: &TempDir) -> (Store, i64) {
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", OWNER).unwrap();
    store
        .put_labels(
            acc,
            &[
                ("INBOX", "INBOX", "system"),
                ("SENT", "SENT", "system"),
                ("DRAFT", "DRAFT", "system"),
            ],
        )
        .unwrap();
    (store, acc)
}

/// The bare lowercase address of the `From` header, as the understanding
/// step saves it in `header.from_addr`.
fn from_addr(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let line = text
        .split("\r\n")
        .find_map(|l| l.strip_prefix("From: "))
        .unwrap();
    let line = line.split(" (").next().unwrap();
    let addr = line.rsplit('<').next().unwrap().trim_end_matches('>');
    addr.to_ascii_lowercase()
}

/// The bare lowercase addresses of a header, joined by `,`, as the
/// understanding step saves `header.to_addrs` and `header.cc_addrs`.
fn addr_list(raw: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let line = text.split("\r\n").find_map(|l| l.strip_prefix(name))?;
    let list: Vec<String> = line
        .split(',')
        .filter(|p| p.contains('@'))
        .map(|p| {
            let p = p.rsplit('<').next().unwrap().trim_end_matches('>');
            p.trim().to_ascii_lowercase()
        })
        .collect();
    Some(list.join(","))
}

fn new_message(id: &str, thread: &str, date: i64, labels: &[&str], raw: &[u8]) -> NewMessage {
    NewMessage {
        provider_id: id.into(),
        thread_provider_id: thread.into(),
        internal_date: Some(date),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        header: Header {
            from_addr: Some(from_addr(raw)),
            to_addrs: addr_list(raw, "To: "),
            cc_addrs: addr_list(raw, "Cc: "),
            ..Header::default()
        },
        ..NewMessage::default()
    }
}

/// A small store for the hand-written cases. Each `put` saves one message
/// and gives back what `decide` says about it.
struct Case {
    _dir: TempDir,
    store: Store,
    acc: i64,
    n: i64,
}

impl Case {
    fn new(name: &str) -> Case {
        let dir = TempDir::new(name);
        let (store, acc) = open(&dir);
        Case {
            _dir: dir,
            store,
            acc,
            n: 0,
        }
    }

    fn put(&mut self, thread: &str, labels: &[&str], head: &str) -> Decision {
        self.n += 1;
        let raw = format!("{head}\r\nSubject: x\r\n\r\nBody text.\r\n");
        let m = new_message(
            &format!("m{}", self.n),
            thread,
            self.n * 1000,
            labels,
            raw.as_bytes(),
        );
        let id = self.store.put_messages(self.acc, &[m]).unwrap()[0];
        self.store
            .write(|tx| Ok(decide(tx, id, raw.as_bytes(), 7)?))
            .unwrap()
    }

    /// (name, kind, basis, decided_by) of the row, or four `None` if there is
    /// no row.
    fn row(
        &self,
        addr: &str,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT name, kind, basis, decided_by FROM sender WHERE address = ?1",
                    [addr],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?)
            })
            .unwrap()
            .unwrap_or_default()
    }
}

const GMAIL_PASS: &str = "Authentication-Results: mx.google.com;\r\n dkim=pass header.i=@shop.example.com;\r\n dmarc=pass header.from=shop.example.com";

fn service_mail() -> String {
    format!("From: Shop <no-reply@shop.example.com>\r\nTo: {OWNER}\r\nAuto-Submitted: auto-generated\r\n{GMAIL_PASS}")
}

#[test]
fn sent_mail_makes_each_recipient_a_person() {
    let mut c = Case::new("sent");
    let d = c.put(
        "t1",
        &["SENT"],
        &format!(
            "From: {OWNER}\r\nTo: \"Lee, Ana\" <Ana@Example.org>, bo@example.net\r\nCc: {OWNER}"
        ),
    );
    assert_eq!((d.kind, d.sender), (Kind::SelfOwner, None));
    assert_eq!(c.row("ana@example.org").1.as_deref(), Some("person"));
    assert_eq!(c.row("bo@example.net").2.as_deref(), Some("sent"));
    // A new thread from Ana: still a person.
    let d = c.put(
        "t2",
        &["INBOX"],
        "From: Ana Lee <ana@example.org>\r\nTo: owner@example.com",
    );
    assert_eq!((d.kind, d.previous), (Kind::Person, Some(Kind::Person)));
}

#[test]
fn a_reply_in_the_thread_makes_a_person_and_a_draft_does_not() {
    let mut c = Case::new("reply");
    let head = "From: Cy <cy@example.net>\r\nTo: owner@example.com";
    assert_eq!(c.put("t1", &["INBOX"], head).kind, Kind::Stranger);
    // A draft reply does not count.
    let draft = c.put(
        "t1",
        &["DRAFT"],
        &format!("From: {OWNER}\r\nTo: zed@example.net"),
    );
    assert_eq!(draft.kind, Kind::SelfOwner);
    assert_eq!(c.row("zed@example.net").1, None);
    assert_eq!(c.put("t1", &["INBOX"], head).kind, Kind::Stranger);
    // A sent reply with no To header: the thread rule decides.
    c.put("t1", &["SENT"], &format!("From: {OWNER}"));
    let d = c.put("t9", &["INBOX"], head);
    assert_eq!((d.kind, d.basis.as_str()), (Kind::Person, "replied"));
}

#[test]
fn a_sent_message_before_a_new_participant_does_not_make_it_a_person() {
    let mut c = Case::new("later");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: ana@example.org"),
    );
    // A new address joins the thread after the last sent message.
    let d = c.put(
        "t1",
        &["INBOX"],
        "From: Dee <dee@example.net>\r\nCc: owner@example.com",
    );
    assert_eq!(d.kind, Kind::Stranger);
    // A sent reply after it makes it a person.
    c.put("t1", &["SENT"], &format!("From: {OWNER}"));
    let d = c.put("t2", &["INBOX"], "From: Dee <dee@example.net>");
    assert_eq!((d.kind, d.basis.as_str()), (Kind::Person, "replied"));
}

#[test]
fn a_bcc_recipient_becomes_a_person() {
    let mut c = Case::new("bcc");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nBcc: Eve <eve@example.org>"),
    );
    assert_eq!(c.row("eve@example.org").1.as_deref(), Some("person"));
    let d = c.put("t2", &["INBOX"], "From: Eve <eve@example.org>");
    assert_eq!(d.kind, Kind::Person);
    // The row stays a person on the next message too.
    assert_eq!(
        c.put("t3", &["INBOX"], "From: Eve <eve@example.org>").kind,
        Kind::Person
    );
}

#[test]
fn older_mail_of_a_known_service_keeps_the_service() {
    // Three service messages are in the store; the engine reads newest first.
    let mut c = Case::new("older");
    let mut ids = Vec::new();
    for t in ["t1", "t2", "t3"] {
        c.n += 1;
        let raw = format!("{}\r\nSubject: x\r\n\r\nBody.\r\n", service_mail());
        let m = new_message(&format!("m{}", c.n), t, c.n * 1000, &[], raw.as_bytes());
        ids.push((c.store.put_messages(c.acc, &[m]).unwrap()[0], raw));
    }
    for (id, raw) in ids.iter().rev() {
        c.store
            .write(|tx| Ok(decide(tx, *id, raw.as_bytes(), 7)?))
            .unwrap();
    }
    assert_eq!(
        c.row("no-reply@shop.example.com").1.as_deref(),
        Some("service")
    );
}

#[test]
fn a_service_needs_automated_mail_gmail_dkim_and_two_earlier_messages() {
    let mut c = Case::new("service");
    assert_eq!(c.put("t1", &[], &service_mail()).kind, Kind::Stranger);
    assert_eq!(c.put("t2", &[], &service_mail()).kind, Kind::Stranger);
    let d = c.put("t3", &[], &service_mail());
    assert_eq!((d.kind, d.previous), (Kind::Service, Some(Kind::Stranger)));

    // The kind belongs to the address: a plain message keeps the service.
    let plain = service_mail().replace("Auto-Submitted: auto-generated\r\n", "");
    assert_eq!(c.put("t4", &[], &plain).kind, Kind::Service);

    // Not automated: never a service.
    let mut c = Case::new("service-plain");
    for t in ["t1", "t2", "t3"] {
        assert_eq!(c.put(t, &[], &plain).kind, Kind::Stranger);
    }
    // A pass that the sender wrote itself, under Gmail's failed result.
    let forged = service_mail().replace(
        "Authentication-Results: mx.google.com;",
        "Authentication-Results: mx.google.com; dkim=fail header.d=shop.example.com\r\nAuthentication-Results: mx.google.com;",
    );
    let mut c = Case::new("service-forged");
    for t in ["t1", "t2", "t3"] {
        assert_eq!(c.put(t, &[], &forged).kind, Kind::Stranger);
    }
}

#[test]
fn a_near_copy_of_a_known_domain_is_hostile() {
    let mut c = Case::new("hostile");
    for t in ["t1", "t2", "t3"] {
        c.put(t, &[], &service_mail());
    }
    let d = c.put(
        "t4",
        &[],
        "From: Shop Security <alerts@shop-secure.example.net>\r\nTo: owner@example.com",
    );
    assert_eq!(d.kind, Kind::Hostile);
    assert_eq!(d.basis, "near copy of shop.example.com");
    assert_eq!(d.rerun, vec![d.sender.unwrap()]);
    // The engine checks `hostile` before `person`: a sent message does not help.
    c.put(
        "t4",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: alerts@shop-secure.example.net, ok@shop.example.com"),
    );
    assert_eq!(
        c.row("alerts@shop-secure.example.net").1.as_deref(),
        Some("hostile")
    );
    assert_eq!(c.row("ok@shop.example.com").1.as_deref(), Some("person"));
    let d = c.put("t4", &[], "From: <alerts@shop-secure.example.net>");
    assert_eq!((d.kind, d.previous), (Kind::Hostile, Some(Kind::Hostile)));
}

#[test]
fn a_kind_set_by_a_person_wins_and_a_row_of_another_crate_gets_its_kind() {
    let mut c = Case::new("person");
    c.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO sender(address, name, kind, basis, decided_by) VALUES
                   ('ok@example.org', NULL, 'service', 'set in the web client', 'person'),
                   ('new@example.org', 'From Talk', NULL, NULL, NULL)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: ok@example.org"),
    );
    let d = c.put("t2", &[], "From: ok@example.org");
    assert_eq!(
        (d.kind, d.basis.as_str()),
        (Kind::Service, "set by a person")
    );
    assert_eq!(c.row("ok@example.org").3.as_deref(), Some("person"));

    let d = c.put("t3", &[], "From: Other Name <NEW@example.org>");
    assert_eq!((d.kind, d.previous), (Kind::Stranger, None));
    let row = c.row("new@example.org");
    assert_eq!(row.0.as_deref(), Some("From Talk"));
    assert_eq!(row.3.as_deref(), Some("engine"));
}

#[test]
fn mail_from_the_account_address_without_sent_is_received_mail() {
    let mut c = Case::new("self");
    let d = c.put(
        "t1",
        &["INBOX"],
        "From: Owner <OWNER@example.com>\r\nTo: dee@example.org",
    );
    // A From with the account address is no proof of a sent message: anyone
    // can write it. Only the SENT label makes mail from you.
    assert_eq!(d.kind, Kind::Stranger);
    assert!(d.sender.is_some());
    assert_eq!(c.row("dee@example.org").1, None);
}

/// Run `decide` again on the stored message `id` with the raw bytes `raw`.
fn again(c: &Case, id: i64, raw: &str) -> Decision {
    c.store
        .write(|tx| Ok(decide(tx, id, raw.as_bytes(), 7)?))
        .unwrap()
}

fn set(c: &Case, sql: &str) {
    c.store
        .write(|tx| {
            tx.execute(sql, [])?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_service_without_its_qualifying_message_is_decided_again() {
    let raw = format!("{}\r\nSubject: x\r\n\r\nBody text.\r\n", service_mail());
    for change in [
        "DELETE FROM message WHERE id = 3",
        "UPDATE message SET content_sha256 = 'other' WHERE id = 3",
    ] {
        let mut c = Case::new("service-gone");
        for t in ["t1", "t2", "t3"] {
            c.put(t, &[], &service_mail());
        }
        assert_eq!(again(&c, 1, &raw).kind, Kind::Service);
        set(&c, change);
        assert_eq!(again(&c, 1, &raw).kind, Kind::Stranger, "{change}");
    }
}

#[test]
fn gone_mail_is_no_evidence() {
    // A sent message that is gone (spec 1 section 4.5).
    let mut c = Case::new("gone-sent");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: bo@example.net"),
    );
    set(&c, "UPDATE message SET gone_at = 1 WHERE id = 1");
    assert_eq!(
        c.put("t2", &[], "From: bo@example.net").kind,
        Kind::Stranger
    );
    // A gone reply in the thread.
    let mut c = Case::new("gone-reply");
    c.put("t1", &[], "From: bo@example.net");
    c.put("t1", &["SENT"], &format!("From: {OWNER}"));
    set(&c, "UPDATE message SET gone_at = 1 WHERE id = 2");
    assert_eq!(
        c.put("t2", &[], "From: bo@example.net").kind,
        Kind::Stranger
    );
    // Gone earlier mail of a service.
    let mut c = Case::new("gone-service");
    c.put("t1", &[], &service_mail());
    c.put("t2", &[], &service_mail());
    set(&c, "UPDATE message SET gone_at = 1 WHERE id = 1");
    assert_eq!(c.put("t3", &[], &service_mail()).kind, Kind::Stranger);
    // Gone the qualifying message of a kept service.
    let mut c = Case::new("gone-kept");
    for t in ["t1", "t2", "t3"] {
        c.put(t, &[], &service_mail());
    }
    set(&c, "UPDATE message SET gone_at = 1 WHERE id = 3");
    let raw = format!("{}\r\nSubject: x\r\n\r\nBody text.\r\n", service_mail());
    assert_eq!(again(&c, 1, &raw).kind, Kind::Stranger);
}

#[test]
fn a_hostile_row_whose_known_domain_goes_becomes_a_person_from_sent_mail() {
    let mut c = Case::new("hostile-clears");
    for t in ["t1", "t2", "t3"] {
        c.put(t, &[], &service_mail());
    }
    c.put(
        "t4",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: ann@shop-secure.example.net"),
    );
    assert_eq!(
        c.row("ann@shop-secure.example.net").1.as_deref(),
        Some("hostile")
    );
    // The known service stops being known (a person sets it a stranger).
    set(&c, "UPDATE sender SET kind = 'stranger', decided_by = 'person' WHERE address = 'no-reply@shop.example.com'");
    let d = c.put("t5", &[], "From: ann@shop-secure.example.net");
    assert_eq!((d.kind, d.previous), (Kind::Person, Some(Kind::Hostile)));
}

#[test]
fn auto_submitted_no_with_a_comment_is_not_a_service() {
    let mut c = Case::new("auto-no");
    let mail = service_mail().replace("auto-generated", "no (human-generated)");
    for t in ["t1", "t2", "t3"] {
        assert_eq!(c.put(t, &[], &mail).kind, Kind::Stranger);
    }
}

#[test]
fn a_comment_after_a_bare_address_is_not_part_of_it() {
    let mut c = Case::new("comment");
    for t in ["t1", "t2", "t3"] {
        c.put(
            t,
            &[],
            &service_mail().replace(
                "From: Shop <no-reply@shop.example.com>",
                "From: no-reply@shop.example.com (Shop mail)",
            ),
        );
    }
    assert_eq!(
        c.row("no-reply@shop.example.com").1.as_deref(),
        Some("service")
    );
}

#[test]
fn a_new_fetch_of_a_sent_message_clears_its_old_bcc() {
    let mut c = Case::new("refetch");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nBcc: eve@example.org"),
    );
    // Gmail gives other bytes for the same message, with no Bcc. The
    // understanding of the new bytes has not run yet.
    let raw = format!("From: {OWNER}\r\nSubject: x\r\n\r\nBody.\r\n");
    let mut m = new_message("m1", "t1", 1000, &["SENT"], raw.as_bytes());
    m.content_sha256 = Some(c.store.put_content(raw.as_bytes()).unwrap());
    c.store.put_messages(c.acc, &[m]).unwrap();
    assert_eq!(
        c.put("t2", &[], "From: eve@example.org").kind,
        Kind::Stranger
    );
}

/// Spec 1 section 6.7: "The first sync fetches newest first, so a known
/// person can be a stranger for some time." The understanding step saves
/// the header row of a message just before it decides the sender, so a sent
/// message of another thread that waits for understanding has no header
/// row yet. Its own decision then makes the recipient a person, and the
/// brief reads the kind when it runs.
#[test]
fn a_sent_message_of_another_thread_counts_at_its_own_decision() {
    let mut c = Case::new("sent-later");
    let d = c.put("t2", &[], "From: Fay <fay@example.org>");
    assert_eq!(d.kind, Kind::Stranger);
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: fay@example.org"),
    );
    assert_eq!(c.row("fay@example.org").1.as_deref(), Some("person"));
    assert_eq!(
        c.put("t3", &[], "From: Fay <fay@example.org>").kind,
        Kind::Person
    );
}

#[test]
fn another_bcc_message_keeps_a_person() {
    let mut c = Case::new("bcc-two");
    let sent = format!("From: {OWNER}\r\nBcc: eve@example.org");
    c.put("t1", &["SENT"], &sent);
    c.put("t2", &["SENT"], &sent);
    // The second sent message gives the basis. It goes.
    set(&c, "DELETE FROM message WHERE id = 2");
    assert_eq!(c.put("t3", &[], "From: eve@example.org").kind, Kind::Person);
}

#[test]
fn a_bcc_person_whose_sent_message_changed_is_decided_again() {
    let mut c = Case::new("bcc-changed");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nBcc: eve@example.org"),
    );
    assert_eq!(c.put("t2", &[], "From: eve@example.org").kind, Kind::Person);
    // The sent message no longer names Eve (a fetch of other bytes, with
    // another Bcc, and a new content hash).
    set(
        &c,
        "UPDATE message SET content_sha256 = 'other' WHERE id = 1",
    );
    assert_eq!(
        c.put("t3", &[], "From: eve@example.org").kind,
        Kind::Stranger
    );
}

#[test]
fn a_person_without_sent_mail_is_decided_again() {
    let mut c = Case::new("evidence");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: bo@example.net"),
    );
    let head = "From: Bo <bo@example.net>\r\nTo: owner@example.com";
    assert_eq!(c.put("t2", &["INBOX"], head).kind, Kind::Person);
    // The only sent message loses its label (for example, it is deleted).
    c.store
        .write(|tx| {
            tx.execute("DELETE FROM message_label WHERE message_id = 1", [])?;
            Ok(())
        })
        .unwrap();
    let d = c.put("t3", &["INBOX"], head);
    assert_eq!((d.kind, d.previous), (Kind::Stranger, Some(Kind::Person)));
}

#[test]
fn a_person_whose_sent_message_is_gone_or_changed_is_decided_again() {
    let mut c = Case::new("gone");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: bo@example.net"),
    );
    let head = "From: Bo <bo@example.net>\r\nTo: owner@example.com";
    // The sent message now has other bytes (a new content hash), and keeps
    // SENT.
    set(
        &c,
        "UPDATE message SET content_sha256 = 'other' WHERE id = 1",
    );
    assert_eq!(c.put("t2", &["INBOX"], head).kind, Kind::Stranger);

    c.put(
        "t3",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: bo@example.net"),
    );
    assert_eq!(c.put("t4", &["INBOX"], head).kind, Kind::Person);
    // The sent message of the basis is deleted from the store.
    c.store
        .write(|tx| {
            tx.execute("DELETE FROM message WHERE id = 3", [])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(c.put("t5", &["INBOX"], head).kind, Kind::Stranger);
}

#[test]
fn a_sent_message_reports_each_hostile_change_of_a_recipient() {
    let mut c = Case::new("rerun");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: alerts@shop-secure.example.net"),
    );
    let id = |c: &Case| -> i64 {
        c.store
            .read(|db| {
                Ok(db.query_row(
                    "SELECT id FROM sender WHERE address = 'alerts@shop-secure.example.net'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    };
    for t in ["t2", "t3", "t4"] {
        c.put(t, &[], &service_mail());
    }
    // The service is known now, so the next sent message makes the
    // recipient hostile. The caller must run understanding again on its mail.
    let d = c.put(
        "t5",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: alerts@shop-secure.example.net"),
    );
    assert_eq!(d.kind, Kind::SelfOwner);
    assert_eq!(d.rerun, vec![id(&c)]);
    // No change: nothing to run again.
    let d = c.put(
        "t6",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: alerts@shop-secure.example.net"),
    );
    assert!(d.rerun.is_empty());
    // The From row reports its change too.
    let d = c.put("t7", &[], "From: <alerts@shop-secure.example.net>");
    assert!(d.rerun.is_empty());
}

#[test]
fn a_look_alike_recipient_of_the_same_sent_message_is_hostile() {
    let mut c = Case::new("same-message");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: support@paypal.com, alerts@paypa1.com"),
    );
    // Neither domain was known before. The first recipient becomes a known
    // person, so the second one is a near copy of a known sender.
    assert_eq!(c.row("support@paypal.com").1.as_deref(), Some("person"));
    assert_eq!(c.row("alerts@paypa1.com").1.as_deref(), Some("hostile"));
    assert_eq!(
        c.put("t2", &[], "From: alerts@paypa1.com").kind,
        Kind::Hostile
    );
}

#[test]
fn a_recipient_whose_only_sent_message_loses_it_is_decided_again() {
    let mut c = Case::new("recheck");
    let head = format!("From: {OWNER}\r\nTo: bo@example.net");
    c.put("t1", &["SENT"], &head);
    assert_eq!(c.row("bo@example.net").1.as_deref(), Some("person"));
    // The message loses SENT (the mirror stores it again), and its decision
    // runs again. Bo has no mail of his own to decide.
    let raw = format!("{head}\r\nSubject: x\r\n\r\nBody text.\r\n");
    let m = new_message("m1", "t1", 1000, &["INBOX"], raw.as_bytes());
    c.store.put_messages(c.acc, &[m]).unwrap();
    again(&c, 1, &raw);
    assert_eq!(c.row("bo@example.net").1.as_deref(), Some("stranger"));
    // A sent message with another recipient does the same.
    let mut c = Case::new("recheck-to");
    c.put("t1", &["SENT"], &head);
    let other = format!("From: {OWNER}\r\nTo: cy@example.net\r\nSubject: x\r\n\r\nBody text.\r\n");
    let mut m = new_message("m1", "t1", 1000, &["SENT"], other.as_bytes());
    m.content_sha256 = Some(c.store.put_content(other.as_bytes()).unwrap());
    c.store.put_messages(c.acc, &[m]).unwrap();
    let d = again(&c, 1, &other);
    assert_eq!(c.row("bo@example.net").1.as_deref(), Some("stranger"));
    assert_eq!(c.row("cy@example.net").1.as_deref(), Some("person"));
    assert!(d.rerun.is_empty());
}

#[test]
fn a_recipient_that_a_person_set_is_not_a_known_sender() {
    let mut c = Case::new("person-set-recipient");
    set(
        &c,
        "INSERT INTO sender(address, kind, basis, decided_by)
         VALUES ('foo@paypal.com', 'stranger', 'set', 'person')",
    );
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: foo@paypal.com, alerts@paypa1.com"),
    );
    assert_eq!(c.row("foo@paypal.com").1.as_deref(), Some("stranger"));
    assert_eq!(c.row("alerts@paypa1.com").1.as_deref(), Some("person"));
}

#[test]
fn a_sent_message_at_the_same_time_is_not_a_reply() {
    let mut c = Case::new("same-time");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: ana@example.org"),
    );
    let head = "From: zed@example.net\r\nTo: owner@example.com";
    assert_eq!(c.put("t1", &[], head).kind, Kind::Stranger);
    // The new participant's message has the same time as the sent message.
    set(&c, "UPDATE message SET internal_date = 1000 WHERE id = 2");
    let raw = format!("{head}\r\nSubject: x\r\n\r\nBody text.\r\n");
    assert_eq!(again(&c, 2, &raw).kind, Kind::Stranger);
}

#[test]
fn a_changed_sent_message_compares_new_recipients_after_the_old_ones_go() {
    let mut c = Case::new("swap");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: bob@paypal.com"),
    );
    // A new fetch of the same message names Alice in place of Bob. Bob was
    // the only known sender of paypal.com.
    let raw = format!("From: {OWNER}\r\nTo: alice@paypa1.com\r\nSubject: x\r\n\r\nBody text.\r\n");
    let mut m = new_message("m1", "t1", 1000, &["SENT"], raw.as_bytes());
    m.content_sha256 = Some(c.store.put_content(raw.as_bytes()).unwrap());
    c.store.put_messages(c.acc, &[m]).unwrap();
    again(&c, 1, &raw);
    assert_eq!(c.row("bob@paypal.com").1.as_deref(), Some("stranger"));
    assert_eq!(c.row("alice@paypa1.com").1.as_deref(), Some("person"));
}

#[test]
fn a_look_alike_that_came_first_is_hostile_when_its_domain_becomes_known() {
    let mut c = Case::new("late-known");
    let d = c.put("t1", &[], "From: alerts@paypa1.com");
    assert_eq!(d.kind, Kind::Stranger);
    let id = d.sender.unwrap();
    // paypal.com becomes known. The look-alike row changes at once, and the
    // caller must run understanding again on its mail.
    let d = c.put(
        "t2",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: support@paypal.com"),
    );
    assert_eq!(c.row("alerts@paypa1.com").1.as_deref(), Some("hostile"));
    assert_eq!(d.rerun, vec![id]);
    // paypal.com stops being known (a person sets it): the row goes back.
    set(&c, "UPDATE sender SET kind = 'stranger', decided_by = 'person' WHERE address = 'support@paypal.com'");
    let d = c.put(
        "t3",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: support@paypal.com"),
    );
    assert_eq!(c.row("alerts@paypa1.com").1.as_deref(), Some("stranger"));
    assert_eq!(d.rerun, vec![id]);
}

#[test]
fn another_qualifying_message_keeps_a_service() {
    let mut c = Case::new("service-two");
    // Messages 3 and 4 each have 2 earlier messages.
    for t in ["t1", "t2", "t3", "t4"] {
        c.put(t, &[], &service_mail());
    }
    set(&c, "DELETE FROM message WHERE id = 4");
    let raw = format!("{}\r\nSubject: x\r\n\r\nBody text.\r\n", service_mail());
    assert_eq!(again(&c, 1, &raw).kind, Kind::Service);
}

#[test]
fn a_fetch_of_the_same_bytes_keeps_a_bcc_person() {
    let mut c = Case::new("same-bytes");
    let head = format!("From: {OWNER}\r\nBcc: eve@example.org");
    c.put("t1", &["SENT"], &head);
    // The mirror stores the same message again, with the same content. The
    // understanding step does not run on it again.
    let raw = format!("{head}\r\nSubject: x\r\n\r\nBody text.\r\n");
    let m = new_message("m1", "t1", 1000, &["SENT"], raw.as_bytes());
    c.store.put_messages(c.acc, &[m]).unwrap();
    assert_eq!(c.put("t2", &[], "From: eve@example.org").kind, Kind::Person);
}

fn answer(kind: Kind) -> Sender {
    match kind {
        Kind::SelfOwner => Sender::Owner,
        Kind::Person => Sender::KnownPerson,
        Kind::Service => Sender::KnownService,
        Kind::Stranger => Sender::Stranger,
        Kind::Hostile => Sender::Hostile,
    }
}

/// The synthetic mailbox goes into a store first, as the first sync puts it
/// there. Then each message gets its kind, newest first as the engine
/// understands mail (spec 1 section 7.1), and the quality test set scores
/// the kinds.
#[test]
fn synthetic_mailbox_meets_the_sender_targets() {
    let dir = TempDir::new("synth");
    let (store, acc) = open(&dir);
    let mb = Mailbox::new(1, 20_000);
    let mut ids = Vec::with_capacity(mb.len());
    let all: Vec<usize> = (0..mb.len()).collect();
    for chunk in all.chunks(1000) {
        let batch: Vec<NewMessage> = chunk
            .iter()
            .map(|&i| {
                let t = mb.truth(i);
                let labels: &[&str] = if t.sender == Sender::Owner {
                    &["SENT"]
                } else {
                    &["INBOX"]
                };
                new_message(
                    &format!("m{i}"),
                    &format!("t{}", t.thread),
                    t.date * 1000,
                    labels,
                    &mb.raw(i),
                )
            })
            .collect();
        ids.extend(store.put_messages(acc, &batch).unwrap());
    }

    let start = std::time::Instant::now();
    let newest_first: Vec<usize> = all.iter().rev().copied().collect();
    let mut next = ids.iter().rev();
    let mut spent = std::time::Duration::ZERO;
    // One transaction for all, as the engine writes a batch.
    let report = store
        .write(|tx| {
            Ok(run(&mb, &newest_first, |raw| {
                let id = *next.next().unwrap();
                let t0 = std::time::Instant::now();
                let d = decide(tx, id, raw, 0).unwrap();
                spent += t0.elapsed();
                Answer {
                    form: None,
                    sender: Some(answer(d.kind)),
                    fields: Default::default(),
                }
            }))
        })
        .unwrap();
    eprintln!(
        "decide: {} messages in {spent:?} ({:?} with the mailbox)",
        mb.len(),
        start.elapsed()
    );
    let text = report.text();
    std::io::Write::write_all(&mut std::io::stderr(), text.as_bytes()).unwrap();
    let failed: Vec<String> = report
        .failures()
        .into_iter()
        .filter(|f| f.starts_with("sender "))
        .collect();
    assert!(failed.is_empty(), "{failed:?}\n{text}");
}
