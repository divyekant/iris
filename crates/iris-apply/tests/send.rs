//! The verbs `draft` and `send` at the fake Gmail server
//! (docs/specs/2026-11-16-send.md). No network, and no real mailbox.

use std::path::PathBuf;
use std::time::Duration;

use iris_apply::{apply, release, Error, Outcome, Token, HOLD_MS};
use iris_changeset::{add_action, create, submit, Author, AuthorKind, Verb};
use iris_fakegmail::FakeGmail;
use iris_gmail::{Config, Gmail};
use iris_store::Store;
use iris_synth::Mailbox;
use rusqlite::params;
use serde_json::{json, Map, Value};

const TOKEN: &str = "test-token";
const OWNER: &str = "owner@example.com";

struct Box {
    fake: FakeGmail,
    gmail: Gmail,
    store: Store,
    account: i64,
    dir: PathBuf,
}

impl Drop for Box {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn setup(name: &str) -> Box {
    let fake = FakeGmail::start(Mailbox::new(7, 40)).unwrap();
    let gmail = Gmail::new(Config {
        base_url: fake.base_url(),
        quota_per_minute: 10_000_000,
        backoff_start: Duration::from_millis(1),
        backoff_max: Duration::from_millis(4),
        ..Config::default()
    });
    let dir = std::env::temp_dir().join(format!("iris-send-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();
    iris_sync::sync(&store, &gmail, account, TOKEN).unwrap();
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO policy_version(text, sha256, confirmed_at)
                 VALUES ('version = 0\nmode = \"apply\"\n', 'h', 0)",
                [],
            )?;
            // No sender is hostile in these tests.
            tx.execute("UPDATE message SET hostile = NULL", [])?;
            // The switch is on since time 0: the hold counts from the approval.
            tx.execute("UPDATE send_switch SET on_at = 0 WHERE id = 1", [])?;
            tx.execute("DELETE FROM sender WHERE kind = 'hostile'", [])?;
            Ok(())
        })
        .unwrap();
    Box {
        fake,
        gmail,
        store,
        account,
        dir,
    }
}

impl Box {
    /// A thread whose newest message (with no DRAFT label) has a
    /// Message-ID, and that message.
    fn thread(&self) -> (i64, i64) {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT m.thread_id, m.id FROM message m
                     WHERE m.gone_at IS NULL
                       AND m.internal_date = (SELECT max(internal_date) FROM message
                                              WHERE thread_id = m.thread_id AND gone_at IS NULL)
                     ORDER BY m.id LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap()
    }

    /// Submit one action by a person, and approve it, `age` milliseconds
    /// ago. Gives the action ID.
    fn approved(&self, verb: Verb, target: &str, args: Value, age: i64) -> i64 {
        self.approved_after(verb, target, args, age, now())
    }

    /// As [`Box::approved`], with the submit at `submitted`.
    fn approved_after(
        &self,
        verb: Verb,
        target: &str,
        args: Value,
        age: i64,
        submitted: i64,
    ) -> i64 {
        let who = Author {
            kind: AuthorKind::Person,
            name: None,
            session: None,
            job: None,
        };
        let cs = create(&self.store, &who, now()).unwrap();
        let args: Map<String, Value> = args.as_object().unwrap().clone();
        add_action(&self.store, cs, &who, verb, &[target], &args, None).unwrap();
        submit(&self.store, cs, &who, "Write.", submitted).unwrap();
        self.store
            .write(|tx| {
                let id: i64 = tx.query_row(
                    "SELECT id FROM changeset_action WHERE changeset_id = ?1",
                    [cs],
                    |r| r.get(0),
                )?;
                let n = tx.execute(
                    "UPDATE changeset_action SET state = 'approved', decided_by = 'person',
                       decided_at = ?2 WHERE id = ?1 AND state IN ('proposed', 'in_review')",
                    params![id, now() - age],
                )?;
                assert_eq!(n, 1, "the action was not proposed");
                Ok(id)
            })
            .unwrap()
    }

    fn token(&self) -> (String, fn() -> Option<String>) {
        (TOKEN.into(), || Some(TOKEN.into()))
    }

    fn apply(&self, action: i64) -> iris_apply::Result<Outcome> {
        let (t, mut renew) = self.token();
        let mut token = Token {
            token: t,
            renew: &mut renew,
            refused: false,
        };
        apply(&self.store, &self.gmail, &mut token, self.account, action)
    }

    /// The release, after the move of the approved sends to `held`, as the
    /// apply step of the engine does.
    fn release(&self, action: i64) -> iris_apply::Result<Outcome> {
        iris_changeset::hold(&self.store, self.account, now()).unwrap();
        let (t, mut renew) = self.token();
        let mut token = Token {
            token: t,
            renew: &mut renew,
            refused: false,
        };
        release(&self.store, &self.gmail, &mut token, self.account, action)
    }

    /// The apply step of the engine at the time now.
    fn step(&self) -> iris_apply::Step {
        let (t, mut renew) = self.token();
        let mut token = Token {
            token: t,
            renew: &mut renew,
            refused: false,
        };
        iris_apply::step(&self.store, &self.gmail, &mut token, self.account, now()).unwrap()
    }

    /// The `d-N` ID and the Gmail draft ID of the draft of `action`.
    fn draft(&self, action: i64) -> (String, String) {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT 'd-' || id, provider_id FROM draft WHERE action_id = ?1",
                    [action],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap()
    }

    fn state(&self, action: i64) -> String {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT state FROM changeset_action WHERE id = ?1",
                    [action],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    fn sends(&self) -> usize {
        self.posts("/drafts/send")
    }

    fn posts(&self, end: &str) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|r| r.starts_with("POST") && r.ends_with(end))
            .count()
    }

    /// The raw message of a Gmail message ID.
    fn raw(&self, id: &str) -> String {
        let mut out = Vec::new();
        self.gmail.raw(TOKEN, id, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn sql(&self, sql: &str, p: impl rusqlite::Params) {
        self.store.write(|tx| Ok(tx.execute(sql, p)?)).unwrap();
    }

    /// A new mail, made at Gmail. Gives the send action, approved `age`
    /// milliseconds ago, and the Gmail draft ID.
    fn new_mail(&self, age: i64) -> (i64, String) {
        let a = self.approved(Verb::Draft, "a-1", mail(), 0);
        assert!(matches!(self.apply(a), Ok(Outcome::Applied(_))));
        let (d, provider) = self.draft(a);
        (self.approved(Verb::Send, &d, json!({}), age), provider)
    }
}

fn mail() -> Value {
    json!({"to": ["priya@example.com"], "subject": "Q4 numbers", "body": "Hi Priya,\n\nHere.\n"})
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

const AGED: i64 = HOLD_MS + 1_000;

/// Acceptance 1: a sent reply has the thread ID, the In-Reply-To of the
/// parent and the `Re:` subject.
#[test]
fn a_reply_goes_into_its_thread() {
    let b = setup("reply");
    let (thread, parent) = b.thread();
    let (provider, thread_provider): (String, String) = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT m.provider_id, t.provider_id FROM message m
                 JOIN thread t ON t.id = m.thread_id WHERE m.id = ?1",
                [parent],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    let head = b
        .gmail
        .metadata(TOKEN, &provider, &["Message-ID", "Subject"])
        .unwrap();
    let header = |n: &str| {
        head.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(n))
            .unwrap()
            .value
            .clone()
    };
    let parent_id = header("Message-ID")
        .trim_matches(['<', '>', ' '])
        .to_string();
    let subject = header("Subject");
    // The parent is understood: the mirror has its subject, so submit
    // flags the same subject as the apply.
    let n = b
        .store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE header SET subject = ?1 WHERE message_id = ?2",
                rusqlite::params![subject, parent],
            )?)
        })
        .unwrap();
    assert_eq!(n, 1);
    let a = b.approved(
        Verb::Draft,
        &format!("t-{thread}"),
        json!({"to": ["priya@example.com"], "body": "Thanks."}),
        0,
    );
    match b.apply(a) {
        Ok(Outcome::Applied(c)) => assert_eq!(c.changed, 1),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.state(a), "applied");
    let (d, _) = b.draft(a);
    let s = b.approved(Verb::Send, &d, json!({}), AGED);
    // apply never sends; only the release does.
    assert!(matches!(b.apply(s), Err(Error::Verb(_))));
    assert_eq!(b.sends(), 0);
    match b.release(s) {
        Ok(Outcome::Applied(_)) => {}
        other => panic!("{other:?}"),
    }
    assert_eq!((b.state(s).as_str(), b.sends()), ("applied", 1));
    let sent: (String,) = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT sent_message FROM draft WHERE action_id = ?1",
                [a],
                |r| Ok((r.get(0)?,)),
            )?)
        })
        .unwrap();
    let raw = b.raw(&sent.0);
    assert!(
        raw.contains(&format!("In-Reply-To: <{parent_id}>")),
        "{raw}"
    );
    let want = iris_compose::reply_subject(&subject);
    assert!(raw.contains(&format!("Subject: {want}")), "{raw}");
    let meta = b.gmail.minimal(TOKEN, &sent.0).unwrap();
    assert!(meta.label_ids.contains(&"SENT".to_string()));
    let found = b
        .gmail
        .search(
            TOKEN,
            &format!("rfc822msgid:{}", sent_message_id(&raw)),
            None,
        )
        .unwrap();
    let thread_of = found.messages[0].thread_id.clone();
    assert_eq!(thread_of, thread_provider);
}

fn sent_message_id(raw: &str) -> String {
    raw.lines()
        .find_map(|l| l.strip_prefix("Message-ID: "))
        .unwrap()
        .trim_matches(['<', '>', ' ', '\r'])
        .to_string()
}

#[test]
fn a_send_waits_for_its_hold() {
    let b = setup("hold");
    let (s, _) = b.new_mail(0);
    match b.release(s) {
        Ok(Outcome::Hold(until)) => assert!(until > now()),
        other => panic!("{other:?}"),
    }
    assert_eq!((b.state(s).as_str(), b.sends()), ("held", 0));
}

/// Acceptance 2: with an unknown result, one `drafts/send` request only,
/// also after many recoveries.
#[test]
fn an_unknown_send_that_went_out_is_found_and_never_sent_again() {
    let b = setup("unknown-sent");
    let (s, _) = b.new_mail(AGED);
    b.fake.fail_next_write(&[503], true);
    let first = b.release(s).unwrap();
    assert!(
        matches!(first, Outcome::Applied(_) | Outcome::Pending),
        "{first:?}"
    );
    for _ in 0..3 {
        let _ = b.release(s);
        let _ = iris_apply::recover(&b.store, &b.gmail, &mut tok(), b.account);
    }
    assert_eq!((b.state(s).as_str(), b.sends()), ("applied", 1));
}

fn tok() -> Token<'static> {
    let renew: &'static mut dyn FnMut() -> Option<String> =
        std::boxed::Box::leak(std::boxed::Box::new(|| Some(TOKEN.to_string())));
    Token {
        token: TOKEN.into(),
        renew,
        refused: false,
    }
}

#[test]
/// An unknown result keeps the claim: after 24 hours the row fails, and
/// the draft takes no new send from Iris (section 6).
fn an_unknown_send_that_did_not_go_out_never_goes_again() {
    let b = setup("unknown-not");
    let (s, _) = b.new_mail(AGED);
    b.fake.fail_next_write(&[503], false);
    assert_eq!(b.release(s).unwrap(), Outcome::Pending);
    assert_eq!(b.release(s).unwrap(), Outcome::Pending);
    assert_eq!((b.state(s).as_str(), b.sends()), ("held", 1));
    // The ledger keeps each entry: move the claim time back, as a clock.
    let hour = 60 * 60 * 1000 + 1;
    let day = 24 * hour;
    b.store
        .write(|tx| {
            // ledger_change forbids a change of `at`: drop and remake the
            // trigger in this test store only.
            let sql: String = tx.query_row(
                "SELECT sql FROM sqlite_schema WHERE name = 'ledger_change'",
                [],
                |r| r.get(0),
            )?;
            tx.execute_batch("DROP TRIGGER ledger_change")?;
            tx.execute(
                "UPDATE ledger SET at = at - ?2 WHERE action_id = ?1",
                params![s, hour],
            )?;
            tx.execute_batch(&sql)?;
            Ok(())
        })
        .unwrap();
    // One hour with the draft still at Gmail proves nothing.
    assert_eq!(b.release(s).unwrap(), Outcome::Pending);
    b.store
        .write(|tx| {
            let sql: String = tx.query_row(
                "SELECT sql FROM sqlite_schema WHERE name = 'ledger_change'",
                [],
                |r| r.get(0),
            )?;
            tx.execute_batch("DROP TRIGGER ledger_change")?;
            tx.execute(
                "UPDATE ledger SET at = at - ?2 WHERE action_id = ?1",
                params![s, day],
            )?;
            tx.execute_batch(&sql)?;
            Ok(())
        })
        .unwrap();
    match b.release(s) {
        Ok(Outcome::Failed { error, .. }) => assert!(error.contains("unknown"), "{error}"),
        other => panic!("{other:?}"),
    }
    assert_eq!((b.state(s).as_str(), b.sends()), ("failed", 1));
    let rows: i64 = b
        .store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM ledger_send", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(rows, 1);
    // A new send of the draft is refused at submit, and nothing goes out.
    let d: String = b
        .store
        .read(|c| Ok(c.query_row("SELECT 'd-' || draft_id FROM ledger_send", [], |r| r.get(0))?))
        .unwrap();
    let who = Author {
        kind: AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    };
    let cs = create(&b.store, &who, now()).unwrap();
    add_action(&b.store, cs, &who, Verb::Send, &[&d], &Map::new(), None).unwrap();
    submit(&b.store, cs, &who, "Again.", now()).unwrap();
    let st: String = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT state FROM changeset_action WHERE changeset_id = ?1",
                [cs],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!((st.as_str(), b.sends()), ("denied", 1));
}

#[test]
fn a_rate_limit_of_the_send_is_one_request_and_pending() {
    let b = setup("rate");
    let (s, _) = b.new_mail(AGED);
    b.fake.fail_next_write(&[429], false);
    assert_eq!(b.release(s).unwrap(), Outcome::Pending);
    assert_eq!(b.sends(), 1);
}

/// Acceptance 3: a change at Gmail after the approval, also a new Bcc
/// only, denies the send, and no `drafts/send` request goes out. A Bcc
/// reads back (send content spec, section 4): the row gets it, and a new
/// send of it waits for the Bcc gate (section 6.2). A `Reply-To` does not
/// read back: the row becomes foreign.
#[test]
fn a_draft_changed_at_gmail_is_not_sent() {
    let b = setup("changed");
    let (s, provider) = b.new_mail(AGED);
    let raw = String::from_utf8(b.gmail.draft_raw(TOKEN, &provider).unwrap().raw.unwrap()).unwrap();
    assert!(b.fake.edit_draft(
        &provider,
        raw.replacen("To: ", "Bcc: spy@x.org\r\nTo: ", 1).as_bytes()
    ));
    match b.release(s) {
        Ok(Outcome::Denied(why)) => assert!(why.contains("changed at Gmail"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.sends(), 0);
    let content: String = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT content FROM draft WHERE foreign_at IS NULL",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(content.contains("spy@x.org"), "{content}");
    let (d, _) = b.draft(
        b.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT action_id FROM draft WHERE foreign_at IS NULL",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap(),
    );
    let who = Author {
        kind: AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    };
    let cs = create(&b.store, &who, now()).unwrap();
    add_action(
        &b.store,
        cs,
        &who,
        Verb::Send,
        &[d.as_str()],
        &Map::new(),
        None,
    )
    .unwrap();
    submit(&b.store, cs, &who, "Send.", now()).unwrap();
    let st: String = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT state FROM changeset_action WHERE changeset_id = ?1",
                [cs],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(st, "proposed");
    // Approved, the new send waits for the Bcc gate of Gmail.
    let id: i64 = b
        .store
        .write(|tx| {
            Ok(tx.query_row(
                "UPDATE changeset_action SET state = 'approved', decided_by = 'person',
                   decided_at = ?2 WHERE changeset_id = ?1 RETURNING id",
                params![cs, now() - AGED],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(b.release(id).unwrap(), Outcome::BccUnchecked);
    assert_eq!(b.sends(), 0);

    let (s, provider) = b.new_mail(AGED);
    let raw = String::from_utf8(b.gmail.draft_raw(TOKEN, &provider).unwrap().raw.unwrap()).unwrap();
    assert!(b.fake.edit_draft(
        &provider,
        raw.replacen("To: ", "Reply-To: spy@x.org\r\nTo: ", 1)
            .as_bytes()
    ));
    match b.release(s) {
        Ok(Outcome::Denied(why)) => assert!(why.contains("changed at Gmail"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.sends(), 0);
    let foreign: bool = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT foreign_at IS NOT NULL FROM draft WHERE provider_id = ?1",
                [&provider],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(foreign);

    // A plain edit refreshes the row, and a new send binds to it.
    let (s, provider) = b.new_mail(AGED);
    let raw = String::from_utf8(b.gmail.draft_raw(TOKEN, &provider).unwrap().raw.unwrap()).unwrap();
    assert!(b.fake.edit_draft(
        &provider,
        raw.replace("Here.", "Here, with a change.").as_bytes()
    ));
    assert!(matches!(b.release(s), Ok(Outcome::Denied(_))));
    let d: String = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT 'd-' || ad.draft_id FROM action_draft ad WHERE ad.action_id = ?1",
                [s],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    let again = b.approved(Verb::Send, &d, json!({}), AGED);
    assert!(matches!(b.release(again), Ok(Outcome::Applied(_))));
    assert_eq!(b.sends(), 1);
}

#[test]
fn a_deleted_draft_is_not_sent() {
    let b = setup("deleted");
    let (s, provider) = b.new_mail(AGED);
    assert!(b.fake.delete_draft(&provider));
    match b.release(s) {
        Ok(Outcome::Denied(why)) => assert!(why.contains("gone"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.sends(), 0);
}

/// The send carries the bytes of Iris: an edit at Gmail after the read
/// does not go out (assumption 6).
#[test]
fn the_send_carries_the_approved_bytes() {
    let b = setup("bytes");
    let (s, provider) = b.new_mail(AGED);
    // The person edits the draft after the release reads it and before
    // the `drafts.send` call.
    b.fake
        .edit_draft_after_get(&provider, b"From: <owner@example.com>\r\nTo: <spy@example.org>\r\nSubject: Q4 numbers\r\n\r\nOther text.\r\n");
    assert!(matches!(b.release(s), Ok(Outcome::Applied(_))));
    let (raw, sent): (Vec<u8>, String) = b
        .store
        .read(|c| {
            Ok(c.query_row("SELECT raw, sent_message FROM draft", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?)
        })
        .unwrap();
    assert_eq!(b.raw(&sent).as_bytes(), raw.as_slice());
    assert!(!b.raw(&sent).contains("spy@example.org"));
    // The edit happened: the release read the draft before the send.
    let log = b.fake.requests();
    let read = log
        .iter()
        .rposition(|r| r.starts_with("GET") && r.contains(&format!("/drafts/{provider}")));
    let send = log.iter().rposition(|r| r.ends_with("/drafts/send"));
    assert!(read < send && read.is_some(), "{log:#?}");
    let error: Option<String> = b
        .store
        .read(|c| {
            Ok(
                c.query_row("SELECT error FROM ledger WHERE action_id = ?1", [s], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(error, None);
}

/// A create with an unknown result: Gmail sets its own Message-ID on the
/// draft (spec, assumption 3), so the recovery search by the ID of Iris
/// finds nothing. The row fails, with no second create. A draft that Gmail
/// made stays at Gmail, and the person can delete it (known limit).
#[test]
fn an_unknown_create_fails_with_no_second_create() {
    let b = setup("create");
    for applied in [true, false] {
        let a = b.approved(Verb::Draft, "a-1", mail(), 0);
        let before = b.posts("/drafts");
        b.fake.fail_next_write(&[503], applied);
        assert!(
            matches!(b.apply(a), Ok(Outcome::Failed { .. })),
            "{applied}"
        );
        assert_eq!(b.posts("/drafts"), before + 1);
    }
    assert_eq!(b.fake.draft_ids().len(), 1);
}

/// A create gives the draft row the Message-ID that Gmail set, and the
/// bytes of the row carry it, so the send recovery searches by that ID.
#[test]
fn a_draft_keeps_the_message_id_of_gmail() {
    let b = setup("gmail-id");
    let a = b.approved(Verb::Draft, "a-1", mail(), 0);
    assert!(matches!(b.apply(a), Ok(Outcome::Applied(_))));
    let (id, raw): (String, Vec<u8>) = b
        .store
        .read(|c| {
            Ok(
                c.query_row("SELECT message_id_hdr, raw FROM draft", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?,
            )
        })
        .unwrap();
    assert!(id.ends_with("@mail.gmail.com"), "{id}");
    assert!(String::from_utf8(raw).unwrap().contains(&format!("<{id}>")));
}

/// If the read back after the create fails, the row keeps the ID of Iris.
/// The first release denies the send and refreshes the row with the ID of
/// Gmail (not `foreign_at`), and a new approval sends the draft.
#[test]
fn a_failed_read_back_costs_one_new_approval() {
    let b = setup("read-back");
    let a = b.approved(Verb::Draft, "a-1", mail(), 0);
    b.fake.fail_next_matching("/drafts/r", 404);
    assert!(matches!(b.apply(a), Ok(Outcome::Applied(_))));
    let (d, _) = b.draft(a);
    let s = b.approved(Verb::Send, &d, json!({}), AGED);
    match b.release(s) {
        Ok(Outcome::Denied(why)) => assert!(why.contains("changed"), "{why}"),
        other => panic!("{other:?}"),
    }
    let (foreign, id): (bool, String) = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT foreign_at IS NOT NULL, message_id_hdr FROM draft",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert!(!foreign);
    assert!(id.ends_with("@mail.gmail.com"), "{id}");
    let again = b.approved(Verb::Send, &d, json!({}), AGED);
    assert!(matches!(b.release(again), Ok(Outcome::Applied(_))));
    assert_eq!(b.sends(), 1);
}

#[test]
fn a_reply_whose_parent_is_gone_is_denied() {
    let b = setup("gone");
    let (thread, parent) = b.thread();
    let a = b.approved(
        Verb::Draft,
        &format!("t-{thread}"),
        json!({"to": ["priya@example.com"], "body": "Thanks."}),
        0,
    );
    b.sql("UPDATE message SET gone_at = 1 WHERE id = ?1", [parent]);
    match b.apply(a) {
        Ok(Outcome::Denied(why)) => assert!(why.contains("parent"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.posts("/drafts"), 0);
}

/// A draft that the person made larger than DRAFT_LIMIT at Gmail (for
/// example with an attachment) is not sent, and is marked foreign.
#[test]
fn a_large_draft_at_gmail_is_not_sent() {
    let b = setup("large");
    let (s, provider) = b.new_mail(AGED);
    let mut big = b"Subject: x\r\n\r\n".to_vec();
    big.resize(iris_gmail::DRAFT_LIMIT + 1, b'a');
    assert!(b.fake.edit_draft(&provider, &big));
    assert!(matches!(b.release(s), Ok(Outcome::Denied(_))));
    let foreign: bool = b
        .store
        .read(|c| Ok(c.query_row("SELECT foreign_at IS NOT NULL FROM draft", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!((foreign, b.sends()), (true, 0));
}

/// The apply writes only to the mailbox that the action names: a new mail
/// of another account is denied, and no draft is made in this mailbox.
#[test]
fn a_new_mail_of_another_account_makes_no_draft() {
    let b = setup("other");
    let other = b.store.add_account("gmail", "other@example.com").unwrap();
    let a = b.approved(Verb::Draft, &format!("a-{other}"), mail(), 0);
    match b.apply(a) {
        Ok(Outcome::Denied(why)) => assert!(why.contains("another account"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.posts("/drafts"), 0);
    assert_eq!(b.state(a), "denied");
}

/// The ledger state of each write action, by action.
fn ledger(store: &Store, action: i64) -> (String, i64) {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT state, call FROM ledger WHERE action_id = ?1",
                [action],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

/// A crash after the claim of a create and before its request: the row
/// stays in `writing` with `call` 1. After a restart, the recovery finds
/// no draft at Gmail, fails the row, and never calls `drafts.create`.
#[test]
fn a_crash_after_the_create_claim_never_creates() {
    let b = setup("crash-create");
    let a = b.approved(Verb::Draft, "a-1", mail(), 0);
    // The create does not reach the state of Gmail, and the search of the
    // recovery in the same run fails: the store keeps the state of a crash
    // after the claim (the row in `writing`, `call` 1, no draft at Gmail).
    b.fake.fail_next_write(&[503], false);
    b.fake.fail_next_matching("rfc822msgid", 400);
    assert!(b.apply(a).is_err());
    assert_eq!(ledger(&b.store, a), ("writing".into(), 1));
    let creates = b.posts("/drafts");
    // The restart: a new store handle, then the recovery of the account.
    let store = Store::open(b.store.dir()).unwrap();
    iris_apply::recover(&store, &b.gmail, &mut tok(), b.account).unwrap();
    assert_eq!(ledger(&store, a).0, "failed");
    assert!(matches!(
        apply(&store, &b.gmail, &mut tok(), b.account, a),
        Err(Error::NotApproved(_)) | Ok(Outcome::Failed { .. })
    ));
    assert_eq!(b.posts("/drafts"), creates);
    assert!(b.fake.draft_ids().is_empty());
}

/// A crash after the claim of a send: the row stays in `writing` and the
/// `ledger_send` row keeps the draft. After a restart, neither the
/// recovery nor a new release calls `drafts.send` again.
#[test]
fn a_crash_after_the_send_claim_never_sends() {
    let b = setup("crash-send");
    let (s, _) = b.new_mail(AGED);
    // The send request fails and Gmail does not apply it: the same state
    // at Gmail and in the store as a crash after the claim.
    b.fake.fail_next_write(&[503], false);
    assert_eq!(b.release(s).unwrap(), Outcome::Pending);
    assert_eq!(ledger(&b.store, s), ("writing".into(), 1));
    let sends = b.sends();
    let store = Store::open(b.store.dir()).unwrap();
    for _ in 0..3 {
        iris_apply::recover(&store, &b.gmail, &mut tok(), b.account).unwrap();
        let _ = release(&store, &b.gmail, &mut tok(), b.account, s);
    }
    assert_eq!(ledger(&store, s).0, "writing");
    assert_eq!(b.sends(), sends);
}

/// A flag that appears after the approval denies the send, and no
/// `drafts.send` goes out (docs/specs/2026-11-17-send-checks.md, section 4).
#[test]
fn a_warning_after_the_approval_denies_the_send() {
    let b = setup("grown");
    let text =
        "the quarterly numbers look strong and the board meeting agreed on the new plan for hiring";
    let a = b.approved(
        Verb::Draft,
        "a-1",
        json!({"to": ["priya@example.com"], "subject": "Plan", "body": text}),
        0,
    );
    assert!(matches!(b.apply(a), Ok(Outcome::Applied(_))));
    let (d, _) = b.draft(a);
    let s = b.approved(Verb::Send, &d, json!({}), AGED);
    // Now a received message holds the same words: untrusted_content.
    let ids = b
        .store
        .put_messages(
            b.account,
            &[iris_store::NewMessage {
                provider_id: "echo".into(),
                thread_provider_id: "echo".into(),
                body: Some(iris_store::Body {
                    subject: "Re".into(),
                    sender: "x@example.org".into(),
                    text: text.into(),
                }),
                ..Default::default()
            }],
        )
        .unwrap();
    assert_eq!(ids.len(), 1);
    match b.release(s) {
        Ok(Outcome::Denied(why)) => assert!(why.contains("warnings"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.sends(), 0);
}

/// The subject of a reply comes from Gmail at the apply. If the mirror had
/// another subject at submit (a parent that is not understood yet), the
/// flags of the final content are checked again, and a new flag denies the
/// draft with no `drafts.create` call.
#[test]
fn a_reply_subject_with_a_new_warning_denies_the_draft() {
    let b = setup("subject");
    // The newest message of a thread whose subject names an amount, a date
    // or an attachment.
    let rows: Vec<(i64, i64, String)> = b
        .store
        .read(|c| {
            let mut st = c.prepare(
                "SELECT m.thread_id, m.id, m.provider_id FROM message m
                 WHERE m.gone_at IS NULL
                   AND m.internal_date = (SELECT max(internal_date) FROM message
                                          WHERE thread_id = m.thread_id AND gone_at IS NULL)
                 ORDER BY m.id",
            )?;
            let rows = st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(rows)
        })
        .unwrap();
    let warns = |s: &str| {
        iris_changeset::has_money(s)
            || iris_changeset::has_date(s)
            || iris_changeset::has_attachment(s)
    };
    let (thread, parent, _) = rows
        .into_iter()
        .find(|(_, _, provider)| {
            let head = b
                .gmail
                .metadata(TOKEN, provider, &["Subject", "Message-ID"])
                .unwrap();
            let has = |n: &str| head.headers.iter().find(|h| h.name.eq_ignore_ascii_case(n));
            has("Message-ID").is_some() && has("Subject").is_some_and(|h| warns(&h.value))
        })
        .expect("the synthetic mailbox has a subject with a warning");
    // The mirror does not know the subject yet.
    b.sql(
        "UPDATE header SET subject = NULL WHERE message_id = ?1",
        [parent],
    );
    let a = b.approved(
        Verb::Draft,
        &format!("t-{thread}"),
        json!({"to": ["priya@example.com"], "body": "Thanks."}),
        0,
    );
    let creates = b.posts("/drafts");
    match b.apply(a) {
        Ok(Outcome::Denied(why)) => assert!(why.contains("warnings"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.state(a), "denied");
    assert_eq!(b.posts("/drafts"), creates);
}

/// The held outbox (docs/specs/2026-11-17-held-outbox.md): the changeset of
/// an action.
fn changeset_of(b: &Box, action: i64) -> i64 {
    b.store
        .read(|c| {
            Ok(c.query_row(
                "SELECT changeset_id FROM changeset_action WHERE id = ?1",
                [action],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

/// Acceptance 4: with the switch off, no send leaves, and the send stays
/// held. When the switch goes on, the hold counts from that time.
#[test]
fn no_send_leaves_while_the_switch_is_off() {
    let b = setup("switch");
    let (s, _) = b.new_mail(AGED);
    iris_apply::set_sending(&b.store, false).unwrap();
    assert_eq!(iris_apply::sending(&b.store).unwrap(), None);
    assert_eq!(b.release(s).unwrap(), Outcome::SendingOff);
    assert_eq!((b.state(s).as_str(), b.sends()), ("held", 0));
    // On again: a full hold from now, though the approval is old.
    let before = now();
    iris_apply::set_sending(&b.store, true).unwrap();
    // The switch takes the time of its own write, not an earlier time of
    // the caller.
    let on = iris_apply::sending(&b.store).unwrap().unwrap();
    assert!(on >= before, "{on} < {before}");
    match b.release(s) {
        Ok(Outcome::Hold(until)) => assert!(until >= on + HOLD_MS, "{until}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.sends(), 0);
    // A second "on" keeps the first time, so it does not restart a hold.
    iris_apply::set_sending(&b.store, true).unwrap();
    assert_eq!(iris_apply::sending(&b.store).unwrap(), Some(on));
}

/// Acceptance 2: a cancel of a held send removes it, and no send goes out.
/// A cancel after the claim changes nothing.
#[test]
fn a_cancel_removes_a_held_send() {
    let b = setup("cancel");
    let (s, _) = b.new_mail(AGED);
    iris_changeset::hold(&b.store, b.account, now()).unwrap();
    assert_eq!(b.state(s), "held");
    assert_eq!(
        iris_changeset::cancel(&b.store, s, now()).unwrap(),
        iris_changeset::Cancel::Done
    );
    assert_eq!(b.state(s), "denied");
    assert!(matches!(b.release(s), Err(Error::NotApproved(_))));
    assert_eq!(b.sends(), 0);
    assert_eq!(
        iris_changeset::cancel(&b.store, s, now()).unwrap(),
        iris_changeset::Cancel::NotOpen
    );
    // A send that the release claimed: the cancel says it started.
    let (s2, _) = b.new_mail(AGED);
    assert!(matches!(b.release(s2), Ok(Outcome::Applied(_))));
    assert_eq!(
        iris_changeset::cancel(&b.store, s2, now()).unwrap(),
        iris_changeset::Cancel::Started
    );
    assert_eq!(b.state(s2), "applied");
}

/// A withdraw of the changeset cancels its held send, and no send goes out.
#[test]
fn a_withdraw_cancels_a_held_send() {
    let b = setup("withdraw");
    let (s, _) = b.new_mail(AGED);
    iris_changeset::hold(&b.store, b.account, now()).unwrap();
    iris_changeset::withdraw(&b.store, changeset_of(&b, s), None).unwrap();
    assert_eq!(b.state(s), "cancelled");
    assert!(matches!(b.release(s), Err(Error::NotApproved(_))));
    assert_eq!(b.sends(), 0);
}

/// Acceptance 5: an approval 14 days after its submit never moves to held,
/// and a held send expires the same way.
#[test]
fn an_expired_send_never_leaves() {
    let b = setup("expiry");
    let old = now() - 15 * 24 * 60 * 60 * 1000;
    let mail = |b: &Box| {
        let a = b.approved(Verb::Draft, "a-1", mail(), 0);
        assert!(matches!(b.apply(a), Ok(Outcome::Applied(_))));
        b.draft(a).0
    };
    // An approved send, 15 days after its submit: expired, never held.
    let d = mail(&b);
    let s = b.approved_after(Verb::Send, &d, json!({}), AGED, old);
    iris_changeset::hold(&b.store, b.account, now()).unwrap();
    assert_eq!(b.state(s), "expired");
    assert!(matches!(b.release(s), Err(Error::NotApproved(_))));
    // A held send expires the same way at the expiry tick (the switch was
    // off for 15 days).
    let d = mail(&b);
    let s2 = b.approved_after(Verb::Send, &d, json!({}), AGED, old);
    b.sql(
        "UPDATE changeset_action SET state = 'held' WHERE id = ?1",
        [s2],
    );
    assert_eq!(b.state(s2), "held");
    iris_changeset::expire(&b.store, now()).unwrap();
    assert_eq!(b.state(s2), "expired");
    assert_eq!(b.sends(), 0);
}

/// Acceptance 3: `sending off` waits for the apply lock, which a release
/// holds during its call, and writes the switch only after it.
#[test]
fn sending_off_waits_for_a_send_in_progress() {
    let b = setup("off-wait");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(b.store.dir().join(iris_apply::LOCK_FILE))
        .unwrap();
    file.try_lock().unwrap();
    let start = std::time::Instant::now();
    let holder = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        drop(file);
    });
    iris_apply::set_sending(&b.store, false).unwrap();
    assert!(start.elapsed() >= Duration::from_millis(300));
    assert_eq!(iris_apply::sending(&b.store).unwrap(), None);
    holder.join().unwrap();
}

/// The apply step applies an approved draft, then releases an approved
/// send whose hold ended: one `drafts.send` (held-outbox spec, section 4).
#[test]
fn the_step_makes_the_draft_and_sends_it() {
    let b = setup("step-send");
    let a = b.approved(Verb::Draft, "a-1", mail(), 0);
    let first = b.step();
    assert!(
        matches!(first.results[..], [(x, Ok(Outcome::Applied(_)))] if x == a),
        "{:?}",
        first.results
    );
    let (d, _) = b.draft(a);
    let s = b.approved(Verb::Send, &d, json!({}), AGED);
    let second = b.step();
    assert!(
        matches!(second.results[..], [(x, Ok(Outcome::Applied(_)))] if x == s),
        "{:?}",
        second.results
    );
    assert_eq!((b.state(s).as_str(), b.sends()), ("applied", 1));
    // A third step finds nothing to do.
    let third = b.step();
    assert!(third.results.is_empty() && third.next.is_none());
    assert_eq!(b.sends(), 1);
}

/// A new approval waits in the outbox, and the step names the end of its
/// hold as the time to wake.
#[test]
fn the_step_holds_a_new_send_and_names_its_end() {
    let b = setup("step-hold");
    let (s, _) = b.new_mail(0);
    let before = now();
    let out = b.step();
    assert!(out.results.is_empty(), "{:?}", out.results);
    let next = out.next.expect("a time to wake");
    assert!(next > before && next <= now() + HOLD_MS, "{next}");
    assert_eq!((b.state(s).as_str(), b.sends()), ("held", 0));
    // Issue #421: the document of a changeset with a held send agrees with
    // the published schema.
    let mut out = Vec::new();
    iris_changeset::write_document(&b.store, changeset_of(&b, s), &mut out).unwrap();
    let doc: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(doc["state"], "open", "{doc}");
    assert_eq!(doc["actions"][0]["decision"]["state"], "held", "{doc}");
    let errors: Vec<String> = iris_spec::validator("changeset")
        .iter_errors(&doc)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    // Only a send can be held: the held rule names the verb of another
    // verb, and only while the action is held.
    let verb_error = |state: &str| {
        let mut other = doc.clone();
        other["actions"][0]["verb"] = json!("archive");
        other["actions"][0]["decision"]["state"] = json!(state);
        let found = iris_spec::validator("changeset")
            .iter_errors(&other)
            .any(|e| {
                e.instance_path().to_string() == "/actions/0/verb" && e.to_string().contains("send")
            });
        found
    };
    assert!(verb_error("held"));
    assert!(!verb_error("approved"));
}

/// While the switch is off, the step holds the send, makes no call and
/// names no time to wake.
#[test]
fn the_step_sends_nothing_while_the_switch_is_off() {
    let b = setup("step-off");
    let (s, _) = b.new_mail(AGED);
    iris_apply::set_sending(&b.store, false).unwrap();
    let out = b.step();
    assert!(out.results.is_empty() && out.next.is_none(), "{out:?}");
    assert_eq!((b.state(s).as_str(), b.sends()), ("held", 0));
}

/// Issue #400: a send whose response was lost stays in `writing`. The
/// switch then goes off, and nothing else waits. The step still runs the
/// recovery, so the send that left Gmail becomes `applied`, with no new
/// call.
#[test]
fn the_step_recovers_a_lost_send_while_the_switch_is_off() {
    let b = setup("step-recover");
    let (s, _) = b.new_mail(AGED);
    // The send goes out, its response is lost, and the search of the
    // recovery fails: the row stays in `writing`.
    b.fake.fail_next_write(&[503], true);
    b.fake.fail_next_matching("rfc822msgid", 400);
    assert!(b.release(s).is_err());
    assert_eq!((ledger(&b.store, s).0.as_str(), b.sends()), ("writing", 1));
    iris_apply::set_sending(&b.store, false).unwrap();
    let out = b.step();
    assert!(out.results.is_empty(), "{:?}", out.results);
    assert_eq!((b.state(s).as_str(), b.sends()), ("applied", 1));
}

/// Issue #420: a writer that holds the store delays `iris sending on`. The
/// hold then counts from the end of that wait, not from the call.
#[test]
fn sending_on_after_a_wait_gets_a_full_hold() {
    let b = setup("switch-wait");
    let (s, _) = b.new_mail(AGED);
    iris_apply::set_sending(&b.store, false).unwrap();
    let store = std::sync::Arc::new(Store::open(b.store.dir()).unwrap());
    let (go, started) = std::sync::mpsc::channel();
    let blocker = {
        let store = store.clone();
        std::thread::spawn(move || {
            store
                .write(|_| {
                    go.send(()).unwrap();
                    std::thread::sleep(Duration::from_millis(400));
                    Ok(now())
                })
                .unwrap()
        })
    };
    started.recv().unwrap();
    iris_apply::set_sending(&store, true).unwrap();
    let freed = blocker.join().unwrap();
    let on = iris_apply::sending(&b.store).unwrap().unwrap();
    assert!(
        on >= freed,
        "on_at {on} is before the writer ended at {freed}"
    );
    match b.release(s) {
        Ok(Outcome::Hold(until)) => assert!(until >= freed + HOLD_MS, "{until}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.sends(), 0);
}

/// While stopped, no held send leaves: not by the release, not by the step.
/// After the resume, each held send waits a full hold again
/// (docs/specs/2026-11-17-stop.md).
#[test]
fn no_send_leaves_while_stopped_and_resume_gets_a_full_hold() {
    let b = setup("stop");
    let (s, _) = b.new_mail(AGED);
    iris_apply::stop(&b.store).unwrap();
    assert_eq!(b.release(s).unwrap(), Outcome::Stopped);
    let step = b.step();
    assert!(step.results.is_empty(), "{:?}", step.results);
    assert_eq!(b.sends(), 0);
    assert_eq!(b.state(s), "held");
    let resumed = now();
    assert!(iris_apply::resume(&b.store).unwrap());
    // A second resume changes nothing: the hold does not move again.
    let on = iris_apply::sending(&b.store).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    assert!(!iris_apply::resume(&b.store).unwrap());
    assert_eq!(iris_apply::sending(&b.store).unwrap(), on);
    match b.release(s) {
        Ok(Outcome::Hold(until)) => assert!(until >= resumed + HOLD_MS, "{until}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.sends(), 0);
}

/// The step still runs the recovery while stopped: a send whose answer was
/// lost gets its true state in the ledger.
#[test]
fn the_step_recovers_a_lost_send_while_stopped() {
    let b = setup("stop-recover");
    let (s, _) = b.new_mail(AGED);
    // The send goes out, its response is lost, and the search of the
    // recovery fails: the row stays in `writing`.
    b.fake.fail_next_write(&[503], true);
    b.fake.fail_next_matching("rfc822msgid", 400);
    assert!(b.release(s).is_err());
    assert_eq!((ledger(&b.store, s).0.as_str(), b.sends()), ("writing", 1));
    iris_apply::stop(&b.store).unwrap();
    let out = b.step();
    assert!(out.results.is_empty(), "{:?}", out.results);
    assert_eq!((b.state(s).as_str(), b.sends()), ("applied", 1));
}

/// A stop and a resume during the read of the draft at Gmail, after the
/// first hold check of the release: the claim reads the new hold, and the
/// send waits a full hold again (docs/specs/2026-11-17-stop.md, section 3).
#[test]
fn a_stop_and_resume_during_the_release_restart_the_hold() {
    let b = setup("stop-race");
    let (s, provider) = b.new_mail(AGED);
    let other = Store::open(b.store.dir()).unwrap();
    let resumed = std::sync::Arc::new(std::sync::Mutex::new(0));
    let at = resumed.clone();
    b.fake.before_next_matching(
        &format!("GET /gmail/v1/users/me/drafts/{provider}"),
        move || {
            // The release holds the apply lock: no wait for it.
            iris_apply::stop_wait(&other, Duration::ZERO).unwrap();
            *at.lock().unwrap() = now();
            assert!(iris_apply::resume(&other).unwrap());
        },
    );
    match b.release(s) {
        Ok(Outcome::Hold(until)) => assert!(until >= *resumed.lock().unwrap() + HOLD_MS, "{until}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.sends(), 0);
    assert_eq!(b.state(s), "held");
}

/// A writer that holds the store delays `iris resume`. The hold then counts
/// from the end of that wait, not from the call (as issue #420).
#[test]
fn resume_after_a_wait_gets_a_full_hold() {
    let b = setup("resume-wait");
    let (s, _) = b.new_mail(AGED);
    iris_apply::stop(&b.store).unwrap();
    let store = std::sync::Arc::new(Store::open(b.store.dir()).unwrap());
    let (go, started) = std::sync::mpsc::channel();
    let blocker = {
        let store = store.clone();
        std::thread::spawn(move || {
            store
                .write(|_| {
                    go.send(()).unwrap();
                    std::thread::sleep(Duration::from_millis(400));
                    Ok(now())
                })
                .unwrap()
        })
    };
    started.recv().unwrap();
    assert!(iris_apply::resume(&store).unwrap());
    let freed = blocker.join().unwrap();
    match b.release(s) {
        Ok(Outcome::Hold(until)) => assert!(until >= freed + HOLD_MS, "{until}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.sends(), 0);
}

/// Send content spec, sections 2 and 4: a draft with a file from the
/// attachment folder goes to Gmail with the file, reads back with the
/// approved hash, and the release sends it.
#[test]
fn a_draft_with_a_file_is_created_and_sent() {
    let b = setup("file");
    let file = b"%PDF-1 the numbers".as_slice();
    let sha = b.store.put_attachment(file).unwrap();
    let mut m = mail();
    m["attachments"] =
        json!([{"name": "q4.pdf", "type": "application/pdf", "sha256": sha, "size": file.len()}]);
    let a = b.approved(Verb::Draft, "a-1", m, 0);
    assert!(matches!(b.apply(a), Ok(Outcome::Applied(_))));
    let (d, provider) = b.draft(a);
    let raw = b.gmail.draft_raw(TOKEN, &provider).unwrap().raw.unwrap();
    let back = iris_compose::read(&raw, OWNER).unwrap();
    assert_eq!(back.files, [file.to_vec()]);
    assert_eq!(back.content.attachments[0].name, "q4.pdf");
    let s = b.approved(Verb::Send, &d, json!({}), AGED);
    assert!(
        matches!(b.release(s), Ok(Outcome::Applied(_))),
        "{:?}",
        b.state(s)
    );
    assert_eq!(b.sends(), 1);
}

/// A file that is missing or changed in the attachment folder denies the
/// apply, and no `drafts.create` goes out.
#[test]
fn a_missing_or_changed_file_denies_the_draft() {
    let b = setup("nofile");
    let file = b"the file".as_slice();
    let sha = b.store.put_attachment(file).unwrap();
    for (sha, size) in [("ab".repeat(32), file.len()), (sha.clone(), file.len() + 1)] {
        let mut m = mail();
        m["attachments"] =
            json!([{"name": "a.txt", "type": "text/plain", "sha256": sha, "size": size}]);
        let a = b.approved(Verb::Draft, "a-1", m, 0);
        match b.apply(a) {
            Ok(Outcome::Denied(why)) => assert!(why.contains("attachment file"), "{why}"),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(b.posts("/drafts"), 0);
}

/// Send content spec, section 3: the From of a Gmail draft has the name of
/// the newest sent message of the owner; a name with a control character
/// is not used.
#[test]
fn a_gmail_draft_has_the_display_name_of_the_owner() {
    let b = setup("name");
    let from = |a: i64| {
        let (_, provider) = b.draft(a);
        let raw = b.gmail.draft_raw(TOKEN, &provider).unwrap().raw.unwrap();
        let raw = String::from_utf8(raw).unwrap();
        raw.lines()
            .find(|l| l.starts_with("From:"))
            .unwrap()
            .to_string()
    };
    let set_name = |name: &str| {
        b.store
            .write(|tx| {
                // The newest message of the mirror becomes a sent message of the owner.
                let m: i64 = tx.query_row(
                    "SELECT id FROM message WHERE gone_at IS NULL ORDER BY internal_date DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )?;
                tx.execute(
                    "INSERT OR IGNORE INTO label(account_id, provider_id, name, kind) VALUES (?1, 'SENT', 'SENT', 'system')",
                    [b.account],
                )?;
                tx.execute(
                    "INSERT OR IGNORE INTO message_label(message_id, label_id)
                     SELECT ?1, id FROM label WHERE provider_id = 'SENT' AND account_id = ?2",
                    params![m, b.account],
                )?;
                tx.execute(
                    "UPDATE header SET from_addr = ?2, from_name = ?3 WHERE message_id = ?1",
                    params![m, OWNER, name],
                )?;
                Ok(())
            })
            .unwrap()
    };
    set_name("Dana Owner");
    let a = b.approved(Verb::Draft, "a-1", mail(), 0);
    assert!(matches!(b.apply(a), Ok(Outcome::Applied(_))));
    assert_eq!(from(a), "From: \"Dana Owner\" <owner@example.com>");
    set_name("Dana\r\nBcc: spy@x.org");
    let a = b.approved(Verb::Draft, "a-1", mail(), 0);
    assert!(matches!(b.apply(a), Ok(Outcome::Applied(_))));
    assert_eq!(from(a), "From: <owner@example.com>");
}

/// Send content spec, section 5: the submit denies a draft whose size
/// bound is over the limit of the provider of the account.
#[test]
fn the_size_limit_is_the_limit_of_the_provider() {
    let b = setup("size");
    let who = Author {
        kind: AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    };
    let state_of = |args: Value| -> String {
        let cs = create(&b.store, &who, now()).unwrap();
        let args = args.as_object().unwrap().clone();
        add_action(&b.store, cs, &who, Verb::Draft, &["a-1"], &args, None).unwrap();
        submit(&b.store, cs, &who, "Write.", now()).unwrap();
        b.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT state FROM changeset_action WHERE changeset_id = ?1",
                    [cs],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    };
    // About 3 MB with base64: more than Outlook takes, less than Gmail.
    let mut m = mail();
    m["attachments"] = json!([{"name": "big.bin", "type": "application/octet-stream", "sha256": "ab".repeat(32), "size": 2_200_000}]);
    assert_ne!(state_of(m.clone()), "denied");
    b.store
        .write(|tx| Ok(tx.execute("UPDATE account SET provider = 'outlook'", [])?))
        .unwrap();
    assert_eq!(state_of(m), "denied");
}

/// Send content spec, section 6.2: the `Bcc` gate of each provider.
fn bcc_mail(cc: &[&str]) -> Value {
    json!({"to": ["priya@example.com"], "cc": cc, "bcc": ["sam@example.org"],
           "subject": "Q4 numbers", "body": "Hi Priya,\n\nHere.\n"})
}

impl Box {
    /// The send of a new mail with `args`, approved `age` milliseconds ago.
    fn send_of(&self, args: Value, age: i64) -> i64 {
        let a = self.approved(Verb::Draft, "a-1", args, 0);
        assert!(matches!(self.apply(a), Ok(Outcome::Applied(_))));
        let (d, _) = self.draft(a);
        self.approved(Verb::Send, &d, json!({}), age)
    }

    fn ledger_note(&self, action: i64) -> Option<String> {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT error FROM ledger WHERE action_id = ?1",
                    [action],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }
}

#[test]
fn a_send_with_bcc_waits_for_the_bcc_gate() {
    let b = setup("bcc-gate");
    let s = b.send_of(bcc_mail(&[]), AGED);
    assert_eq!(b.release(s).unwrap(), Outcome::BccUnchecked);
    assert_eq!((b.state(s).as_str(), b.sends()), ("held", 0));
    // No test yet: the gate does not open.
    assert!(!iris_apply::bcc_open(&b.store, "outlook").unwrap());
    // A send with no bcc does not read the gate.
    let (plain, _) = b.new_mail(AGED);
    assert!(matches!(b.release(plain), Ok(Outcome::Applied(_))));
    // The owner check: only the test send passes the closed gate.
    assert!(
        iris_apply::bcc_test(&b.store, "gmail", "Priya@Example.com", "sam@example.org").unwrap()
    );
    let other = b.send_of(bcc_mail(&["lee@example.org"]), AGED);
    assert_eq!(b.release(other).unwrap(), Outcome::BccUnchecked);
    assert!(matches!(b.release(s), Ok(Outcome::Applied(_))));
    assert_eq!(b.ledger_note(s), None);
    // The gate of Outlook stays closed. An open gate takes no new test.
    assert!(iris_apply::bcc_open(&b.store, "gmail").unwrap());
    assert!(!iris_apply::bcc_test(&b.store, "gmail", "a@example.org", "b@example.org").unwrap());
    assert!(matches!(b.release(other), Ok(Outcome::Applied(_))));
    assert_eq!(b.sends(), 3);
}

/// A closed gate does not hide a change at Gmail: release reads the draft
/// first, so an edit that removes the Bcc denies the send and refreshes
/// the row, and a new send of the edited draft goes out.
#[test]
fn a_closed_bcc_gate_still_sees_a_change_at_gmail() {
    let b = setup("bcc-edit");
    let a = b.approved(Verb::Draft, "a-1", bcc_mail(&[]), 0);
    assert!(matches!(b.apply(a), Ok(Outcome::Applied(_))));
    let d = b.draft(a);
    let s = b.approved(Verb::Send, &d.0, json!({}), AGED);
    let raw = String::from_utf8(b.gmail.draft_raw(TOKEN, &d.1).unwrap().raw.unwrap()).unwrap();
    let edited: String = raw
        .lines()
        .filter(|l| !l.starts_with("Bcc:"))
        .map(|l| format!("{l}\r\n"))
        .collect();
    assert!(b.fake.edit_draft(&d.1, edited.as_bytes()));
    match b.release(s) {
        Ok(Outcome::Denied(why)) => assert!(why.contains("changed at Gmail"), "{why}"),
        other => panic!("{other:?}"),
    }
    let again = b.approved(Verb::Send, &d.0, json!({}), AGED);
    assert!(matches!(b.release(again), Ok(Outcome::Applied(_))));
    assert_eq!(b.sends(), 1);
}

/// The gate takes only a test that a draft can make: two valid, different
/// addresses, and neither an account of the provider.
#[test]
fn the_bcc_gate_refuses_a_test_that_cannot_test_it() {
    let b = setup("bcc-bad-test");
    let own = OWNER.to_ascii_uppercase();
    for (to, bcc) in [
        ("alice@example", "sam@example.org"),
        ("priya@example.com", "sam example.org"),
        ("Sam@example.org", "sam@example.org"),
        (own.as_str(), "sam@example.org"),
        ("priya@example.com", OWNER),
    ] {
        match iris_apply::bcc_test(&b.store, "gmail", to, bcc) {
            Err(iris_apply::Error::BccTest(_)) => {}
            other => panic!("{to} {bcc}: {other:?}"),
        }
    }
    // No test row: the gate cannot open.
    assert!(!iris_apply::bcc_open(&b.store, "gmail").unwrap());
    // The address of a Gmail account is a valid test for Outlook.
    assert!(iris_apply::bcc_test(&b.store, "outlook", OWNER, "sam@example.org").unwrap());
}

#[test]
fn a_sent_copy_with_no_bcc_takes_the_approved_bcc_with_a_note() {
    let b = setup("bcc-note");
    assert!(iris_apply::bcc_test(&b.store, "gmail", "x@example.org", "y@example.org").unwrap());
    assert!(iris_apply::bcc_open(&b.store, "gmail").unwrap());
    b.fake.drop_sent_bcc();
    let s = b.send_of(bcc_mail(&[]), AGED);
    assert!(matches!(b.release(s), Ok(Outcome::Applied(_))));
    assert_eq!(b.ledger_note(s).as_deref(), Some(iris_apply::NO_BCC));
    // The send stops no later release: the copy is the approved content.
    let (plain, _) = b.new_mail(AGED);
    assert!(matches!(b.release(plain), Ok(Outcome::Applied(_))));
    assert_eq!(b.ledger_note(plain), None);
}

impl Box {
    /// A new mail draft at Gmail and a person change of it, approved.
    /// Gives the change action, the `d-N` ID and the Gmail draft ID.
    fn changed(&self, args: Value) -> (i64, String, String) {
        self.changed_from(mail(), args)
    }

    /// As [`Box::changed`], from a new mail with the content `first`.
    fn changed_from(&self, first: Value, args: Value) -> (i64, String, String) {
        iris_changeset::allow_draft_change(true);
        let a = self.approved(Verb::Draft, "a-1", first, 0);
        assert!(matches!(self.apply(a), Ok(Outcome::Applied(_))));
        let (d, provider) = self.draft(a);
        (self.approved(Verb::Draft, &d, args, 0), d, provider)
    }

    fn puts(&self) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|r| r.starts_with("PUT"))
            .count()
    }

    /// The hash and the content of draft row 1.
    fn row(&self) -> (String, String) {
        self.store
            .read(|c| {
                Ok(
                    c.query_row("SELECT hash, content FROM draft WHERE id = 1", [], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })?,
                )
            })
            .unwrap()
    }

    /// The state, the call count, the undo and the error of the ledger
    /// entry of `action`.
    fn entry(&self, action: i64) -> (String, i64, String, Option<String>) {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT state, call, undo, error FROM ledger WHERE action_id = ?1",
                    [action],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?)
            })
            .unwrap()
    }
}

fn new_text() -> Value {
    json!({"to": ["priya@example.com"], "subject": "Q4 numbers", "body": "New text.\n"})
}

/// A change of a draft (docs/specs/2026-11-25-draft-change.md, section 2):
/// one `drafts.update` of the same Gmail draft, no new draft, and the
/// draft row keeps its ID with the new content.
#[test]
fn a_change_updates_the_draft_in_place() {
    let b = setup("change");
    let (change, d, provider) = b.changed(new_text());
    let (old_hash, _) = b.row();
    let creates = b.posts("/drafts");
    let o = b.apply(change);
    assert!(matches!(o, Ok(Outcome::Applied(_))), "{o:?}");
    assert_eq!(b.puts(), 1);
    assert_eq!(b.posts("/drafts"), creates, "no new draft");
    assert_eq!(b.state(change), "applied");
    let (hash, content) = b.row();
    assert_ne!(hash, old_hash);
    assert!(content.contains("New text."), "{content}");
    let raw = b.gmail.draft_raw(TOKEN, &provider).unwrap().raw.unwrap();
    assert!(String::from_utf8_lossy(&raw).contains("New text."));
    assert_eq!(
        b.entry(change),
        ("applied".into(), 1, "inverse".into(), None)
    );
    let (ops, pre): (String, String) = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT ops, pre FROM ledger WHERE action_id = ?1",
                [change],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    let ops: Value = serde_json::from_str(&ops).unwrap();
    assert_eq!(ops["call"], "drafts.update");
    assert_eq!(ops["pre_hash"], json!(old_hash));
    assert_eq!(ops["hash"], json!(hash));
    assert_eq!(format!("d-{}", ops["draft"]), d);
    let pre: Value = serde_json::from_str(&pre).unwrap();
    assert_eq!(pre["draft"]["content"]["body"], "Hi Priya,\n\nHere.\n");
    assert!(pre["date"].is_i64(), "{pre}");
    // A send binds to the new content.
    let send = b.approved(Verb::Send, &d, json!({}), AGED);
    assert!(
        matches!(b.release(send), Ok(Outcome::Applied(_))),
        "{}",
        b.state(send)
    );
}

/// The same content again: no call, a final entry with no undo.
#[test]
fn the_same_content_again_makes_no_call() {
    let b = setup("change-same");
    let (change, _, _) = b.changed(mail());
    let before = b.row();
    let o = b.apply(change);
    assert!(matches!(o, Ok(Outcome::Applied(_))), "{o:?}");
    assert_eq!(b.puts(), 0);
    assert_eq!(b.row(), before);
    assert_eq!(b.entry(change), ("applied".into(), 0, "none".into(), None));
}

/// The draft at Gmail is not the content of the row: no call. The row
/// refreshes from Gmail, and the action is denied.
#[test]
fn a_draft_changed_at_gmail_is_not_changed() {
    let b = setup("change-gmail");
    let (change, _, provider) = b.changed(new_text());
    let raw = b.gmail.draft_raw(TOKEN, &provider).unwrap().raw.unwrap();
    let edited = String::from_utf8(raw)
        .unwrap()
        .replace("Here.", "Edited in Gmail.");
    assert!(b.fake.edit_draft(&provider, edited.as_bytes()));
    match b.apply(change) {
        Ok(Outcome::Denied(why)) => assert_eq!(why, iris_apply::CHANGE_CHANGED),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.puts(), 0);
    assert!(b.row().1.contains("Edited in Gmail."), "{}", b.row().1);
}

/// A draft that is gone at Gmail: no call, the action is denied.
#[test]
fn a_gone_draft_is_denied() {
    let b = setup("change-gone");
    let (change, _, provider) = b.changed(new_text());
    assert!(b.fake.delete_draft(&provider));
    match b.apply(change) {
        Ok(Outcome::Denied(why)) => assert_eq!(why, iris_apply::CHANGE_GONE),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.puts(), 0);
}

/// A definite rejection: the entry fails, and the draft row does not change.
#[test]
fn a_rejected_change_leaves_the_draft() {
    let b = setup("change-rejected");
    let (change, _, _) = b.changed(new_text());
    let before = b.row();
    b.fake.fail_next_write(&[400], false);
    let o = b.apply(change);
    assert!(matches!(o, Ok(Outcome::Failed { .. })), "{o:?}");
    assert_eq!(b.row(), before);
    assert_eq!(b.entry(change).0, "failed");
}

/// An unknown result (section 4): recovery reads the draft at Gmail and
/// never calls again. The new hash: the change applied. The old hash:
/// Iris does not know, and the entry fails.
#[test]
fn a_lost_answer_is_recovered_with_one_call() {
    let b = setup("change-lost");
    let (change, _, _) = b.changed(new_text());
    b.fake.fail_next_write(&[503], true);
    let o = b.apply(change);
    assert!(matches!(o, Ok(Outcome::Applied(_))), "{o:?}");
    assert_eq!(b.puts(), 1);
    assert!(b.row().1.contains("New text."));

    let b = setup("change-lost-not");
    let (change, _, _) = b.changed(new_text());
    let before = b.row();
    b.fake.fail_next_write(&[503], false);
    match b.apply(change) {
        Ok(Outcome::Failed { error, .. }) => assert_eq!(error, iris_apply::CHANGE_UNKNOWN),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.puts(), 1, "recovery does not call again");
    assert_eq!(b.row().0, before.0);
    assert_eq!(b.entry(change).0, "failed");
}

/// A reply change keeps the subject and the reply headers of its draft,
/// and stays in its thread.
#[test]
fn a_reply_change_stays_in_its_thread() {
    iris_changeset::allow_draft_change(true);
    let b = setup("change-reply");
    let (thread, parent) = b.thread();
    // The parent is understood: the mirror has the subject of Gmail, so
    // submit flags the same subject as the apply.
    let provider: String = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT provider_id FROM message WHERE id = ?1",
                [parent],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    let head = b.gmail.metadata(TOKEN, &provider, &["Subject"]).unwrap();
    b.sql(
        "UPDATE header SET subject = ?1 WHERE message_id = ?2",
        rusqlite::params![head.headers[0].value, parent],
    );
    let a = b.approved(
        Verb::Draft,
        &format!("t-{thread}"),
        json!({"to": ["priya@example.com"], "body": "Thanks."}),
        0,
    );
    let o = b.apply(a);
    assert!(matches!(o, Ok(Outcome::Applied(_))), "{o:?}");
    let (d, provider) = b.draft(a);
    let old = String::from_utf8(b.gmail.draft_raw(TOKEN, &provider).unwrap().raw.unwrap()).unwrap();
    let header = |raw: &str, name: &str| {
        raw.lines()
            .find(|l| l.to_ascii_lowercase().starts_with(&format!("{name}:")))
            .map(str::to_string)
    };
    let change = b.approved(
        Verb::Draft,
        &d,
        json!({"to": ["priya@example.com"], "body": "Thanks again."}),
        0,
    );
    let o = b.apply(change);
    assert!(matches!(o, Ok(Outcome::Applied(_))), "{o:?}");
    let at = b.gmail.draft_raw(TOKEN, &provider).unwrap();
    let new = String::from_utf8(at.raw.unwrap()).unwrap();
    assert!(new.contains("Thanks again."));
    for name in ["subject", "in-reply-to"] {
        assert_eq!(header(&new, name), header(&old, name), "{name}");
    }
    let thread_provider: String = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT provider_id FROM thread WHERE id = ?1",
                [thread],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(at.message.thread_id, thread_provider);
}

/// A reply change answers the parent that its approval binds. A newer
/// mail in the thread becomes the parent at submit, and the draft still
/// answers the old message: no call (change spec, section 1).
#[test]
fn a_reply_change_after_a_new_mail_is_denied() {
    iris_changeset::allow_draft_change(true);
    let b = setup("change-newer");
    let (thread, parent) = b.thread();
    let (provider, thread_provider, date): (String, String, i64) = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT m.provider_id, t.provider_id, m.internal_date FROM message m
                 JOIN thread t ON t.id = m.thread_id WHERE m.id = ?1",
                [parent],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    let head = b.gmail.metadata(TOKEN, &provider, &["Subject"]).unwrap();
    b.sql(
        "UPDATE header SET subject = ?1 WHERE message_id = ?2",
        rusqlite::params![head.headers[0].value, parent],
    );
    let a = b.approved(
        Verb::Draft,
        &format!("t-{thread}"),
        json!({"to": ["priya@example.com"], "body": "Thanks."}),
        0,
    );
    let o = b.apply(a);
    assert!(matches!(o, Ok(Outcome::Applied(_))), "{o:?}");
    let (d, _) = b.draft(a);
    // A new mail of priya, at Gmail and in the thread of the mirror.
    let raw = format!(
        "From: priya@example.com\r\nTo: {OWNER}\r\nSubject: {}\r\nMessage-ID: <newer@example.com>\r\nDate: Wed, 25 Nov 2026 10:00:00 +0000\r\n\r\nOne more thing.\r\n",
        head.headers[0].value
    );
    let id = b.fake.add_message(raw.as_bytes(), &["INBOX"]);
    b.store
        .put_messages(
            b.account,
            &[iris_store::NewMessage {
                provider_id: id,
                thread_provider_id: thread_provider,
                internal_date: Some(date + 1),
                header: iris_store::Header {
                    from_addr: Some("priya@example.com".into()),
                    subject: Some(head.headers[0].value.clone()),
                    message_id_hdr: Some("<newer@example.com>".into()),
                    ..Default::default()
                },
                ..Default::default()
            }],
        )
        .unwrap();
    b.sql("UPDATE message SET hostile = NULL", []);
    let change = b.approved(
        Verb::Draft,
        &d,
        json!({"to": ["priya@example.com"], "body": "Thanks again."}),
        0,
    );
    match b.apply(change) {
        Ok(Outcome::Denied(why)) => assert_eq!(why, iris_apply::CHANGE_NEW_MAIL),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.puts(), 0);
}

/// An attachment file of the stored draft is gone: no call, because the
/// undo could not compose the old draft again.
#[test]
fn a_change_needs_the_files_of_the_old_draft() {
    let b = setup("change-old-file");
    let file = b"the file".as_slice();
    let sha = b.store.put_attachment(file).unwrap();
    let mut m = mail();
    m["attachments"] =
        json!([{"name": "a.txt", "type": "text/plain", "sha256": sha, "size": file.len()}]);
    let (change, _, _) = b.changed_from(m, new_text());
    std::fs::remove_file(b.store.attachment_path(&sha).unwrap()).unwrap();
    match b.apply(change) {
        Ok(Outcome::Denied(why)) => assert_eq!(why, iris_apply::CHANGE_OLD_FILE),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.puts(), 0);
}

/// A stop after the claim (change spec, section 4): the entry stays in
/// `writing`, and a new process recovers it from the store with no second
/// call. The new hash at Gmail ends `applied`; the old hash ends `failed`.
#[test]
fn a_change_in_writing_recovers_after_a_restart() {
    for applied in [true, false] {
        let b = setup(&format!("change-restart-{applied}"));
        let (change, _, _) = b.changed(new_text());
        let before = b.row();
        // The read after the call fails, so the apply stops with the entry
        // in `writing`, as a crash does.
        let fake = b.fake.clone();
        b.fake.before_next_matching("PUT", move || {
            if !applied {
                fake.fail_next_write(&[503], false);
            }
            fake.fail_next_matching("GET /gmail/v1/users/me/drafts/", 400);
        });
        let o = b.apply(change);
        assert!(matches!(o, Ok(Outcome::Pending)), "{o:?}");
        assert_eq!(b.entry(change).0, "writing");
        // A new process: a new store handle and a new client.
        let store = Store::open(&b.dir).unwrap();
        let gmail = Gmail::new(Config {
            base_url: b.fake.base_url(),
            ..Config::default()
        });
        let (t, mut renew) = b.token();
        let mut token = Token {
            token: t,
            renew: &mut renew,
            refused: false,
        };
        iris_apply::recover_step(&store, &gmail, &mut token, b.account).unwrap();
        assert_eq!(b.puts(), 1, "recovery does not call again");
        if applied {
            assert_eq!(b.entry(change).0, "applied");
            assert!(b.row().1.contains("New text."));
        } else {
            let (state, _, _, error) = b.entry(change);
            assert_eq!(state, "failed");
            assert_eq!(error.as_deref(), Some(iris_apply::CHANGE_UNKNOWN));
            assert_eq!(b.row().0, before.0);
        }
    }
}

impl Box {
    /// Move the time of the ledger entry of `action` back by `ms`, as a
    /// clock does. `ledger_change` forbids a change of `at`: drop and
    /// remake the trigger in this test store only.
    fn age_entry(&self, action: i64, ms: i64) {
        self.store
            .write(|tx| {
                let sql: String = tx.query_row(
                    "SELECT sql FROM sqlite_schema WHERE name = 'ledger_change'",
                    [],
                    |r| r.get(0),
                )?;
                tx.execute_batch("DROP TRIGGER ledger_change")?;
                tx.execute(
                    "UPDATE ledger SET at = at - ?2 WHERE action_id = ?1",
                    params![action, ms],
                )?;
                tx.execute_batch(&sql)?;
                Ok(())
            })
            .unwrap();
    }
}

/// The person edits the draft in Gmail between the update and the read
/// back (change spec, section 7): the entry fails with the reason of the
/// window, and the row keeps the text of the person.
#[test]
fn an_edit_during_the_update_is_kept() {
    let b = setup("change-during");
    let (change, _, provider) = b.changed(new_text());
    let raw = b.gmail.draft_raw(TOKEN, &provider).unwrap().raw.unwrap();
    let edited = String::from_utf8(raw).unwrap().replace("Here.", "Edited.");
    let fake = b.fake.clone();
    let id = provider.clone();
    b.fake.before_next_matching("PUT", move || {
        let f = fake.clone();
        fake.before_next_matching("GET /gmail/v1/users/me/drafts/", move || {
            assert!(f.edit_draft(&id, edited.as_bytes()));
        });
    });
    match b.apply(change) {
        Ok(Outcome::Failed { error, .. }) => assert_eq!(error, iris_apply::CHANGE_DURING),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.puts(), 1);
    assert!(b.row().1.contains("Edited."), "{}", b.row().1);
}

/// Recovery when the draft ID is gone and each read of the lookup by
/// Message-ID fails: the entry waits, and fails after 24 hours with no
/// second call (change spec, section 4).
#[test]
fn a_failed_lookup_ends_after_a_day() {
    let b = setup("change-lookup");
    let (change, _, provider) = b.changed(new_text());
    let fake = b.fake.clone();
    b.fake.before_next_matching("PUT", move || {
        fake.fail_next_write(&[503], false);
        fake.fail_next_matching("GET /gmail/v1/users/me/drafts/", 400);
    });
    assert!(matches!(b.apply(change), Ok(Outcome::Pending)));
    assert!(b.fake.delete_draft(&provider));
    let recover = |b: &Box| {
        b.fake
            .fail_next_matching("GET /gmail/v1/users/me/drafts?", 400);
        let (t, mut renew) = b.token();
        let mut token = Token {
            token: t,
            renew: &mut renew,
            refused: false,
        };
        iris_apply::recover_step(&b.store, &b.gmail, &mut token, b.account).unwrap();
    };
    recover(&b);
    assert_eq!(b.entry(change).0, "writing");
    b.age_entry(change, 24 * 60 * 60 * 1000 + 1);
    recover(&b);
    let (state, _, _, error) = b.entry(change);
    assert_eq!(state, "failed");
    assert_eq!(error.as_deref(), Some(iris_apply::CHANGE_UNKNOWN));
    assert_eq!(b.puts(), 1);
}
