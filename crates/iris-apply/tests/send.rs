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
        let who = Author {
            kind: AuthorKind::Person,
            name: None,
            session: None,
            job: None,
        };
        let cs = create(&self.store, &who, now()).unwrap();
        let args: Map<String, Value> = args.as_object().unwrap().clone();
        add_action(&self.store, cs, &who, verb, &[target], &args, None).unwrap();
        submit(&self.store, cs, &who, "Write.", now()).unwrap();
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

    fn release(&self, action: i64) -> iris_apply::Result<Outcome> {
        let (t, mut renew) = self.token();
        let mut token = Token {
            token: t,
            renew: &mut renew,
            refused: false,
        };
        release(&self.store, &self.gmail, &mut token, self.account, action)
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
    assert_eq!((b.state(s).as_str(), b.sends()), ("approved", 0));
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
    assert_eq!((b.state(s).as_str(), b.sends()), ("approved", 1));
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
/// only, denies the send, and no `drafts/send` request goes out.
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
    let foreign: bool = b
        .store
        .read(|c| Ok(c.query_row("SELECT foreign_at IS NOT NULL FROM draft", [], |r| r.get(0))?))
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
    let (s, _) = b.new_mail(AGED);
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

/// A create with an unknown result: the recovery finds the draft by its
/// Message-ID, and makes no second create.
#[test]
fn an_unknown_create_is_found() {
    let b = setup("create");
    let a = b.approved(Verb::Draft, "a-1", mail(), 0);
    b.fake.fail_next_write(&[503], true);
    match b.apply(a) {
        Ok(Outcome::Applied(_)) => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(b.posts("/drafts"), 1);
    let (_, provider) = b.draft(a);
    assert!(b.gmail.draft_raw(TOKEN, &provider).is_ok());

    // A lost create that Gmail did not make fails, with no second call.
    let a = b.approved(Verb::Draft, "a-1", mail(), 0);
    b.fake.fail_next_write(&[503], false);
    assert!(matches!(b.apply(a), Ok(Outcome::Failed { .. })));
    assert_eq!(b.posts("/drafts"), 2);
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
