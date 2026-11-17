//! The apply of approved actions at the fake Gmail server (specification 2,
//! sections 7.2 and 10). No network, and no real mailbox. The tests write
//! the approved actions with raw SQL, as the changeset store does.

use std::path::PathBuf;
use std::time::Duration;

use iris_apply::{apply, Error, Outcome, Token};
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
    /// The job of the author of each next changeset (a job session).
    job: std::sync::Mutex<Option<i64>>,
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
        job: Default::default(),
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
        self.approved_at(verb, messages, decision, now())
    }

    /// As `approved`, with the submit time `submitted` (milliseconds).
    fn approved_at(&self, verb: &str, messages: &[i64], decision: &str, submitted: i64) -> i64 {
        self.approved_args(verb, "{}", messages, decision, submitted)
    }

    /// A `label` or `unlabel` action of the label `name`, approved by a
    /// person.
    fn labelled(&self, verb: &str, name: &str, messages: &[i64]) -> i64 {
        let args = serde_json::json!({ "label": name }).to_string();
        self.approved_args(verb, &args, messages, PERSON, now())
    }

    fn approved_args(
        &self,
        verb: &str,
        args: &str,
        messages: &[i64],
        decision: &str,
        submitted: i64,
    ) -> i64 {
        let ids: Vec<String> = messages.iter().map(|m| format!("\"m-{m}\"")).collect();
        let targets = format!("{{\"messages\":[{}]}}", ids.join(","));
        self.store
            .write(|tx| {
                let job = self.job.lock().unwrap().map(iris_job::format_job_id);
                tx.execute(
                    "INSERT INTO changeset(author_kind, author_session, author_job, created_at)
                     VALUES ('agent', CASE WHEN ?1 IS NULL THEN NULL ELSE 'se-1' END, ?1, 0)",
                    [job],
                )?;
                let cs = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                     VALUES (?1, 1, 1, ?2, ?3, ?4)",
                    params![cs, verb, targets, args],
                )?;
                let a = tx.last_insert_rowid();
                for m in messages {
                    tx.execute(
                        "INSERT INTO action_message(action_id, message_id) VALUES (?1, ?2)",
                        params![a, m],
                    )?;
                }
                let (_, h) = iris_changeset::hash(tx, a).unwrap();
                tx.execute(
                    "UPDATE changeset_action SET hash = ?2 WHERE id = ?1",
                    params![a, h],
                )?;
                tx.execute(
                    "UPDATE changeset SET why = 'w', submitted_at = ?2 WHERE id = ?1",
                    params![cs, submitted],
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
        self.apply_as(self.account, action, &mut || Some(TOKEN.into()))
    }

    fn apply_as(
        &self,
        account: i64,
        action: i64,
        renew: &mut dyn FnMut() -> Option<String>,
    ) -> iris_apply::Result<Outcome> {
        let mut token = Token {
            token: TOKEN.into(),
            renew,
            refused: false,
        };
        apply(&self.store, &self.gmail, &mut token, account, action)
    }

    /// True if the action has a ledger row in `writing`.
    fn writing(&self, action: i64) -> bool {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT EXISTS (SELECT 1 FROM ledger WHERE action_id = ?1 AND state = 'writing')",
                    [action],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    /// The ledger row of an `archive` apply that stopped: each message in
    /// call 1, and the row at `call`.
    fn stopped(&self, action: i64, messages: &[i64], call: i64) {
        self.store
            .write(|tx| {
                tx.execute(
                    "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
                     VALUES (?1, ?2, '{\"call\":\"batchModify\",\"add\":[],\"remove\":[\"INBOX\"],\"calls\":1}', 'writing', 'inverse', 1)",
                    params![action, self.account],
                )?;
                let l = tx.last_insert_rowid();
                for m in messages {
                    tx.execute(
                        "INSERT INTO ledger_message(ledger_id, message_id, call, pre, post)
                         VALUES (?1, ?2, 1, '[\"INBOX\"]', '[]')",
                        params![l, m],
                    )?;
                }
                tx.execute(
                    "UPDATE ledger SET call = ?2 WHERE id = ?1",
                    params![l, call],
                )?;
                Ok(())
            })
            .unwrap();
    }

    /// The ledger row of an action: its state, its call, and for each
    /// message its `pre`, `post`, `done` and `listed`.
    #[allow(clippy::type_complexity)]
    fn ledger(
        &self,
        action: i64,
    ) -> (
        String,
        i64,
        Vec<(i64, String, String, bool, Option<String>)>,
    ) {
        self.store
            .read(|c| {
                let (id, state, call): (i64, String, i64) = c.query_row(
                    "SELECT id, state, call FROM ledger WHERE action_id = ?1",
                    [action],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
                let mut st = c.prepare(
                    "SELECT message_id, pre, post, done, listed FROM ledger_message
                     WHERE ledger_id = ?1 ORDER BY 1",
                )?;
                let rows = st.query_map([id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })?;
                Ok((state, call, rows.collect::<rusqlite::Result<_>>()?))
            })
            .unwrap()
    }

    /// The document of the changeset of `action` passes the published
    /// schema.
    fn check_document(&self, action: i64) {
        let cs: i64 = self
            .store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT changeset_id FROM changeset_action WHERE id = ?1",
                    [action],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        let mut out = Vec::new();
        iris_changeset::write_document(&self.store, cs, &mut out).unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let errors: Vec<String> = iris_spec::validator("changeset")
            .iter_errors(&doc)
            .map(|e| format!("{e} at {}", e.instance_path()))
            .collect();
        assert!(errors.is_empty(), "{errors:#?}\n{doc}");
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

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// The provider ID of the user label `name` in the mirror.
fn label_id(b: &Box, name: &str) -> Option<String> {
    b.store
        .read(|c| {
            Ok(c.query_row(
                "SELECT provider_id FROM label WHERE account_id = ?1 AND name = ?2 AND kind = 'user'",
                params![b.account, name],
                |r| r.get(0),
            )
            .ok())
        })
        .unwrap()
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
        b.check_document(a);
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

/// A withdraw keeps an action with a ledger row in `writing`. The next
/// apply runs the recovery of the row, also in the mode observe: a message
/// whose call started and that has the `post` state is `done`, each other
/// one is `unknown`, and no write call is made again.
#[test]
fn a_withdraw_during_the_apply_keeps_the_action() {
    let b = setup("withdraw", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 3);
    let a = b.approved("archive", &ms, PERSON);
    // The engine stopped after the call started. Gmail changed message 0
    // only: the person moved the others back, or the call never arrived.
    b.stopped(a, &ms, 1);
    b.fake.modify_labels(&b.provider_id(ms[0]), &[], &["INBOX"]);
    b.store
        .write(|tx| {
            tx.execute("UPDATE changeset SET state = 'withdrawn'", [])?;
            tx.execute(
                "INSERT INTO policy_version(text, sha256, confirmed_at)
                 VALUES ('version = 0\nmode = \"observe\"\n', 'h3', 3)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(b.state(a).0, "approved");
    match b.apply(a) {
        Ok(Outcome::Failed { counts, .. }) => {
            assert_eq!(counts.changed, 1);
            assert_eq!(counts.unknown, vec![ms[1], ms[2]]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(b.state(a), ("failed".into(), "person".into()));
    assert_eq!(b.calls("batchModify"), 0);
    let (state, _, rows) = b.ledger(a);
    assert_eq!(state, "failed");
    let done: Vec<bool> = rows.iter().map(|r| r.3).collect();
    assert_eq!(done, vec![true, false, false]);
    for &m in &ms {
        assert_eq!(b.mirror_labels(m), b.gmail_labels(m));
    }
    b.check_document(a);
}

/// A row whose calls never started: the recovery reads each `pre` again,
/// skips a message that has the target state now, and makes the calls.
#[test]
fn the_recovery_makes_the_calls_that_never_started() {
    let b = setup("not-started", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 3);
    let a = b.approved("archive", &ms, PERSON);
    b.stopped(a, &ms, 0);
    b.fake.modify_labels(&b.provider_id(ms[1]), &[], &["INBOX"]);
    let c = applied(b.apply(a));
    assert_eq!((c.changed, c.unchanged), (2, 1));
    assert_eq!(b.calls("batchModify"), 1);
    let (state, call, rows) = b.ledger(a);
    assert_eq!((state.as_str(), call), ("applied", 1));
    let listed: Vec<Option<String>> = rows.iter().map(|r| r.4.clone()).collect();
    assert_eq!(listed, vec![None, Some("skipped".into()), None]);
    for &m in &ms {
        assert!(!b.gmail_labels(m).contains(&"INBOX".to_string()));
        assert_eq!(b.mirror_labels(m), b.gmail_labels(m));
    }
}

/// A row in `writing` of another action blocks no message: the apply runs
/// its recovery first, then claims its own messages.
#[test]
fn the_apply_recovers_the_rows_of_the_account_first() {
    let b = setup("other-row", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    let first = b.approved("archive", &ms, PERSON);
    b.stopped(first, &ms, 1);
    let second = b.approved("unread", &ms, PERSON);
    applied(b.apply(second));
    assert_eq!(b.state(first).0, "failed");
    assert!(!b.writing(first));
    assert_eq!(b.calls("batchModify"), 1);
    // The public recovery of the start finds nothing more.
    let mut renew = || None;
    let mut token = Token {
        token: TOKEN.into(),
        renew: &mut renew,
        refused: false,
    };
    iris_apply::recover(&b.store, &b.gmail, &mut token, b.account).unwrap();
}

/// Each applied action has its ledger row: `ops`, the calls, and for each
/// changed message its `pre`, `post` and `done`.
#[test]
fn the_ledger_lists_each_changed_message() {
    let b = setup("rows-ledger", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 3);
    b.fake.modify_labels(&b.provider_id(ms[2]), &[], &["INBOX"]);
    let a = b.approved("trash", &ms, PERSON);
    applied(b.apply(a));
    let (state, call, rows) = b.ledger(a);
    assert_eq!((state.as_str(), call), ("applied", 3));
    assert_eq!(
        rows,
        vec![
            (
                ms[0],
                "[\"INBOX\"]".into(),
                "[\"TRASH\"]".into(),
                true,
                None
            ),
            (
                ms[1],
                "[\"INBOX\"]".into(),
                "[\"TRASH\"]".into(),
                true,
                None
            ),
            (ms[2], "[]".into(), "[\"TRASH\"]".into(), true, None),
        ]
    );
    let (ops, history): (String, Option<String>) = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT ops, history_id FROM ledger WHERE action_id = ?1",
                [a],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&ops).unwrap(),
        serde_json::json!({"call": "trash", "add": ["TRASH"], "remove": [], "calls": 3})
    );
    assert!(history.is_some());
    // An action whose messages have the target state has a row with no
    // message and no call.
    let again = b.approved("trash", &ms, PERSON);
    applied(b.apply(again));
    assert_eq!(b.ledger(again), ("applied".into(), 0, vec![]));
    b.check_document(again);
}

/// A mirror write changes only the labels that differ: a message with the
/// target state keeps its model wait.
#[test]
fn an_unchanged_message_keeps_its_rows() {
    let b = setup("rows", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    b.fake.modify_labels(&b.provider_id(ms[0]), &[], &["INBOX"]);
    b.store
        .write(|tx| {
            tx.execute(
                "DELETE FROM message_label WHERE message_id = ?1 AND label_id =
                   (SELECT id FROM label WHERE provider_id = 'INBOX')",
                [ms[0]],
            )?;
            tx.execute("UPDATE message SET waits_model = 1", [])?;
            Ok(())
        })
        .unwrap();
    let a = b.approved("archive", &ms, PERSON);
    applied(b.apply(a));
    let waits = |m: i64| -> i64 {
        b.store
            .read(|c| {
                Ok(
                    c.query_row("SELECT waits_model FROM message WHERE id = ?1", [m], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap()
    };
    assert_eq!((waits(ms[0]), waits(ms[1])), (1, 0));
}

/// 14 days after the submit, the action expires and applies nothing.
#[test]
fn an_old_action_expires() {
    let b = setup("expire", 40, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    let a = b.approved_at("archive", &ms, PERSON, 1);
    assert_eq!(b.apply(a).unwrap(), Outcome::Expired);
    assert_eq!(b.state(a), ("expired".into(), "expiry".into()));
    assert_eq!(b.calls("messages"), 0);
}

/// Two applies at the same time: the second gets Busy and changes
/// nothing, and the first ends as applied.
#[test]
fn two_applies_make_one_write() {
    let b = setup("two", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 3);
    let a = b.approved("archive", &ms, PERSON);
    // Another apply holds the lock.
    let other = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(b.dir.join(iris_apply::LOCK_FILE))
        .unwrap();
    other.lock().unwrap();
    assert!(matches!(b.apply(a), Err(Error::Busy)));
    assert_eq!(b.state(a).0, "approved");
    assert!(!b.writing(a));
    drop(other);
    // A sync of the account runs.
    let sync = iris_sync::lock(&b.store, b.account).unwrap();
    assert!(matches!(b.apply(a), Err(Error::Busy)));
    assert!(!b.writing(a));
    drop(sync);
    for round in 0..5 {
        let verb = if round % 2 == 0 { "archive" } else { "inbox" };
        let a = b.approved(verb, &ms, PERSON);
        let before = b.calls("batchModify");
        let results: Vec<_> = std::thread::scope(|sc| {
            let t: Vec<_> = (0..2).map(|_| sc.spawn(|| b.apply(a))).collect();
            t.into_iter().map(|t| t.join().unwrap()).collect()
        });
        assert!(
            results.iter().any(|r| matches!(r, Ok(Outcome::Applied(_)))),
            "{results:?}"
        );
        assert!(
            results.iter().all(|r| matches!(
                r,
                Ok(Outcome::Applied(_)) | Err(Error::Busy) | Err(Error::NotApproved(_))
            )),
            "{results:?}"
        );
        assert_eq!(b.state(a).0, "applied");
        assert_eq!(b.calls("batchModify") - before, 1);
    }
}

/// The mode changes to observe during the reads of step 2: the action
/// waits, with no write call. The expiry is in the same check.
#[test]
fn the_mark_checks_the_mode_again() {
    let b = setup("late-mode", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 3);
    let a = b.approved("archive", &ms, PERSON);
    // The profile call passes; the first label read gets 401, and the new
    // token comes after the person changed the mode.
    b.fake.fail_next(&[0, 401]);
    let r = b.apply_as(b.account, a, &mut || {
        b.store
            .write(|tx| {
                Ok(tx.execute(
                    "INSERT INTO policy_version(text, sha256, confirmed_at)
                     VALUES ('version = 0\nmode = \"observe\"\n', 'h4', 4)",
                    [],
                )?)
            })
            .unwrap();
        Some(TOKEN.into())
    });
    assert_eq!(r.unwrap(), Outcome::Waiting);
    assert_eq!(b.state(a).0, "approved");
    assert!(!b.writing(a));
    assert_eq!(b.calls("messages"), 0);
}

/// An action with no write call: the mode changes to observe during the
/// reads, and the action waits.
#[test]
fn an_action_with_no_write_checks_the_mode_at_its_end() {
    let b = setup("late-mode-nowrite", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    for &m in &ms {
        b.fake.modify_labels(&b.provider_id(m), &[], &["INBOX"]);
    }
    let a = b.approved("archive", &ms, PERSON);
    b.fake.fail_next(&[0, 401]);
    let r = b.apply_as(b.account, a, &mut || {
        b.store
            .write(|tx| {
                Ok(tx.execute(
                    "INSERT INTO policy_version(text, sha256, confirmed_at)
                     VALUES ('version = 0\nmode = \"observe\"\n', 'h5', 5)",
                    [],
                )?)
            })
            .unwrap();
        Some(TOKEN.into())
    });
    assert_eq!(r.unwrap(), Outcome::Waiting);
    assert_eq!(b.state(a).0, "approved");
}

/// A token of another mailbox changes nothing.
#[test]
fn the_token_must_be_of_the_account() {
    let b = setup("mailbox", 40, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    let a = b.approved("archive", &ms, PERSON);
    b.store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE account SET address = 'someone@example.com' WHERE id = ?1",
                [b.account],
            )?)
        })
        .unwrap();
    assert!(matches!(b.apply(a), Err(Error::OtherMailbox)));
    assert_eq!(b.state(a).0, "approved");
    assert!(!b.writing(a));
    assert_eq!(b.calls("messages"), 0);
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
    // The claim numbers the calls in the order of the message IDs: 1,000
    // messages in call 1, the rest in call 2.
    let (state, call, rows) = b.ledger(a);
    assert_eq!(
        (state.as_str(), call, rows.len() as u64),
        ("applied", 2, need)
    );
    let calls: Vec<i64> = b
        .store
        .read(|c| {
            let mut st = c.prepare(
                "SELECT call FROM ledger_message WHERE ledger_id =
                   (SELECT id FROM ledger WHERE action_id = ?1) ORDER BY message_id",
            )?;
            let rows = st.query_map([a], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    let first = calls.iter().filter(|&&c| c == 1).count();
    assert_eq!(first, 1000.min(need as usize));
    assert!(calls.windows(2).all(|w| w[0] <= w[1]));
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
    b.check_document(a);
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

/// An unknown result starts the recovery at once. A call that Gmail
/// applied is confirmed, and the action is applied. A call that did not
/// arrive is never made again: the person can have reversed the change in
/// Gmail, and Iris keeps it. The action is failed, with the messages listed.
#[test]
fn an_unknown_result_is_read_and_no_call_repeats() {
    let b = setup("unknown", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 3);
    let a = b.approved("archive", &ms, PERSON);
    b.fake.fail_next_write(&[503], true);
    let c = applied(b.apply(a));
    assert_eq!(c.changed, 3);
    assert_eq!(b.calls("batchModify"), 1);

    let a = b.approved("inbox", &ms, PERSON);
    b.fake.fail_next_write(&[503], false);
    match b.apply(a) {
        Ok(Outcome::Failed { counts, error }) => {
            assert!(error.contains("cannot confirm"), "{error}");
            assert_eq!(counts.unknown, ms);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(b.state(a), ("failed".into(), "person".into()));
    assert!(matches!(b.apply(a), Err(Error::NotApproved(_))));
    assert_eq!(b.calls("batchModify"), 2);
    for &m in &ms {
        assert!(!b.gmail_labels(m).contains(&"INBOX".to_string()));
    }
    b.check_document(a);
}

/// A read of the recovery that fails: the row stays in `writing`, the
/// action stays approved, and the next apply continues the recovery.
#[test]
fn a_failed_recovery_read_keeps_the_row() {
    let b = setup("recovery-read", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    let a = b.approved("archive", &ms, PERSON);
    b.stopped(a, &ms, 1);
    // The profile call passes, the first read gets 400.
    b.fake.fail_next(&[0, 400]);
    assert!(matches!(b.apply(a), Err(Error::Gmail(_))));
    assert!(b.writing(a));
    assert_eq!(b.state(a).0, "approved");
    assert!(matches!(b.apply(a), Ok(Outcome::Failed { .. })));
    assert_eq!(b.calls("batchModify"), 0);
}

/// HTTP 401: one new token and one call again. With no new token before
/// the first write call, the action stays as it was.
#[test]
fn a_refused_token_gets_one_new_token() {
    let b = setup("token", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 3);
    let t = b.approved("trash", &ms, PERSON);
    b.fake.fail_next_write(&[401], false);
    let mut renewed = 0;
    let c = match b.apply_as(b.account, t, &mut || {
        renewed += 1;
        Some(TOKEN.into())
    }) {
        Ok(Outcome::Applied(c)) => c,
        other => panic!("{other:?}"),
    };
    assert_eq!((c.changed, renewed), (3, 1));
    for &m in &ms {
        assert_eq!(b.mirror_labels(m), b.gmail_labels(m));
    }

    let ms = b.messages(IN_INBOX, 2);
    let a = b.approved("archive", &ms, PERSON);
    b.fake.fail_next(&[401]);
    assert!(matches!(
        b.apply_as(b.account, a, &mut || None),
        Err(Error::Gmail(iris_gmail::Error::Unauthorized))
    ));
    assert_eq!(b.state(a).0, "approved");
    assert!(!b.writing(a));

    // A new token that Gmail refuses too: the caller learns it, to set the
    // account to needs sign-in.
    b.fake.fail_next(&[401, 401]);
    let mut renew = || Some(TOKEN.to_string());
    let mut token = Token {
        token: TOKEN.into(),
        renew: &mut renew,
        refused: false,
    };
    assert!(matches!(
        apply(&b.store, &b.gmail, &mut token, b.account, a),
        Err(Error::Gmail(iris_gmail::Error::Unauthorized))
    ));
    assert!(token.refused);
    applied(b.apply(a));
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
        b.apply_as(b.account + 1, other, &mut || None),
        Ok(Outcome::Denied(_))
    ));

    // A verb with no Gmail apply: a state verb (issue #164).
    let close = b.approved("close", &ms, PERSON);
    assert!(matches!(b.apply(close), Err(Error::Verb(_))));
    assert_eq!(b.state(close).0, "approved");
    assert_eq!(b.calls("messages"), 0);
}

/// A stop between two calls: the recovery confirms the call that started,
/// never makes it again, and makes each call that never started.
#[test]
fn the_recovery_continues_after_the_call_that_started() {
    let b = setup("between-calls", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 3);
    let a = b.approved("trash", &ms, PERSON);
    b.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
                 VALUES (?1, ?2, '{\"call\":\"trash\",\"add\":[\"TRASH\"],\"remove\":[],\"calls\":3}', 'writing', 'inverse', 1)",
                params![a, b.account],
            )?;
            let l = tx.last_insert_rowid();
            for (i, m) in ms.iter().enumerate() {
                tx.execute(
                    "INSERT INTO ledger_message(ledger_id, message_id, call, pre, post)
                     VALUES (?1, ?2, ?3, '[\"INBOX\"]', '[\"TRASH\"]')",
                    params![l, m, i as i64 + 1],
                )?;
            }
            Ok(tx.execute("UPDATE ledger SET call = 1 WHERE id = ?1", [l])?)
        })
        .unwrap();
    // Call 1 reached Gmail before the stop.
    b.gmail.trash(TOKEN, &b.provider_id(ms[0])).unwrap();
    let before = b.calls("trash");
    let c = applied(b.apply(a));
    assert_eq!(c.changed, 3);
    assert_eq!(b.calls("trash") - before, 2);
    let (state, call, rows) = b.ledger(a);
    assert_eq!((state.as_str(), call), ("applied", 3));
    assert!(rows.iter().all(|r| r.3));
    for &m in &ms {
        assert!(b.gmail_labels(m).contains(&"TRASH".to_string()));
        assert_eq!(b.mirror_labels(m), b.gmail_labels(m));
    }
}

/// `label` adds the user label of the name, with no regard to case, and
/// `unlabel` removes it. Gmail and the mirror agree after each apply.
#[test]
fn label_and_unlabel_change_gmail_and_the_mirror() {
    let b = setup("label", 80, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 6);
    let receipts = "Label_1".to_string();
    let need = ms
        .iter()
        .filter(|&&m| !b.gmail_labels(m).contains(&receipts))
        .count() as u64;
    assert!(need > 0, "the test changes nothing");
    let a = b.labelled("label", "receipts", &ms);
    let c = applied(b.apply(a));
    assert_eq!((c.changed, c.unchanged), (need, 6 - need));
    assert_eq!(b.calls("labels"), 0, "no label is made");
    b.check_document(a);
    for &m in &ms {
        let at_gmail = b.gmail_labels(m);
        assert!(at_gmail.contains(&receipts), "{at_gmail:?}");
        assert_eq!(b.mirror_labels(m), at_gmail);
    }
    let a = b.labelled("unlabel", "Receipts", &ms);
    let c = applied(b.apply(a));
    assert_eq!((c.changed, c.unchanged), (6, 0));
    for &m in &ms {
        let at_gmail = b.gmail_labels(m);
        assert!(!at_gmail.contains(&receipts), "{at_gmail:?}");
        assert_eq!(b.mirror_labels(m), at_gmail);
    }
}

/// A `label` action makes its label at Gmail one time, before its claim, and
/// the mirror gets the label. A second action of the name uses it.
#[test]
fn a_label_action_makes_its_label() {
    let b = setup("label-new", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 3);
    let a = b.labelled("label", "Iris/Later", &ms);
    let c = applied(b.apply(a));
    assert_eq!((c.changed, c.unchanged), (3, 0));
    assert_eq!(b.calls("/labels"), 1);
    let id = label_id(&b, "Iris/Later").expect("the mirror has the label");
    for &m in &ms {
        let at_gmail = b.gmail_labels(m);
        assert!(at_gmail.contains(&id), "{at_gmail:?}");
        assert_eq!(b.mirror_labels(m), at_gmail);
    }
    let more = b.messages(IN_INBOX, 5);
    let a = b.labelled("label", "Iris/Later", &more);
    let c = applied(b.apply(a));
    assert_eq!((c.changed, c.unchanged), (2, 3));
    assert_eq!(b.calls("/labels"), 1, "the label is not made again");
}

/// After an unknown result or HTTP 409 of the create, the apply reads the
/// labels and uses the label of the name. With no such label, the action
/// fails. The create is not called again.
#[test]
fn a_create_with_no_clear_result_reads_the_labels() {
    let b = setup("label-unknown", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    // Gmail makes the label, and the answer is lost.
    b.fake.fail_next_write(&[503], true);
    let a = b.labelled("label", "Lost", &ms);
    let c = applied(b.apply(a));
    assert_eq!(c.changed, 2);
    assert_eq!(b.calls("/labels"), 1);
    // Another client made the label at the same time: HTTP 409.
    b.fake.fail_next_write(&[409], true);
    let a = b.labelled("label", "Raced", &ms);
    applied(b.apply(a));
    assert!(label_id(&b, "Raced").is_some());
    // HTTP 409 with no label of the name: failed, and no batchModify.
    let modifies = b.calls("batchModify");
    b.fake.fail_next_write(&[409], false);
    let a = b.labelled("label", "Never", &ms);
    match b.apply(a) {
        Ok(Outcome::Failed { counts, .. }) => assert_eq!(counts.changed, 0),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.state(a).0, "failed");
    assert_eq!(b.calls("/labels"), 3);
    assert_eq!(b.calls("batchModify"), modifies);
}

/// `unlabel` of a name that Gmail does not have changes nothing.
#[test]
fn an_unlabel_of_no_label_changes_nothing() {
    let b = setup("unlabel-none", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 3);
    let a = b.labelled("unlabel", "Nowhere", &ms);
    let c = applied(b.apply(a));
    assert_eq!((c.changed, c.unchanged), (0, 3));
    assert_eq!(
        b.fake
            .requests()
            .iter()
            .filter(|r| r.starts_with("POST"))
            .count(),
        0
    );
}

/// A name of a system label at Gmail is denied, also when the changeset
/// store did not know the label.
#[test]
fn a_system_label_name_is_denied() {
    let b = setup("label-system", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    for name in ["inbox", "Starred"] {
        let a = b.labelled("label", name, &ms);
        match b.apply(a) {
            Ok(Outcome::Denied(_)) => {}
            other => panic!("{name}: {other:?}"),
        }
        assert_eq!(b.state(a), ("denied".into(), "engine".into()));
    }
    assert_eq!(
        b.fake
            .requests()
            .iter()
            .filter(|r| r.starts_with("POST"))
            .count(),
        0
    );
}

/// One Unicode case folding in submit, the apply and the fake: a name that
/// differs only in case finds the label of Gmail, also where a lower case
/// does not equate the pair (`Σ` and final `ς`). No label is made.
#[test]
fn a_label_name_matches_with_case_folding() {
    let b = setup("label-fold", 60, Some(APPLY_POLICY));
    let id = b.fake.create_label("Σ");
    let ms = b.messages(IN_INBOX, 2);
    for name in ["ς", "σ", "Σ"] {
        let a = b.labelled("label", name, &ms);
        applied(b.apply(a));
        for &m in &ms {
            assert!(b.gmail_labels(m).contains(&id), "{name}");
        }
        let a = b.labelled("unlabel", name, &ms);
        let c = applied(b.apply(a));
        assert_eq!(c.changed, 2, "{name}");
    }
    assert_eq!(b.calls("/labels"), 0, "no label is made");
}

/// The rows in `writing` of the account end before any outcome of another
/// action: here the other action waits for the mode `apply`, and the row
/// still recovers.
#[test]
fn a_waiting_action_still_recovers_the_rows_of_the_account() {
    let b = setup("observe-recover", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    let first = b.approved("archive", &ms, PERSON);
    b.stopped(first, &ms, 1);
    let second = b.approved("unread", &ms, PERSON);
    b.store
        .write(|tx| {
            Ok(tx.execute(
                "INSERT INTO policy_version(text, sha256, confirmed_at)
                 VALUES ('version = 0\nmode = \"observe\"\n', 'h6', 6)",
                [],
            )?)
        })
        .unwrap();
    assert_eq!(b.apply(second).unwrap(), Outcome::Waiting);
    assert!(!b.writing(first));
    assert_eq!(b.state(first).0, "failed");
    assert_eq!(b.state(second).0, "approved");
    assert_eq!(b.calls("batchModify"), 0);
}

/// A label create with no label after it: the action fails only after the
/// same checks as the claim. Here the person changed the mode to observe
/// during the create, so the action waits.
#[test]
fn a_failed_label_create_checks_the_mode_again() {
    let b = setup("label-gate", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    let a = b.labelled("label", "Never", &ms);
    // The profile call passes; labels.list gets 401, and the new token
    // comes after the person changed the mode.
    b.fake.fail_next(&[0, 401]);
    b.fake.fail_next_write(&[409], false);
    let r = b.apply_as(b.account, a, &mut || {
        b.store
            .write(|tx| {
                Ok(tx.execute(
                    "INSERT INTO policy_version(text, sha256, confirmed_at)
                     VALUES ('version = 0\nmode = \"observe\"\n', 'h7', 7)",
                    [],
                )?)
            })
            .unwrap();
        Some(TOKEN.into())
    });
    assert_eq!(r.unwrap(), Outcome::Waiting);
    assert_eq!(b.state(a).0, "approved");
    assert_eq!(b.calls("/labels"), 1);
    assert_eq!(b.calls("batchModify"), 0);
}

/// A job whose scope holds the thread of each message in `ms`. Each next
/// changeset is of a session of the job. The mirror messages count as
/// understood, with a From address, so the grant can hold them.
fn session_job(b: &Box, ms: &[i64]) -> i64 {
    b.store
        .write(|tx| {
            tx.execute_batch(
                "UPDATE message SET understood_at = 1;
                 INSERT OR IGNORE INTO header(message_id, from_addr)
                   SELECT id, 'dana@example.com' FROM message;
                 UPDATE header SET from_addr = 'dana@example.com' WHERE from_addr IS NULL;",
            )?;
            Ok(())
        })
        .unwrap();
    let job = iris_job::create(&b.store, "Tidy the inbox.", None, now()).unwrap();
    let threads: std::collections::BTreeSet<i64> = ms
        .iter()
        .map(|m| {
            b.store
                .read(|c| {
                    Ok(
                        c.query_row("SELECT thread_id FROM message WHERE id = ?1", [m], |r| {
                            r.get(0)
                        })?,
                    )
                })
                .unwrap()
        })
        .collect();
    for t in &threads {
        iris_job::add_scope(
            &b.store,
            job,
            iris_job::Ref::Thread(*t),
            iris_job::By::Person,
            now(),
        )
        .unwrap();
    }
    *b.job.lock().unwrap() = Some(job);
    job
}

fn revoke_all(b: &Box, job: i64) {
    for r in iris_job::scope(&b.store, job).unwrap() {
        iris_job::revoke_scope(&b.store, job, r, iris_job::By::Person, now()).unwrap();
    }
}

/// A session action in the grant applies. After a revoke, the next apply
/// denies it before any provider call (docs/specs/2026-11-17-job-verbs.md).
#[test]
fn a_session_action_applies_only_inside_the_grant() {
    let b = setup("grant", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    session_job(&b, &ms[..1]);
    let a = b.approved("archive", &ms[..1], PERSON);
    applied(b.apply(a));

    let job = session_job(&b, &ms[1..]);
    let a = b.approved("archive", &ms[1..], PERSON);
    revoke_all(&b, job);
    let before = b.calls("batchModify");
    match b.apply(a) {
        Ok(Outcome::Denied(why)) => assert!(why.contains("outside the grant"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(b.state(a), ("denied".into(), "engine".into()));
    assert_eq!(b.calls("batchModify"), before);
    assert!(b.gmail_labels(ms[1]).contains(&"INBOX".to_string()));
}

/// The check of a label runs before `labels.create`.
#[test]
fn a_session_label_outside_the_grant_makes_no_label() {
    let b = setup("grant-label", 30, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 1);
    let job = session_job(&b, &ms);
    let a = b.labelled("label", "Brand new", &ms);
    revoke_all(&b, job);
    assert!(matches!(b.apply(a), Ok(Outcome::Denied(_))));
    assert_eq!(b.calls("labels"), 0);
    assert_eq!(label_id(&b, "Brand new"), None);
}

/// A revoke between two calls of one action: the call that ran stays done,
/// the later call never runs, and the entry is failed.
#[test]
fn a_revoke_between_two_calls_stops_the_later_calls() {
    let b = setup("grant-calls", 60, Some(APPLY_POLICY));
    let ms = b.messages(IN_INBOX, 2);
    let job = session_job(&b, &ms);
    let a = b.approved("archive", &ms, PERSON);
    // Call 1 ran and Gmail confirmed it; the engine stopped before call 2.
    b.fake.modify_labels(&b.provider_id(ms[0]), &[], &["INBOX"]);
    b.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
                 VALUES (?1, ?2, '{\"call\":\"batchModify\",\"add\":[],\"remove\":[\"INBOX\"],\"calls\":2}', 'writing', 'inverse', 1)",
                params![a, b.account],
            )?;
            let l = tx.last_insert_rowid();
            for (i, m) in ms.iter().enumerate() {
                tx.execute(
                    "INSERT INTO ledger_message(ledger_id, message_id, call, pre, post)
                     VALUES (?1, ?2, ?3, '[\"INBOX\"]', '[]')",
                    params![l, m, i as i64 + 1],
                )?;
            }
            tx.execute("UPDATE ledger SET call = 1 WHERE id = ?1", [l])?;
            tx.execute(
                "UPDATE ledger_message SET done = 1 WHERE ledger_id = ?1 AND call = 1",
                [l],
            )?;
            Ok(())
        })
        .unwrap();
    revoke_all(&b, job);
    match b.apply(a) {
        Ok(Outcome::Failed { error, .. }) => {
            assert!(error.contains("outside the grant"), "{error}")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(b.calls("batchModify"), 0);
    let (state, call, rows) = b.ledger(a);
    assert_eq!((state.as_str(), call), ("failed", 1));
    let done: Vec<bool> = rows.iter().map(|r| r.3).collect();
    assert_eq!(done, vec![true, false]);
    assert!(b.gmail_labels(ms[1]).contains(&"INBOX".to_string()));
}
