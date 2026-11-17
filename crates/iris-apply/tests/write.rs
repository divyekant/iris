//! The write view (docs/specs/2026-11-17-write.md): one press of Send,
//! then the draft, the send and the release through the apply step, at the
//! fake Gmail server. No network, and no real mailbox.

use std::path::PathBuf;
use std::time::Duration;

use iris_apply::{Outcome, Token, HOLD_MS};
use iris_changeset::{
    last_mark, send_written, write_mail, written_since, Mail, Press, Refusal, Written, MARK_MS,
};
use iris_fakegmail::FakeGmail;
use iris_gmail::{Config, Gmail};
use iris_store::Store;
use iris_synth::Mailbox;
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
    let dir = std::env::temp_dir().join(format!("iris-write-{name}-{}", std::process::id()));
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

    fn token(&self) -> (String, fn() -> Option<String>) {
        (TOKEN.into(), || Some(TOKEN.into()))
    }

    /// The apply step of the engine at the time now.
    fn step(&self) -> iris_apply::Step {
        self.step_at(now())
    }

    /// The apply step of the engine at the time `t`.
    fn step_at(&self, at: i64) -> iris_apply::Step {
        let (t, mut renew) = self.token();
        let mut token = Token {
            token: t,
            renew: &mut renew,
            refused: false,
        };
        iris_apply::step(&self.store, &self.gmail, &mut token, self.account, at).unwrap()
    }

    /// The apply step with a clock that gives `start` at its first read
    /// and `later` after it, as if the Gmail calls of step 2 took that long.
    fn slow_step(&self, start: i64, later: i64) -> iris_apply::Step {
        let (t, mut renew) = self.token();
        let mut token = Token {
            token: t,
            renew: &mut renew,
            refused: false,
        };
        let reads = std::cell::Cell::new(0);
        let clock = || {
            reads.set(reads.get() + 1);
            if reads.get() == 1 {
                start
            } else {
                later
            }
        };
        iris_apply::step_clock(&self.store, &self.gmail, &mut token, self.account, &clock).unwrap()
    }

    fn sends(&self) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|r| r.starts_with("POST") && r.ends_with("/drafts/send"))
            .count()
    }

    fn sql(&self, sql: &str, p: impl rusqlite::Params) {
        self.store.write(|tx| Ok(tx.execute(sql, p)?)).unwrap();
    }

    fn one<T: rusqlite::types::FromSql>(&self, sql: &str, p: impl rusqlite::Params) -> T {
        self.store
            .read(|c| Ok(c.query_row(sql, p, |r| r.get(0))?))
            .unwrap()
    }

    /// The write of a new mail. Gives the changeset row ID.
    fn write(&self) -> i64 {
        match press(
            &self.store,
            Mail::New { account: "a-1" },
            &args(mail()),
            KEY,
            &["new_recipient"],
        ) {
            Ok(Written::New(cs)) => cs,
            other => panic!("{other:?}"),
        }
    }

    /// Give the mirror the Gmail subject of `message`, as the understanding
    /// step does, so submit flags the same subject as the apply.
    fn understood(&self, message: i64) {
        let provider: String = self.one("SELECT provider_id FROM message WHERE id = ?1", [message]);
        let head = self.gmail.metadata(TOKEN, &provider, &["Subject"]).unwrap();
        let subject = head
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("Subject"))
            .unwrap()
            .value
            .clone();
        self.sql(
            "UPDATE header SET subject = ?1 WHERE message_id = ?2",
            rusqlite::params![subject, message],
        );
    }

    /// The apply of the `draft` action of the mark, with no step 2a.
    fn apply_mark(&self) {
        let action: i64 = self.one("SELECT action_id FROM write_send", []);
        let (t, mut renew) = self.token();
        let mut token = Token {
            token: t,
            renew: &mut renew,
            refused: false,
        };
        let r = iris_apply::apply(&self.store, &self.gmail, &mut token, self.account, action);
        assert!(matches!(r, Ok(Outcome::Applied(_))), "{r:?}");
    }

    /// The state of the `draft` action and of the send of the write `cs`.
    fn states(&self, cs: i64) -> (String, Option<String>) {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT a.state, s.state FROM changeset_action a
                     JOIN write_send w ON w.action_id = a.id
                     LEFT JOIN changeset_action s ON s.id = w.send_action_id
                     WHERE a.changeset_id = ?1",
                    [cs],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap()
    }
}

fn mail() -> Value {
    json!({"to": ["priya@example.com"], "cc": [], "subject": "Q4 numbers", "body": "Hi Priya,\n\nHere.\n"})
}

const KEY: &str = "key-0000000000000001";

fn press(
    store: &Store,
    mail: Mail<'_>,
    args: &Map<String, Value>,
    key: &str,
    flags: &[&str],
) -> Result<Written, Refusal> {
    let flags: Vec<String> = flags.iter().map(|f| f.to_string()).collect();
    write_mail(
        store,
        &Press {
            mail,
            args,
            key,
            flags: &flags,
        },
        now(),
    )
    .unwrap()
}

fn args(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Acceptance 1: one press of Send makes the draft, the send in the held
/// outbox, and, after the hold, one `drafts.send`.
#[test]
fn a_written_mail_leaves_after_the_hold() {
    let b = setup("leaves");
    let cs = b.write();
    assert_eq!(b.states(cs), ("approved".into(), None));
    let out = b.step();
    assert!(out.next.is_some(), "the step names the end of the hold");
    assert_eq!(b.states(cs), ("applied".into(), Some("held".into())));
    assert_eq!(b.sends(), 0, "the hold is not over");
    // The send is of the person, approved by the person.
    let (by, author): (String, String) = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT s.decided_by, c.author_kind FROM write_send w
                 JOIN changeset_action s ON s.id = w.send_action_id
                 JOIN changeset c ON c.id = s.changeset_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!((by.as_str(), author.as_str()), ("person", "person"));
}

/// After the hold, the release sends the mail one time. Step 2a runs at a
/// time one hold ago, so the hold of the send is over.
#[test]
fn a_written_mail_is_sent_one_time() {
    let b = setup("sent-once");
    let cs = b.write();
    b.apply_mark();
    send_written(&b.store, b.account, &|| now() - HOLD_MS - 1_000).unwrap();
    b.step();
    assert_eq!(b.states(cs), ("applied".into(), Some("applied".into())));
    b.step();
    assert_eq!(b.sends(), 1);
}

/// Acceptance 2: a reply names the parent that the person saw. A newer
/// mail in the thread refuses the write, and nothing stays.
#[test]
fn a_reply_to_an_old_parent_is_refused() {
    let b = setup("parent");
    let (thread, parent) = b.thread();
    b.understood(parent);
    let t = format!("t-{thread}");
    let reply = args(json!({"to": ["priya@example.com"], "cc": [], "body": "Yes.\n"}));
    let before: i64 = b.one("SELECT count(*) FROM changeset", []);
    let r = press(
        &b.store,
        Mail::Reply {
            thread: &t,
            parent: parent + 1_000_000,
        },
        &reply,
        KEY,
        &[],
    );
    assert!(matches!(r, Err(Refusal::NewParent)), "{r:?}");
    assert_eq!(b.one::<i64>("SELECT count(*) FROM changeset", []), before);
    assert_eq!(b.one::<i64>("SELECT count(*) FROM write_send", []), 0);
    let r = press(
        &b.store,
        Mail::Reply { thread: &t, parent },
        &reply,
        KEY,
        &[],
    );
    // A reply to an earlier writer can be a first contact: the person
    // confirms the warnings that the engine names.
    let flags: Vec<String> = match r {
        Err(Refusal::Flags(f)) => f,
        Ok(_) => vec![],
        other => panic!("{other:?}"),
    };
    let seen: Vec<&str> = flags.iter().map(String::as_str).collect();
    let cs = match press(
        &b.store,
        Mail::Reply { thread: &t, parent },
        &reply,
        KEY,
        &seen,
    ) {
        Ok(Written::New(cs)) => cs,
        other => panic!("{other:?}"),
    };
    b.step();
    let why: Option<String> = b.one(
        "SELECT reason FROM changeset_action WHERE changeset_id = ?1",
        [cs],
    );
    assert_eq!(
        b.states(cs),
        ("applied".into(), Some("held".into())),
        "{why:?}"
    );
}

/// A mail that the submit denies leaves nothing, and gives the reason.
#[test]
fn a_denied_write_leaves_nothing() {
    let b = setup("denied");
    let bad = args(json!({"to": ["not an address"], "cc": [], "subject": "S", "body": "B"}));
    let r = press(&b.store, Mail::New { account: "a-1" }, &bad, KEY, &[]);
    assert!(
        matches!(&r, Err(Refusal::Denied(why)) if !why.is_empty()),
        "{r:?}"
    );
    assert_eq!(b.one::<i64>("SELECT count(*) FROM changeset", []), 0);
}

/// Acceptance 3: a withdraw before the draft exists stops the write: no
/// draft and no send.
#[test]
fn a_withdraw_before_the_draft_stops_the_write() {
    let b = setup("withdraw");
    let cs = b.write();
    iris_changeset::withdraw(&b.store, cs, None).unwrap();
    b.step();
    assert_eq!(b.states(cs), ("cancelled".into(), None));
    assert_eq!(b.one::<i64>("SELECT count(*) FROM draft", []), 0);
}

/// A withdraw after the draft exists, before step 2a: the draft stays,
/// and no send is made.
#[test]
fn a_withdraw_after_the_draft_makes_no_send() {
    let b = setup("withdraw-late");
    let cs = b.write();
    // The changeset closes when its action applies, so make the draft by
    // the apply alone, then withdraw as the person would.
    let action: i64 = b.one("SELECT action_id FROM write_send", []);
    let (t, mut renew) = b.token();
    let mut token = Token {
        token: t,
        renew: &mut renew,
        refused: false,
    };
    assert!(matches!(
        iris_apply::apply(&b.store, &b.gmail, &mut token, b.account, action),
        Ok(Outcome::Applied(_))
    ));
    b.sql(
        "UPDATE changeset SET state = 'withdrawn' WHERE id = ?1",
        [cs],
    );
    assert_eq!(send_written(&b.store, b.account, &|| now()).unwrap(), 0);
    assert_eq!(b.states(cs).1, None);
}

/// Acceptance 4: two runs of step 2a make one send.
#[test]
fn each_mark_gets_one_send() {
    let b = setup("once");
    let cs = b.write();
    let action: i64 = b.one("SELECT action_id FROM write_send", []);
    let (t, mut renew) = b.token();
    let mut token = Token {
        token: t,
        renew: &mut renew,
        refused: false,
    };
    iris_apply::apply(&b.store, &b.gmail, &mut token, b.account, action).unwrap();
    assert_eq!(send_written(&b.store, b.account, &|| now()).unwrap(), 1);
    assert_eq!(send_written(&b.store, b.account, &|| now()).unwrap(), 0);
    assert_eq!(
        b.one::<i64>(
            "SELECT count(*) FROM changeset_action WHERE verb = 'send'",
            []
        ),
        1
    );
    assert_eq!(b.states(cs).1.as_deref(), Some("approved"));
}

/// A draft that the person sent at Gmail before step 2a gets no send.
#[test]
fn a_draft_sent_at_gmail_gets_no_send() {
    let b = setup("sent");
    b.write();
    let action: i64 = b.one("SELECT action_id FROM write_send", []);
    let (t, mut renew) = b.token();
    let mut token = Token {
        token: t,
        renew: &mut renew,
        refused: false,
    };
    iris_apply::apply(&b.store, &b.gmail, &mut token, b.account, action).unwrap();
    b.sql("UPDATE draft SET sent_at = 1, sent_message = 'x'", []);
    assert_eq!(send_written(&b.store, b.account, &|| now()).unwrap(), 0);
}

/// The store refuses a mark for an action that the person did not approve,
/// and a second change of a mark.
#[test]
fn the_store_guards_the_mark() {
    let b = setup("guard");
    b.write();
    let action: i64 = b.one("SELECT action_id FROM write_send", []);
    let r = b.store.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO write_send(action_id, created_at) VALUES (?1 + 1000, 1)",
            [action],
        )?)
    });
    assert!(r.is_err());
    let (t, mut renew) = b.token();
    let mut token = Token {
        token: t,
        renew: &mut renew,
        refused: false,
    };
    iris_apply::apply(&b.store, &b.gmail, &mut token, b.account, action).unwrap();
    send_written(&b.store, b.account, &|| now()).unwrap();
    let r = b
        .store
        .write(|tx| Ok(tx.execute("UPDATE write_send SET send_action_id = NULL", [])?));
    assert!(r.is_err());
    let r = b
        .store
        .write(|tx| Ok(tx.execute("DELETE FROM write_send", [])?));
    assert!(r.is_err());
}

/// The sync thread wakes for each mark that commits after its read of the
/// last mark, also in the same millisecond: the cursor is the row ID, not
/// the clock. Two presses one after the other each move the cursor.
#[test]
fn a_new_mark_wakes_the_step() {
    let b = setup("wake");
    let seen = last_mark(&b.store).unwrap();
    assert_eq!(seen, 0);
    assert!(!written_since(&b.store, seen).unwrap());
    b.write();
    // The same millisecond as the press: a clock cursor misses this mark.
    assert!(written_since(&b.store, seen).unwrap());
    let first = last_mark(&b.store).unwrap();
    assert!(!written_since(&b.store, first).unwrap());
    let other =
        args(json!({"to": ["sam@example.com"], "cc": [], "subject": "Two", "body": "Hi.\n"}));
    let r = press(
        &b.store,
        Mail::New { account: "a-1" },
        &other,
        "key-0000000000000002",
        &["new_recipient"],
    );
    assert!(matches!(r, Ok(Written::New(_))), "{r:?}");
    assert!(written_since(&b.store, first).unwrap());
}

/// A second try of one press (a lost answer) finds the first write and
/// makes no second mail. The same key with another mail is refused.
#[test]
fn a_second_try_of_a_press_makes_no_second_mail() {
    let b = setup("key");
    let cs = b.write();
    let again = press(
        &b.store,
        Mail::New { account: "a-1" },
        &args(mail()),
        KEY,
        &[],
    );
    assert_eq!(again, Ok(Written::Again(cs)));
    let mut other = mail();
    other["body"] = json!("Another text.\n");
    let r = press(
        &b.store,
        Mail::New { account: "a-1" },
        &args(other),
        KEY,
        &[],
    );
    assert_eq!(r, Err(Refusal::KeyTaken));
    assert_eq!(b.one::<i64>("SELECT count(*) FROM write_send", []), 1);
}

/// A mail with a warning that the person did not see is refused with the
/// warnings, and nothing stays. With the warnings seen, it is written.
#[test]
fn the_person_sees_each_warning_before_the_send() {
    let b = setup("flags");
    let mut money = mail();
    money["body"] = json!("The price is $500.\n");
    let r = press(
        &b.store,
        Mail::New { account: "a-1" },
        &args(money.clone()),
        KEY,
        &[],
    );
    let Err(Refusal::Flags(flags)) = r else {
        panic!("{r:?}")
    };
    assert!(flags.iter().any(|f| f == "money"), "{flags:?}");
    assert_eq!(b.one::<i64>("SELECT count(*) FROM changeset", []), 0);
    let seen: Vec<&str> = flags.iter().map(String::as_str).collect();
    let r = press(
        &b.store,
        Mail::New { account: "a-1" },
        &args(money),
        KEY,
        &seen,
    );
    let Ok(Written::New(cs)) = r else {
        panic!("{r:?}")
    };
    b.step();
    assert_eq!(b.states(cs), ("applied".into(), Some("held".into())));
}

/// A send whose warnings are not the warnings of the press goes to review
/// and does not leave.
#[test]
fn a_send_with_a_new_warning_goes_to_review() {
    let b = setup("new-flag");
    let cs = b.write();
    // The draft has the warning `new_recipient`. Make the mark say that
    // the person saw no warning (the store guards the mark, so the test
    // drops its guard first).
    b.apply_mark();
    b.sql("DROP TRIGGER write_send_update", []);
    b.sql("UPDATE write_send SET flags = '[]'", []);
    send_written(&b.store, b.account, &|| now()).unwrap();
    assert_eq!(b.states(cs).1.as_deref(), Some("in_review"));
    b.step_at(now() + HOLD_MS + 1_000);
    assert_eq!(b.sends(), 0);
}

/// Acceptance 8: the approval of a press lives 24 hours. A mark that got
/// no send in that time (Iris stopped after the press) never gets one.
#[test]
fn an_old_mark_gets_no_send() {
    let b = setup("old-mark");
    let cs = b.write();
    b.apply_mark();
    let pressed: i64 = b.one("SELECT created_at FROM write_send", []);
    let end = pressed + MARK_MS;
    assert_eq!(send_written(&b.store, b.account, &|| end).unwrap(), 0);
    assert_eq!(b.states(cs), ("applied".into(), None));
    assert_eq!(send_written(&b.store, b.account, &|| end - 1).unwrap(), 1);
}

/// A slow step: the hold of the send starts when step 2a makes the send,
/// not at the start of the step, so the person keeps the full 60 seconds.
#[test]
fn a_slow_step_keeps_the_full_hold() {
    let b = setup("slow-hold");
    b.write();
    let start = now();
    let later = start + 2 * HOLD_MS;
    b.slow_step(start, later);
    let decided: i64 = b.one(
        "SELECT s.decided_at FROM write_send w JOIN changeset_action s ON s.id = w.send_action_id",
        [],
    );
    assert_eq!(decided, later);
}

/// A slow step that crosses the 24 hours of the mark makes no send: the age
/// check reads the clock when it makes the send.
#[test]
fn a_slow_step_past_the_mark_life_makes_no_send() {
    let b = setup("slow-old");
    let cs = b.write();
    let pressed: i64 = b.one("SELECT created_at FROM write_send", []);
    b.slow_step(pressed + MARK_MS - 1, pressed + MARK_MS);
    assert_eq!(b.states(cs), ("applied".into(), None));
}
