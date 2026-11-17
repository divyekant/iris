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
