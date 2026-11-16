//! The apply of approved actions at the fake Gmail server (specification 2,
//! sections 7.2 and 10). No network, and no real mailbox. The tests write
//! the approved actions with raw SQL, as the changeset store does.

use std::path::PathBuf;
use std::time::Duration;

use iris_apply::{apply, hash, Error, Outcome};
use iris_fakegmail::FakeGmail;
use iris_gmail::{Config, Gmail};
use iris_store::Store;
use iris_synth::Mailbox;
use rusqlite::params;

const TOKEN: &str = "test-token";
const OWNER: &str = "owner@example.com";
const APPLY_POLICY: &str = "version = 0\nmode = \"apply\"\n";

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

/// A synced mirror of a synthetic mailbox of `n` messages. With `policy`,
/// the active policy has this text and the rule ID 1.
fn setup(name: &str, n: usize, policy: Option<&str>) -> Box {
    let fake = FakeGmail::start(Mailbox::new(7, n)).unwrap();
    let gmail = Gmail::new(Config {
        base_url: fake.base_url(),
        quota_per_minute: 10_000_000,
        backoff_start: Duration::from_millis(1),
        backoff_max: Duration::from_millis(4),
        ..Config::default()
    });
    let dir = std::env::temp_dir().join(format!("iris-apply-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();
    iris_sync::sync(&store, &gmail, account, TOKEN).unwrap();
    if let Some(text) = policy {
        store
            .write(|tx| {
                tx.execute(
                    "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES (?1, 'h', 0)",
                    [text],
                )?;
                tx.execute("INSERT INTO policy_rule_id(id) VALUES (1)", [])?;
                Ok(())
            })
            .unwrap();
    }
    Box {
        fake,
        gmail,
        store,
        account,
        dir,
    }
}

impl Box {
    /// Message row IDs whose mirror labels match the SQL condition on `l`.
    fn messages(&self, with: &str, limit: usize) -> Vec<i64> {
        self.store
            .read(|c| {
                let mut st = c.prepare(&format!(
                    "SELECT m.id FROM message m WHERE m.gone_at IS NULL AND {with} \
                     ORDER BY m.id LIMIT {limit}"
                ))?;
                let rows = st.query_map([], |r| r.get(0))?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap()
    }

    /// An action of `verb` on `messages`, submitted and approved by
    /// `decision`. Gives the action ID.
    fn approved(&self, verb: &str, messages: &[i64], decision: &str) -> i64 {
        self.store
            .write(|tx| {
                tx.execute(
                    "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0)",
                    [],
                )?;
                let cs = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                     VALUES (?1, 1, 1, ?2, '{\"messages\":[]}', '{}')",
                    params![cs, verb],
                )?;
                let a = tx.last_insert_rowid();
                for m in messages {
                    tx.execute(
                        "INSERT INTO action_message(action_id, message_id) VALUES (?1, ?2)",
                        params![a, m],
                    )?;
                }
                let h = hash(tx, a)?;
                tx.execute(
                    "UPDATE changeset_action SET hash = ?2 WHERE id = ?1",
                    params![a, h],
                )?;
                tx.execute(
                    "UPDATE changeset SET why = 'w', submitted_at = 1 WHERE id = ?1",
                    [cs],
                )?;
                tx.execute(
                    &format!(
                        "UPDATE changeset_action SET state = 'approved', {decision} WHERE id = ?1"
                    ),
                    [a],
                )?;
                Ok(a)
            })
            .unwrap()
    }

    fn apply(&self, action: i64) -> iris_apply::Result<Outcome> {
        apply(&self.store, &self.gmail, TOKEN, self.account, action)
    }

    fn state(&self, action: i64) -> (String, String) {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT state, decided_by FROM changeset_action WHERE id = ?1",
                    [action],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap()
    }

    fn provider_id(&self, message: i64) -> String {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT provider_id FROM message WHERE id = ?1",
                    [message],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    /// The labels of a message at Gmail, sorted.
    fn gmail_labels(&self, message: i64) -> Vec<String> {
        let mut l = self
            .gmail
            .minimal(TOKEN, &self.provider_id(message))
            .unwrap()
            .label_ids;
        l.sort();
        l
    }

    /// The labels of a message in the mirror, sorted.
    fn mirror_labels(&self, message: i64) -> Vec<String> {
        self.store
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT l.provider_id FROM message_label ml JOIN label l ON l.id = ml.label_id
                     WHERE ml.message_id = ?1 ORDER BY 1",
                )?;
                let rows = st.query_map([message], |r| r.get(0))?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap()
    }

    fn calls(&self, path: &str) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|r| r.starts_with("POST") && r.contains(path))
            .count()
    }
}

const PERSON: &str = "decided_by = 'person', decided_at = 2";
const IN_INBOX: &str = "EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
  WHERE ml.message_id = m.id AND l.provider_id = 'INBOX')";

fn applied(o: iris_apply::Result<Outcome>) -> iris_apply::Counts {
    match o {
        Ok(Outcome::Applied(c)) => c,
        other => panic!("{other:?}"),
    }
}

/// Each verb changes Gmail and the mirror the same way, and an action whose
/// messages have the target state changes nothing.
#[test]
fn each_verb_changes_gmail_and_the_mirror() {
    let b = setup("verbs", 80, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 5);
    assert_eq!(ms.len(), 5);
    let target = |verb: &str, labels: &[String]| {
        let has = |l: &str| labels.iter().any(|x| x == l);
        match verb {
            "archive" => !has("INBOX"),
            "inbox" => has("INBOX"),
            "read" => !has("UNREAD"),
            "unread" => has("UNREAD"),
            "trash" => has("TRASH"),
            _ => unreachable!(),
        }
    };
    for verb in ["archive", "inbox", "unread", "read", "trash"] {
        let need = ms
            .iter()
            .filter(|&&m| !target(verb, &b.gmail_labels(m)))
            .count() as u64;
        assert!(need > 0, "{verb}: the test changes nothing");
        let a = b.approved(verb, &ms, PERSON);
        let c = applied(b.apply(a));
        assert_eq!(
            (c.changed, c.unchanged, c.gone.len()),
            (need, 5 - need, 0),
            "{verb}"
        );
        assert_eq!(b.state(a), ("applied".into(), "person".into()));
        for &m in &ms {
            let at_gmail = b.gmail_labels(m);
            assert!(target(verb, &at_gmail), "{verb} {at_gmail:?}");
            assert_eq!(b.mirror_labels(m), at_gmail, "{verb}");
        }
        // The same action again: each message has the target state.
        let again = b.approved(verb, &ms, PERSON);
        let c = applied(b.apply(again));
        assert_eq!((c.changed, c.unchanged), (0, 5), "{verb} again");
    }
}

/// Step 2 of section 7.2: a message that changed at Gmail after the sync
/// keeps its state, and a gone message is listed.
#[test]
fn the_apply_reads_the_labels_at_gmail() {
    let b = setup("read-first", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 4);
    // Archived in Gmail after the last sync: the mirror still has INBOX.
    b.fake.modify_labels(&b.provider_id(ms[0]), &[], &["INBOX"]);
    b.fake.delete_message(&b.provider_id(ms[1]));
    // Gone in the mirror.
    b.store
        .write(|tx| Ok(tx.execute("UPDATE message SET gone_at = 1 WHERE id = ?1", [ms[2]])?))
        .unwrap();
    let a = b.approved("archive", &ms, PERSON);
    let c = applied(b.apply(a));
    assert_eq!((c.changed, c.unchanged), (1, 1));
    assert_eq!(c.gone, vec![ms[1], ms[2]]);
    assert_eq!(b.calls("batchModify"), 1);
    assert!(!b.gmail_labels(ms[3]).contains(&"INBOX".to_string()));
}

/// The mirror gets the labels that Gmail has, also for a label that
/// changed at Gmail after the last sync.
#[test]
fn the_mirror_gets_the_labels_of_gmail() {
    let b = setup("mirror", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 3);
    b.fake
        .modify_labels(&b.provider_id(ms[0]), &["Label_2"], &["INBOX"]);
    b.fake
        .modify_labels(&b.provider_id(ms[1]), &["Label_1"], &[]);
    assert_ne!(b.mirror_labels(ms[0]), b.gmail_labels(ms[0]));
    let a = b.approved("archive", &ms, PERSON);
    let c = applied(b.apply(a));
    assert_eq!((c.changed, c.unchanged), (2, 1));
    for &m in &ms {
        assert_eq!(b.mirror_labels(m), b.gmail_labels(m));
    }
}

/// A withdraw after the first call keeps the action approved: the next
/// apply ends it, also in the mode observe.
#[test]
fn a_withdraw_during_the_apply_keeps_the_action() {
    let b = setup("withdraw", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    let a = b.approved("trash", &ms, PERSON);
    b.fake.fail_next_write(&[0, 503], true);
    assert!(matches!(b.apply(a), Err(Error::Gmail(_))));
    b.store
        .write(|tx| {
            tx.execute("UPDATE changeset SET state = 'withdrawn'", [])?;
            tx.execute(
                "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('version = 0\nmode = \"observe\"\n', 'h3', 3)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(b.state(a).0, "approved");
    let c = applied(b.apply(a));
    assert_eq!((c.changed, c.unchanged), (0, 2));
    assert_eq!(b.state(a), ("applied".into(), "person".into()));
}

/// More than 1,000 messages: one batchModify call for each 1,000.
#[test]
fn a_large_action_streams_in_pages() {
    let b = setup("pages", 1100, Some(APPLY_POLICY));
    let ms = b.messages("1", 1050);
    assert_eq!(ms.len(), 1050);
    let need = ms
        .iter()
        .filter(|&&m| !b.mirror_labels(m).contains(&"UNREAD".to_string()))
        .count() as u64;
    let a = b.approved("unread", &ms, PERSON);
    let c = applied(b.apply(a));
    assert_eq!((c.changed, c.unchanged), (need, 1050 - need));
    assert_eq!(b.calls("batchModify"), 2);
    for &m in ms.iter().step_by(97) {
        assert_eq!(b.mirror_labels(m), b.gmail_labels(m));
    }
}

/// A clear refusal: the action is failed, and keeps its decision.
#[test]
fn a_refusal_fails_the_action() {
    let b = setup("refusal", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    let a = b.approved("archive", &ms, PERSON);
    b.fake.fail_next_write(&[400], false);
    match b.apply(a) {
        Ok(Outcome::Failed { counts, error }) => {
            assert_eq!(counts.changed, 0);
            assert!(error.contains("400"), "{error}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(b.state(a), ("failed".into(), "person".into()));
    // The second trash call fails: the first message keeps the change.
    let t = b.approved("trash", &ms, PERSON);
    b.fake.fail_next_write(&[0, 400], false);
    match b.apply(t) {
        Ok(Outcome::Failed { counts, .. }) => assert_eq!(counts.changed, 1),
        other => panic!("{other:?}"),
    }
    assert!(b.gmail_labels(ms[0]).contains(&"TRASH".to_string()));
    assert!(!b.gmail_labels(ms[1]).contains(&"TRASH".to_string()));
    for &m in &ms {
        assert_eq!(b.mirror_labels(m), b.gmail_labels(m));
    }
}

/// An unknown result (the call applied, the answer was lost) and HTTP 401
/// leave the action approved. The next apply makes no call again.
#[test]
fn an_unknown_result_stays_approved_until_the_next_apply() {
    let b = setup("unknown", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 3);
    let a = b.approved("archive", &ms, PERSON);
    b.fake.fail_next_write(&[503], true);
    assert!(matches!(b.apply(a), Err(Error::Gmail(e)) if e.write_result_unknown()));
    assert_eq!(b.state(a).0, "approved");
    let c = applied(b.apply(a));
    assert_eq!((c.changed, c.unchanged), (0, 3));
    assert_eq!(b.calls("batchModify"), 1);

    let t = b.approved("trash", &ms, PERSON);
    b.fake.fail_next_write(&[401], false);
    assert!(matches!(
        b.apply(t),
        Err(Error::Gmail(iris_gmail::Error::Unauthorized))
    ));
    assert_eq!(b.state(t).0, "approved");
    let c = applied(b.apply(t));
    assert_eq!(c.changed, 3);
    for &m in &ms {
        assert_eq!(b.mirror_labels(m), b.gmail_labels(m));
    }
}

/// The mode observe: nothing applies, and the action waits.
#[test]
fn observe_applies_nothing() {
    let b = setup("observe", 40, None);
    let ms = b.messages(IN_INBOX, 2);
    let a = b.approved("archive", &ms, PERSON);
    let before = b.fake.requests().len();
    assert_eq!(b.apply(a).unwrap(), Outcome::Waiting);
    assert_eq!(b.state(a).0, "approved");
    assert_eq!(b.fake.requests().len(), before);
}

/// Before the ledger, no rule approval applies: two actions of 300
/// messages that a rule approved would go past the bulk limit of 500
/// (section 5.4). Each goes to review, and a person approval applies.
#[test]
fn a_rule_approval_goes_to_review() {
    let b = setup("rule", 40, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    for policy in [1, 2] {
        if policy == 2 {
            b.store
                .write(|tx| {
                    Ok(tx.execute(
                        "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES (?1, 'h2', 3)",
                        [APPLY_POLICY],
                    )?)
                })
                .unwrap();
        }
        let a = b.approved(
            "archive",
            &ms,
            &format!("decided_by = 'rule', rule = 1, policy = {policy}, decided_at = 2"),
        );
        assert!(matches!(b.apply(a), Ok(Outcome::Review(_))));
        assert_eq!(b.state(a), ("in_review".into(), "engine".into()));
    }
    assert_eq!(b.calls("messages"), 0);
    let ok = b.approved("archive", &ms, PERSON);
    applied(b.apply(ok));
}

/// The hash check, another account, a verb with no apply, and a state that
/// is not approved.
#[test]
fn the_apply_refuses_what_was_not_approved() {
    let b = setup("refuse", 40, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    let a = b.approved("archive", &ms, PERSON);
    // Another writer changes the hash past the trigger.
    b.store
        .write(|tx| {
            let trigger: String = tx.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'action_fixed'",
                [],
                |r| r.get(0),
            )?;
            tx.execute_batch("DROP TRIGGER action_fixed")?;
            tx.execute("UPDATE changeset_action SET hash = 'x' WHERE id = ?1", [a])?;
            tx.execute_batch(&trigger)?;
            Ok(())
        })
        .unwrap();
    assert!(matches!(b.apply(a), Ok(Outcome::Denied(_))));
    assert_eq!(b.state(a), ("denied".into(), "engine".into()));
    assert!(matches!(b.apply(a), Err(Error::NotApproved(_))));

    let other = b.approved("archive", &ms, PERSON);
    assert!(matches!(
        apply(&b.store, &b.gmail, TOKEN, b.account + 1, other),
        Ok(Outcome::Denied(_))
    ));

    let label = b.approved("label", &ms, PERSON);
    assert!(matches!(b.apply(label), Err(Error::Verb(_))));
    assert_eq!(b.state(label).0, "approved");
    assert_eq!(b.calls("messages"), 0);
}
