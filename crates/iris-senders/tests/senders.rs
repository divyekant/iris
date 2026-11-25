//! The sender rules on a real store: a hand-written set of cases, and the
//! score on the synthetic mailbox with the quality test set.

use std::path::PathBuf;

use iris_eval::{run, Answer};
use iris_senders::{decide, refresh, Decision, Kind};
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
                ("UNSENT", "UNSENT", "system"),
                ("SENT_COPY", "SENT_COPY", "system"),
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
            // The display name of `From`, as the understanding step saves it.
            from_name: iris_senders::head::Head::parse(raw)
                .get("from")
                .and_then(|v| iris_senders::head::addresses(v).into_iter().next())
                .and_then(|(_, n)| n),
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

/// #109: a name that you wrote in sent mail replaces a name from received
/// mail, and later received mail does not replace it. The spoof check reads
/// this name, so a forged `From: Ana Lee` finds the known person.
#[test]
fn a_name_you_wrote_replaces_a_received_name() {
    let mut c = Case::new("sent-name");
    let support = "From: Support <ana@example.org>\r\nTo: owner@example.com";
    c.put("t1", &["INBOX"], support);
    assert_eq!(c.row("ana@example.org").0.as_deref(), Some("Support"));
    c.put(
        "t2",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: Ana Lee <ana@example.org>"),
    );
    assert_eq!(c.row("ana@example.org").0.as_deref(), Some("Ana Lee"));
    // Received mail, and sent mail with no name, keep your name.
    c.put("t3", &["INBOX"], support);
    c.put(
        "t4",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: ana@example.org"),
    );
    assert_eq!(
        c.row("ana@example.org"),
        (
            Some("Ana Lee".into()),
            Some("person".into()),
            Some("sent".into()),
            Some("engine".into())
        )
    );
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

/// An IMAP draft and a copy in a Sent folder are no evidence of a sender:
/// they are not earlier mail of a service, and a reply after a copy does
/// not make its sender a person (IMAP spec section 4).
#[test]
fn drafts_and_sent_copies_are_no_sender_evidence() {
    let mut c = Case::new("copies-replied");
    let head = "From: Cy <cy@example.net>\r\nTo: owner@example.com";
    c.put("t1", &["SENT_COPY"], head);
    c.put("t1", &["UNSENT"], head);
    c.put("t1", &["SENT"], &format!("From: {OWNER}"));
    assert_eq!(c.put("t9", &["INBOX"], head).kind, Kind::Stranger);

    let mut c = Case::new("copies-service");
    c.put("t1", &["UNSENT"], &service_mail());
    c.put("t2", &["SENT_COPY"], &service_mail());
    assert_eq!(c.put("t3", &[], &service_mail()).kind, Kind::Stranger);
    assert_eq!(c.put("t4", &[], &service_mail()).kind, Kind::Stranger);
    assert_eq!(c.put("t5", &[], &service_mail()).kind, Kind::Service);
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

/// For an account that is not Gmail, a header with the Gmail authserv-id is
/// text that any sender can write: it is no DKIM pass (IMAP spec section 8).
#[test]
fn a_gmail_header_counts_only_for_a_gmail_account() {
    let mut c = Case::new("service-imap");
    c.store
        .write(|tx| {
            tx.execute(
                "UPDATE account SET provider = 'imap' WHERE id = ?1",
                [c.acc],
            )?;
            Ok(())
        })
        .unwrap();
    for t in ["t1", "t2", "t3", "t4"] {
        assert_eq!(c.put(t, &[], &service_mail()).kind, Kind::Stranger);
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
    c.put("t2", &[], &service_mail());
    c.put("t3", &[], &service_mail());
    // The service becomes known, so the recipient becomes hostile in the
    // same call. The caller must run understanding again on its mail.
    let d = c.put("t4", &[], &service_mail());
    assert_eq!(d.kind, Kind::Service);
    assert_eq!(d.rerun, vec![id(&c)]);
    assert_eq!(
        c.row("alerts@shop-secure.example.net").1.as_deref(),
        Some("hostile")
    );
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
    assert_eq!(c.row("alice@paypa1.com").1.as_deref(), Some("person"));
    // Bob has no evidence now, and his domain is a near copy of the known
    // domain of Alice (spec 1 section 6.7: the first known domain wins).
    assert_eq!(c.row("bob@paypal.com").1.as_deref(), Some("hostile"));
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
    // paypal.com stops being known: its only sent message loses SENT. The
    // look-alike row goes back.
    let raw =
        format!("From: {OWNER}\r\nTo: support@paypal.com\r\nSubject: x\r\n\r\nBody text.\r\n");
    let m = new_message("m2", "t2", 2000, &["INBOX"], raw.as_bytes());
    c.store.put_messages(c.acc, &[m]).unwrap();
    let d = again(&c, 2, &raw);
    assert_eq!(c.row("support@paypal.com").1.as_deref(), Some("stranger"));
    assert_eq!(c.row("alerts@paypa1.com").1.as_deref(), Some("stranger"));
    assert_eq!(d.rerun, vec![id]);
}

/// Run [`refresh`], as the engine does after each batch.
fn refreshed(c: &Case) -> Vec<i64> {
    c.store.write(|tx| Ok(refresh(tx, 7)?)).unwrap()
}

#[test]
fn a_refresh_after_a_change_of_evidence_keeps_known_domains_true() {
    // The qualifying message of a service is fetched again from another
    // address. No decision runs on it before the refresh.
    let mut c = Case::new("refresh-service");
    for t in ["t1", "t2", "t3"] {
        c.put(t, &[], &service_mail());
    }
    let raw = "From: zed@example.org\r\nSubject: x\r\n\r\nBody.\r\n";
    let mut m = new_message("m3", "t3", 3000, &[], raw.as_bytes());
    m.content_sha256 = Some(c.store.put_content(raw.as_bytes()).unwrap());
    c.store.put_messages(c.acc, &[m]).unwrap();
    refreshed(&c);
    assert_eq!(
        c.row("no-reply@shop.example.com").1.as_deref(),
        Some("stranger")
    );
    assert_eq!(
        c.put("t4", &[], "From: ann@shop-secure.example.net").kind,
        Kind::Stranger
    );

    // Sync marks the only sent message to an address gone.
    let mut c = Case::new("refresh-gone");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: bob@paypal.com"),
    );
    let look_alike = c.put("t2", &[], "From: alerts@paypa1.com");
    assert_eq!(look_alike.kind, Kind::Hostile);
    set(&c, "UPDATE message SET gone_at = 1 WHERE id = 1");
    let rerun = refreshed(&c);
    assert_eq!(c.row("bob@paypal.com").1.as_deref(), Some("stranger"));
    assert_eq!(c.row("alerts@paypa1.com").1.as_deref(), Some("stranger"));
    assert_eq!(rerun, vec![look_alike.sender.unwrap()]);
    assert_eq!(
        c.put("t3", &[], "From: alerts@paypa1.com").kind,
        Kind::Stranger
    );
    // Nothing changed: a second refresh changes nothing.
    assert!(refreshed(&c).is_empty());
}

/// Store the raw messages `heads` (thread, labels, head) as sync does, with
/// empty header rows. Then understand them newest first: save the header
/// row of each one just before its decision, as spec 1 section 7.1 does.
fn staged(c: &mut Case, heads: &[(&str, &[&str], String)]) -> Vec<Decision> {
    let mut stored = Vec::new();
    for (thread, labels, head) in heads {
        c.n += 1;
        let raw = format!("{head}\r\nSubject: x\r\n\r\nBody text.\r\n");
        let mut m = new_message(
            &format!("m{}", c.n),
            thread,
            c.n * 1000,
            labels,
            raw.as_bytes(),
        );
        let full = std::mem::take(&mut m.header);
        let id = c.store.put_messages(c.acc, &[m]).unwrap()[0];
        stored.push((id, raw, full));
    }
    let mut out = Vec::new();
    for (id, raw, full) in stored.into_iter().rev() {
        c.store
            .write(|tx| {
                tx.execute(
                    "UPDATE header SET from_addr = ?1, to_addrs = ?2, cc_addrs = ?3 WHERE message_id = ?4",
                    rusqlite::params![full.from_addr, full.to_addrs, full.cc_addrs, id],
                )?;
                out.push(decide(tx, id, raw.as_bytes(), 7)?);
                Ok(())
            })
            .unwrap();
    }
    out
}

#[test]
fn a_service_understood_newest_first_is_a_service_after_the_refresh() {
    let mut c = Case::new("staged-service");
    let heads: Vec<(&str, &[&str], String)> = ["t1", "t2", "t3"]
        .into_iter()
        .map(|t| (t, &[][..], service_mail()))
        .collect();
    staged(&mut c, &heads);
    refreshed(&c);
    assert_eq!(
        c.row("no-reply@shop.example.com").1.as_deref(),
        Some("service")
    );
}

#[test]
fn a_reply_with_no_recipient_makes_an_earlier_sender_a_person() {
    let mut c = Case::new("staged-reply");
    // Newest first: the reply is decided first, before the header row of
    // Zed's message is saved. Then Zed's message is decided.
    staged(
        &mut c,
        &[
            ("t1", &[][..], "From: zed@example.net".to_string()),
            ("t1", &["SENT"][..], format!("From: {OWNER}")),
        ],
    );
    assert_eq!(c.row("zed@example.net").1.as_deref(), Some("person"));
    // Oldest first: Zed is a stranger, then the reply makes him a person.
    let mut c = Case::new("reply-later");
    assert_eq!(
        c.put("t1", &[], "From: zed@example.net").kind,
        Kind::Stranger
    );
    c.put("t1", &["SENT"], &format!("From: {OWNER}"));
    assert_eq!(c.row("zed@example.net").1.as_deref(), Some("person"));
}

#[test]
fn a_service_that_was_hostile_is_a_service_again() {
    let mut c = Case::new("hostile-service");
    let sent = format!("From: {OWNER}\r\nTo: support@paypal.com");
    c.put("t1", &["SENT"], &sent);
    // The look-alike service is hostile while paypal.com is known.
    let mail = "From: no-reply@paypa1.com\r\nAuto-Submitted: auto-generated\r\nAuthentication-Results: mx.google.com;\r\n dkim=pass header.i=@paypa1.com";
    for t in ["t2", "t3", "t4"] {
        assert_eq!(c.put(t, &[], mail).kind, Kind::Hostile);
    }
    // paypal.com stops being known: the sent message loses SENT.
    let raw = format!("{sent}\r\nSubject: x\r\n\r\nBody text.\r\n");
    let m = new_message("m1", "t1", 1000, &["INBOX"], raw.as_bytes());
    c.store.put_messages(c.acc, &[m]).unwrap();
    refreshed(&c);
    assert_eq!(c.row("no-reply@paypa1.com").1.as_deref(), Some("service"));
    // support@paypal.com has no evidence now, and its domain is a near copy
    // of the known service.
    assert_eq!(c.row("support@paypal.com").1.as_deref(), Some("hostile"));
}

#[test]
fn a_refresh_after_a_person_sets_a_known_sender_finds_its_look_alikes() {
    let mut c = Case::new("person-known");
    c.put("t1", &[], "From: alice@paypal.com");
    let look_alike = c.put("t2", &[], "From: alerts@paypa1.com");
    assert_eq!(look_alike.kind, Kind::Stranger);
    set(&c, "UPDATE sender SET kind = 'person', decided_by = 'person' WHERE address = 'alice@paypal.com'");
    assert_eq!(refreshed(&c), vec![look_alike.sender.unwrap()]);
    assert_eq!(c.row("alerts@paypa1.com").1.as_deref(), Some("hostile"));
}

#[test]
fn own_sent_mail_is_not_a_reply_to_received_mail() {
    for from in [OWNER, "alias@example.org"] {
        let mut c = Case::new("own-sent");
        c.put("t1", &["SENT"], &format!("From: {from}"));
        c.put("t1", &["SENT"], &format!("From: {from}"));
        // Received mail that writes the same From address is not a person.
        let d = c.put("t1", &["INBOX"], &format!("From: {from}"));
        assert_eq!(d.kind, Kind::Stranger, "{from}");
    }
}

#[test]
fn sent_or_draft_mail_is_no_service_evidence() {
    // Two earlier messages with the service address are your own mail.
    let mut c = Case::new("own-service");
    c.put("t1", &["SENT"], &service_mail());
    c.put("t2", &["DRAFT"], &service_mail());
    assert_eq!(c.put("t3", &[], &service_mail()).kind, Kind::Stranger);
    // The qualifying message gets SENT, and a refresh runs before its
    // decision.
    let mut c = Case::new("service-sent");
    for t in ["t1", "t2", "t3"] {
        c.put(t, &[], &service_mail());
    }
    set(&c, "INSERT INTO message_label(message_id, label_id) SELECT 3, id FROM label WHERE provider_id = 'SENT'");
    refreshed(&c);
    assert_eq!(
        c.row("no-reply@shop.example.com").1.as_deref(),
        Some("stranger")
    );
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
    // The refresh after the batch: its cost on the whole mailbox.
    let t0 = std::time::Instant::now();
    let rerun = store.write(|tx| Ok(refresh(tx, 0)?)).unwrap();
    eprintln!(
        "refresh: {} rows to run again in {:?}",
        rerun.len(),
        t0.elapsed()
    );
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

/// The `From` addresses of the mail in the queue of the fixed rules
/// (`fixed_due`, decision D31), sorted. Empties the queue.
fn due(c: &Case) -> Vec<String> {
    c.store
        .write(|tx| {
            let v = tx
                .prepare(
                    "SELECT DISTINCT h.from_addr FROM fixed_due f
                       JOIN header h ON h.message_id = f.message_id ORDER BY 1",
                )?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            tx.execute("DELETE FROM fixed_due", [])?;
            Ok(v)
        })
        .unwrap()
}

/// #111, decision D31: when a name becomes the name of a known person, the
/// mail under that name goes to the queue of the fixed rules, so that the
/// spoof check reads it again with no model call. Also when the name goes
/// away. No model rerun.
#[test]
fn a_new_known_name_queues_the_mail_under_that_name() {
    let mut c = Case::new("name-twins");
    let forged = "From: Ana Lee <ana@evil.example.net>\r\nTo: owner@example.com";
    c.put("t1", &["INBOX"], forged);
    due(&c);
    // You write to the real Ana: her name is now a known name.
    let d = c.put(
        "t2",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: Ana Lee <ana@example.org>"),
    );
    assert!(d.rerun.is_empty(), "no model rerun");
    assert_eq!(due(&c), ["ana@evil.example.net"]);
    // The same name again changes nothing.
    c.put(
        "t3",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: ANA LEE <ana@example.org>"),
    );
    assert!(due(&c).is_empty());
    // A new name: the mail under the old name and the new name goes again.
    let bo = "From: Ana Bo <x@other.example.net>\r\nTo: owner@example.com";
    c.put("t4", &["INBOX"], bo);
    due(&c);
    let d = c.put(
        "t5",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: Ana Bo <ana@example.org>"),
    );
    assert!(d.rerun.is_empty());
    assert_eq!(due(&c), ["ana@evil.example.net", "x@other.example.net"]);
}

/// #111, decision D31: a sender that becomes or stops being a service
/// queues its mail, so the code-signal rule reads it again.
#[test]
fn a_service_change_queues_the_mail_of_the_sender() {
    let mut c = Case::new("service-due");
    c.put(
        "t1",
        &["INBOX"],
        "From: Bank <no-reply@bank.example>\r\nTo: owner@example.com",
    );
    due(&c);
    let set = |kind: &str| {
        c.store
            .write(|tx| {
                Ok(tx.execute(
                    "UPDATE sender SET kind = ?1 WHERE address = 'no-reply@bank.example'",
                    [kind],
                )?)
            })
            .unwrap();
    };
    set("service");
    assert_eq!(due(&c), ["no-reply@bank.example"]);
    set("service");
    assert!(due(&c).is_empty(), "no change, no queue");
    set("stranger");
    assert_eq!(due(&c), ["no-reply@bank.example"]);
}

fn sender_id(c: &Case, addr: &str) -> i64 {
    c.store
        .read(|r| {
            Ok(
                r.query_row("SELECT id FROM sender WHERE address = ?1", [addr], |x| {
                    x.get(0)
                })?,
            )
        })
        .unwrap()
}

/// #111: a second Ana Lee has a spoof result while the first is known.
/// When you write to the second one, its own mail goes to the queue too,
/// so the mark can clear.
#[test]
fn a_new_person_with_a_known_name_queues_its_own_mail() {
    let mut c = Case::new("own-twin");
    c.put(
        "t1",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: Ana Lee <ana@example.org>"),
    );
    c.put(
        "t2",
        &["INBOX"],
        "From: Ana Lee <ana@work.example.net>\r\nTo: owner@example.com",
    );
    c.put(
        "t3",
        &["INBOX"],
        "From: Bo Ray <bo@example.net>\r\nTo: owner@example.com",
    );
    due(&c);
    let d = c.put(
        "t4",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: Ana Lee <ana@work.example.net>, Bo Ray <bo@example.net>"),
    );
    assert!(d.rerun.is_empty());
    assert_eq!(due(&c), ["ana@work.example.net", "bo@example.net"]);
}

/// #111: a sender that the person accepts at the gate makes its name a
/// known name, so the mail under that name goes to the queue.
#[test]
fn an_accept_at_the_gate_queues_the_mail_under_the_name() {
    let mut c = Case::new("gate-twins");
    c.put(
        "t1",
        &["INBOX"],
        "From: Cy Ray <cy@evil.example.net>\r\nTo: owner@example.com",
    );
    c.put(
        "t2",
        &["INBOX"],
        "From: Cy Ray <cy@example.org>\r\nTo: owner@example.com",
    );
    let cy = sender_id(&c, "cy@example.org");
    due(&c);
    assert_eq!(gate_of(&c, accept, cy), Gate::Done { rerun: vec![] });
    assert_eq!(due(&c), ["cy@evil.example.net", "cy@example.org"]);
}

// The stranger gate (spec 1 section 6.7, card R5-03).

use iris_senders::gate::{self, Answer as Gate};

fn gate_of(
    c: &Case,
    f: fn(&rusqlite::Connection, i64, i64) -> rusqlite::Result<Gate>,
    id: i64,
) -> Gate {
    c.store.write(|tx| Ok(f(tx, id, 9)?)).unwrap()
}

// The gate functions for a sender at the gate.
fn accept(c: &rusqlite::Connection, id: i64, now: i64) -> rusqlite::Result<Gate> {
    gate::accept(c, id, now, true)
}

fn decline(c: &rusqlite::Connection, id: i64, now: i64) -> rusqlite::Result<Gate> {
    gate::decline(c, id, now, true)
}

fn declined_at(c: &Case, addr: &str) -> Option<i64> {
    c.store
        .read(|r| {
            Ok(r.query_row(
                "SELECT declined_at FROM sender WHERE address = ?1",
                [addr],
                |x| x.get(0),
            )?)
        })
        .unwrap()
}

#[test]
fn an_accepted_stranger_is_a_person_and_its_look_alikes_turn_hostile() {
    let mut c = Case::new("gate-accept");
    let alice = c
        .put("t1", &[], "From: Alice <alice@paypal.com>")
        .sender
        .unwrap();
    let look = c.put("t2", &[], "From: alerts@paypa1.com").sender.unwrap();
    set(
        &c,
        &format!("UPDATE sender SET declined_at = 5 WHERE id = {alice}"),
    );

    assert_eq!(gate_of(&c, accept, alice), Gate::Done { rerun: vec![look] });
    let row = c.row("alice@paypal.com");
    assert_eq!(
        (row.1.as_deref(), row.2.as_deref(), row.3.as_deref()),
        (Some("person"), Some(gate::ACCEPTED), Some("person"))
    );
    assert_eq!(
        declined_at(&c, "alice@paypal.com"),
        None,
        "accept ends a decline"
    );
    assert_eq!(c.row("alerts@paypa1.com").1.as_deref(), Some("hostile"));
    // A second accept changes nothing, and the engine keeps the answer.
    assert_eq!(gate_of(&c, accept, alice), Gate::Done { rerun: vec![] });
    assert_eq!(refreshed(&c), Vec::<i64>::new());
    assert_eq!(c.row("alice@paypal.com").1.as_deref(), Some("person"));
}

#[test]
fn the_gate_refuses_a_sender_that_is_not_a_stranger_now() {
    let mut c = Case::new("gate-refuse");
    for t in ["t1", "t2", "t3"] {
        c.put(t, &[], &service_mail());
    }
    let shop = c.put("t4", &[], &service_mail()).sender.unwrap();
    let look = c
        .put("t5", &[], "From: alerts@shop-secure.example.net")
        .sender
        .unwrap();
    let known = c.put("t6", &[], "From: ana@example.org").sender.unwrap();
    c.put(
        "t7",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: ana@example.org"),
    );

    for f in [accept, decline] {
        assert_eq!(gate_of(&c, f, shop), Gate::NotStranger(Some(Kind::Service)));
        assert_eq!(gate_of(&c, f, look), Gate::NotStranger(Some(Kind::Hostile)));
        assert_eq!(gate_of(&c, f, known), Gate::NotStranger(Some(Kind::Person)));
        assert_eq!(gate_of(&c, f, 999), Gate::NoSender);
    }
    // A stored kind older than the evidence: the rules decide, and a refusal
    // writes nothing.
    set(
        &c,
        &format!("UPDATE sender SET kind = 'stranger' WHERE id = {look}"),
    );
    for f in [accept, decline] {
        assert_eq!(gate_of(&c, f, look), Gate::NotStranger(Some(Kind::Hostile)));
    }
    let row = c.row("alerts@shop-secure.example.net");
    assert_eq!(
        (row.1.as_deref(), row.3.as_deref()),
        (Some("stranger"), Some("engine"))
    );
    assert_eq!(declined_at(&c, "alerts@shop-secure.example.net"), None);
    // A person set the kind: only a stranger is at the gate, and the engine
    // rules do not apply.
    set(
        &c,
        &format!("UPDATE sender SET kind = 'service', decided_by = 'person' WHERE id = {look}"),
    );
    assert_eq!(
        gate_of(&c, decline, look),
        Gate::NotStranger(Some(Kind::Service))
    );
    set(
        &c,
        &format!("UPDATE sender SET kind = 'stranger' WHERE id = {look}"),
    );
    assert_eq!(gate_of(&c, decline, look), Gate::Done { rerun: vec![] });
    assert_eq!(gate_of(&c, accept, look), Gate::Done { rerun: vec![] });
    assert_eq!(
        c.row("alerts@shop-secure.example.net").1.as_deref(),
        Some("person")
    );
}

#[test]
fn a_declined_stranger_keeps_an_engine_kind_and_can_turn_hostile() {
    let mut c = Case::new("gate-decline");
    let look = c.put("t1", &[], "From: alerts@paypa1.com").sender.unwrap();
    assert_eq!(gate_of(&c, decline, look), Gate::Done { rerun: vec![] });
    assert_eq!(declined_at(&c, "alerts@paypa1.com"), Some(9));
    // The first decline keeps its time.
    let again = c.store.write(|tx| Ok(decline(tx, look, 20)?)).unwrap();
    assert_eq!(again, Gate::Done { rerun: vec![] });
    assert_eq!(declined_at(&c, "alerts@paypa1.com"), Some(9));
    // A known domain appears: the engine still decides the declined row.
    c.put(
        "t2",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: alice@paypal.com"),
    );
    assert_eq!(c.row("alerts@paypa1.com").1.as_deref(), Some("hostile"));
    assert_eq!(c.row("alerts@paypa1.com").3.as_deref(), Some("engine"));
}

#[test]
fn reopen_gives_the_row_back_to_the_engine() {
    let mut c = Case::new("gate-reopen");
    let alice = c.put("t1", &[], "From: alice@paypal.com").sender.unwrap();
    let look = c.put("t2", &[], "From: alerts@paypa1.com").sender.unwrap();
    gate_of(&c, accept, alice);
    assert_eq!(c.row("alerts@paypa1.com").1.as_deref(), Some("hostile"));
    // The domain is not known after the reopen: the look-alike is a stranger.
    assert_eq!(
        gate_of(&c, gate::reopen, alice),
        Gate::Done { rerun: vec![look] }
    );
    let row = c.row("alice@paypal.com");
    assert_eq!(
        (row.1.as_deref(), row.3.as_deref()),
        (Some("stranger"), Some("engine"))
    );
    assert_eq!(c.row("alerts@paypa1.com").1.as_deref(), Some("stranger"));

    // Reopen ends a decline, and a row with no answer does not change.
    gate_of(&c, decline, alice);
    assert_eq!(
        gate_of(&c, gate::reopen, alice),
        Gate::Done { rerun: vec![] }
    );
    assert_eq!(declined_at(&c, "alice@paypal.com"), None);
    assert_eq!(
        gate_of(&c, gate::reopen, alice),
        Gate::Done { rerun: vec![] }
    );
    assert_eq!(gate_of(&c, gate::reopen, 999), Gate::NoSender);
}

#[test]
fn reopen_decides_an_accepted_look_alike_from_the_evidence_of_now() {
    let mut c = Case::new("gate-reopen-hostile");
    let look = c.put("t1", &[], "From: alerts@paypa1.com").sender.unwrap();
    gate_of(&c, accept, look);
    // While the person owns the kind, the look-alike is a known domain, and
    // the first known domain wins: the real domain is the near copy.
    let alice = c.put("t2", &[], "From: alice@paypal.com").sender.unwrap();
    assert_eq!(c.row("alice@paypal.com").1.as_deref(), Some("hostile"));
    c.put(
        "t3",
        &["SENT"],
        &format!("From: {OWNER}\r\nTo: bob@paypal.com"),
    );
    let bob = c.row("bob@paypal.com");
    assert_eq!(bob.1.as_deref(), Some("hostile"));
    // A person makes Bob known.
    set(
        &c,
        "UPDATE sender SET kind = 'person', decided_by = 'person' WHERE address = 'bob@paypal.com'",
    );
    // The reopen: the look-alike is hostile now, and Alice is no near copy.
    let Gate::Done { mut rerun } = gate_of(&c, gate::reopen, look) else {
        panic!("reopen refused");
    };
    rerun.sort();
    assert_eq!(rerun, vec![look, alice]);
    assert_eq!(c.row("alerts@paypa1.com").1.as_deref(), Some("hostile"));
    assert_eq!(c.row("alice@paypal.com").1.as_deref(), Some("stranger"));
}

#[test]
fn an_answer_needs_the_sender_at_the_gate() {
    let mut c = Case::new("gate-at-gate");
    let ana = c.put("t1", &[], "From: ana@example.org").sender.unwrap();
    let away = |f: fn(&rusqlite::Connection, i64, i64, bool) -> rusqlite::Result<Gate>| {
        c.store.write(|tx| Ok(f(tx, ana, 9, false)?)).unwrap()
    };
    // A rule took the items out of the gate: no answer, and no write.
    assert_eq!(away(gate::accept), Gate::NotAtGate);
    assert_eq!(away(gate::decline), Gate::NotAtGate);
    let row = c.row("ana@example.org");
    assert_eq!(
        (row.1.as_deref(), row.3.as_deref()),
        (Some("stranger"), Some("engine"))
    );
    assert_eq!(declined_at(&c, "ana@example.org"), None);
    // An answer that the sender has already stays an answer.
    gate_of(&c, decline, ana);
    assert_eq!(away(gate::decline), Gate::Done { rerun: vec![] });
    assert_eq!(declined_at(&c, "ana@example.org"), Some(9));
    assert_eq!(away(gate::accept), Gate::NotAtGate);
    gate_of(&c, accept, ana);
    assert_eq!(away(gate::accept), Gate::Done { rerun: vec![] });
}

#[test]
fn reopen_keeps_a_kind_that_a_person_set_outside_the_gate() {
    let mut c = Case::new("gate-reopen-other");
    let ana = c.put("t1", &[], "From: ana@example.org").sender.unwrap();
    for kind in ["hostile", "person", "service", "stranger"] {
        set(
            &c,
            &format!(
                "UPDATE sender SET kind = '{kind}', decided_by = 'person', basis = 'a correction',
                        declined_at = NULL WHERE id = {ana}"
            ),
        );
        assert_eq!(gate_of(&c, gate::reopen, ana), Gate::Done { rerun: vec![] });
        let row = c.row("ana@example.org");
        assert_eq!(
            (row.1.as_deref(), row.2.as_deref(), row.3.as_deref()),
            (Some(kind), Some("a correction"), Some("person")),
            "{kind}"
        );
    }
    // A decline on a person-set stranger ends, and the kind stays.
    gate_of(&c, decline, ana);
    gate_of(&c, gate::reopen, ana);
    assert_eq!(declined_at(&c, "ana@example.org"), None);
    assert_eq!(c.row("ana@example.org").3.as_deref(), Some("person"));
}
