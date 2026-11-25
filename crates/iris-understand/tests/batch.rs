//! The batch runner and the estimate of docs/specs/2026-11-16-cost-control.md,
//! with a fake model. No key, no network.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;

use iris_model::{Error as ModelError, Reply, Stop, Usage};
use iris_store::{NewMessage, Store};
use iris_understand::batch::{self, Error, Limits};
use iris_understand::budget::ModelRow;
use iris_understand::Model;

const NOW: i64 = 1_767_225_600_000; // 2026-01-01
const DAY: i64 = 86_400_000;
const VALID: &str = r#"{"items": [], "phishing": false}"#;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-batch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One answer of the fake: the text and the usage, or an error.
type Answer = Result<(String, Usage), ModelError>;

fn usage(input: u64, output: u64) -> Usage {
    Usage {
        input_tokens: input,
        output_tokens: output,
        cost_micro_usd: None,
        known: true,
    }
}

/// Shared by the test and each model that `connect` makes.
#[derive(Default)]
struct Script {
    answers: RefCell<VecDeque<Answer>>,
    calls: RefCell<u32>,
    /// Runs in the next call, before the answer: a write of another
    /// writer during the call.
    during: RefCell<Option<std::boxed::Box<dyn FnOnce()>>>,
}

impl Script {
    fn push(&self, a: Answer) {
        self.answers.borrow_mut().push_back(a);
    }

    fn calls(&self) -> u32 {
        *self.calls.borrow()
    }
}

struct Fake(Rc<Script>);

impl Model for Fake {
    fn name(&self) -> &str {
        "fake-1"
    }

    fn request_bytes(&self, _: &str, _: &str) -> u64 {
        1000
    }

    fn ask(&self, _: &str, _: &str) -> Result<Reply, ModelError> {
        *self.0.calls.borrow_mut() += 1;
        if let Some(during) = self.0.during.borrow_mut().take() {
            during();
        }
        let (text, usage) = self
            .0
            .answers
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok((VALID.into(), usage(100, 10))))?;
        Ok(Reply {
            stop: Stop::End,
            text,
            usage,
        })
    }
}

struct Box_ {
    _dir: TempDir,
    store: Store,
    account: i64,
    script: Rc<Script>,
}

fn store(name: &str) -> Box_ {
    let dir = TempDir::new(name);
    let store = Store::open(&dir.0).unwrap();
    let account = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .put_labels(
            account,
            &[
                ("INBOX", "INBOX", "system"),
                ("SENT", "SENT", "system"),
                ("SPAM", "SPAM", "system"),
                ("DRAFT", "DRAFT", "system"),
                ("UNSENT", "UNSENT", "system"),
                ("SENT_COPY", "SENT_COPY", "system"),
            ],
        )
        .unwrap();
    Box_ {
        _dir: dir,
        store,
        account,
        script: Rc::default(),
    }
}

fn raw(n: &str) -> String {
    format!(
        "From: Ana Lee <ana@example.org>\r\nTo: owner@example.com\r\nSubject: Note {n}\r\n\
         Message-ID: <{n}@example.org>\r\nDate: Tue, 30 Dec 2025 08:00:00 +0000\r\n\
         Content-Type: text/plain\r\n\r\nHello, this is note {n}.\r\n"
    )
}

impl Box_ {
    /// Adds message `id` in `thread`, `age` days before NOW.
    fn add(&self, id: &str, thread: &str, age: i64, labels: &[&str]) -> i64 {
        let raw = raw(id);
        let sha = self.store.put_content(raw.as_bytes()).unwrap();
        self.store
            .put_messages(
                self.account,
                &[NewMessage {
                    provider_id: id.into(),
                    thread_provider_id: thread.into(),
                    content_sha256: Some(sha),
                    size: Some(raw.len() as i64),
                    internal_date: Some(NOW - age * DAY),
                    labels: labels.iter().map(|l| l.to_string()).collect(),
                    ..NewMessage::default()
                }],
            )
            .unwrap()[0]
    }

    /// Adds a message with the raw text `raw`.
    fn add_raw(&self, id: &str, thread: &str, age: i64, raw: &str, labels: &[&str]) -> i64 {
        let raw = raw.replace('\n', "\r\n");
        let sha = self.store.put_content(raw.as_bytes()).unwrap();
        self.store
            .put_messages(
                self.account,
                &[NewMessage {
                    provider_id: id.into(),
                    thread_provider_id: thread.into(),
                    content_sha256: Some(sha),
                    size: Some(raw.len() as i64),
                    internal_date: Some(NOW - age * DAY),
                    labels: labels.iter().map(|l| l.to_string()).collect(),
                    ..NewMessage::default()
                }],
            )
            .unwrap()[0]
    }

    /// A model row: priced (1 micro-USD per token in and out) or local.
    fn model(&self, priced: bool, budget: Option<i64>) {
        self.store
            .write(|tx| {
                tx.execute(
                    "INSERT OR REPLACE INTO model_setting(id, provider, base_url, model, version,
                       price_in, price_out, max_tokens, timeout_s, budget_micro_usd)
                     VALUES (1, ?1, 'http://127.0.0.1:1/v1', 'fake-1', 1, ?2, ?2, 100, 30, ?3)",
                    rusqlite::params![
                        if priced { "anthropic" } else { "local" },
                        priced.then_some(1_000_000),
                        budget
                    ],
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn run(&self, limits: &Limits) -> Result<batch::Report, Error> {
        let script = self.script.clone();
        let connect = move |_: &ModelRow| -> Result<Box<dyn Model>, String> {
            Ok(Box::new(Fake(script.clone())))
        };
        batch::run(&self.store, &connect, limits, NOW)
    }

    fn col(&self, sql: &str) -> Option<i64> {
        self.store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    fn understood(&self, m: i64) -> bool {
        self.col(&format!("SELECT understood_at FROM message WHERE id = {m}"))
            .is_some()
    }

    fn budget(&self) -> Option<i64> {
        self.col("SELECT budget_micro_usd FROM model_setting")
    }
}

// The worst case of one call of the fake: 1000 bytes + 100 max tokens.
const WORST: i64 = 1100;
// The cost of one call with the default usage (100 in, 10 out).
const COST: i64 = 110;

#[test]
fn a_second_run_makes_zero_calls() {
    let b = store("second");
    b.model(false, None);
    for (id, t) in [("1", "a"), ("2", "a"), ("3", "b")] {
        b.add(id, t, 1, &["INBOX"]);
    }
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("done"));
    assert_eq!((r.messages, r.calls, b.script.calls()), (3, 3, 3));
    assert_eq!((r.input_tokens, r.output_tokens), (300, 30));
    assert_eq!(r.cost_micro_usd, None, "no price: no cost");
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!((r.stop.as_deref(), r.calls), (Some("done"), 0));
    assert_eq!(b.script.calls(), 3);
}

#[test]
fn a_priced_model_with_no_budget_makes_zero_calls_but_free_steps_run() {
    let b = store("nobudget");
    b.model(true, None);
    let m = b.add("1", "a", 2, &["INBOX"]);
    // A newer SPAM message: hostile mail does not wait for the older one.
    let spam = b.add("2", "a", 1, &["SPAM"]);
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("budget"));
    assert_eq!((r.calls, b.script.calls()), (0, 0));
    assert!(!b.understood(m));
    assert!(b.understood(spam), "the free step ran with no budget");
    assert_eq!(
        b.col(&format!("SELECT tries FROM message WHERE id = {m}")),
        Some(0),
        "a refused call counts no try"
    );
}

#[test]
fn no_model_runs_the_free_steps() {
    let b = store("nomodel");
    let m = b.add("1", "a", 1, &["INBOX"]);
    let spam = b.add("2", "b", 1, &["SPAM"]);
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("no_model"));
    assert!(b.understood(spam) && !b.understood(m));
}

/// Issue 106: a run computes again the conversations of the threads that a
/// sender kind change marked, and empties the queue.
#[test]
fn a_run_updates_the_talks_of_a_kind_change() {
    let b = store("talkdue");
    b.add("1", "a", 1, &["INBOX"]);
    b.run(&Limits::default()).unwrap();
    b.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO sender(address, kind) VALUES ('ana@example.org', 'service')
                 ON CONFLICT(address) DO UPDATE SET kind = 'service', decided_by = 'person'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(due(&b), Some(1));
    b.run(&Limits::default()).unwrap();
    assert_eq!(due(&b), Some(0));
}

fn due(b: &Box_) -> Option<i64> {
    b.col("SELECT count(*) FROM talk_due")
}

/// Issue 106: a kind change that the understanding of the last messages of
/// a run makes (a sent message makes its recipient a person) marks talks
/// after the last sender refresh. The run empties the queue before it ends.
#[test]
fn a_run_ends_with_no_marked_talk() {
    let b = store("talkend");
    b.add("1", "a", 2, &["INBOX"]);
    b.run(&Limits::default()).unwrap();
    assert_eq!(due(&b), Some(0));
    b.add_raw(
        "2",
        "b",
        1,
        "From: owner@example.com\nTo: Ana Lee <ana@example.org>\nSubject: Hi\n\
         Message-ID: <s2@example.com>\nDate: Wed, 31 Dec 2025 08:00:00 +0000\n\
         Content-Type: text/plain\n\nHello Ana.\n",
        &["SENT"],
    );
    b.run(&Limits::default()).unwrap();
    assert_eq!(
        b.col("SELECT count(*) FROM sender WHERE address = 'ana@example.org' AND kind = 'person'"),
        Some(1),
        "the sent message makes Ana a person"
    );
    assert_eq!(due(&b), Some(0));
}

/// Issue 106: migration 40 marks each thread with understood mail, so a
/// conversation that a kind change of an older store left stale is
/// computed again once.
#[test]
fn migration_40_marks_each_understood_thread() {
    let b = store("talkup");
    b.model(false, None);
    b.add("1", "a", 2, &["INBOX"]);
    b.add("2", "b", 1, &["INBOX"]);
    b.run(&Limits::default()).unwrap();
    b.add("3", "c", 1, &["INBOX"]);
    b.store
        .write(|tx| {
            tx.execute_batch(
                "DROP TRIGGER talk_due_sender_add; DROP TRIGGER talk_due_sender;
                 DROP TRIGGER talk_due_sender_remove; DROP TRIGGER model_declined_end; DROP TRIGGER model_declined_end_remove; DROP TABLE model_declined; ALTER TABLE reply_draft DROP COLUMN seen; ALTER TABLE header DROP COLUMN reply_to; ALTER TABLE header DROP COLUMN content_sha256; DROP TRIGGER IF EXISTS spam_drops_items; DROP TRIGGER IF EXISTS spam_back_drops_items; DROP TRIGGER IF EXISTS understand_reset; DROP TRIGGER IF EXISTS understand_new_content; DROP TRIGGER IF EXISTS understand_come_back; DROP TRIGGER understand_owner_label_add; DROP TRIGGER understand_owner_label_remove; CREATE TRIGGER understand_owner_label_add AFTER INSERT ON message_label WHEN (SELECT provider_id FROM label WHERE id = NEW.label_id) IN ('SENT', 'DRAFT') BEGIN UPDATE message SET understood_at = NULL WHERE id = NEW.message_id AND understood_at IS NOT NULL; END; CREATE TRIGGER understand_owner_label_remove AFTER DELETE ON message_label WHEN (SELECT provider_id FROM label WHERE id = OLD.label_id) IN ('SENT', 'DRAFT') BEGIN UPDATE message SET understood_at = NULL WHERE id = OLD.message_id AND understood_at IS NOT NULL; END; DROP TABLE talk_due; DROP TRIGGER sent_copy_keep; DROP TRIGGER sent_copy_add; DROP VIEW iris_sent_copy; DROP INDEX draft_sent_copy; DROP TRIGGER draft_or_copy_drops_items; DROP TRIGGER draft_or_copy_talk_add; DROP TRIGGER draft_or_copy_talk_remove; DROP TRIGGER draft_or_copy_back_drops_items; DROP VIEW item_droppable;
                 DROP TRIGGER changeset_withdraw_by; DROP TRIGGER changeset_withdrawn_by; ALTER TABLE changeset DROP COLUMN withdrawn_by; DROP TRIGGER matter_add_end; DROP TRIGGER matter_item_kept_add; DROP TRIGGER matter_item_kept_update; DROP TRIGGER matter_add_insert; DROP TRIGGER matter_add_update; DROP VIEW matter_add_live; DROP TABLE matter_add; DROP TRIGGER matter_insert; DROP TRIGGER matter_update; DROP TRIGGER matter_decide; DROP TRIGGER matter_item_insert; DROP TRIGGER matter_item_update; DROP TRIGGER matter_delete; ALTER TABLE matter DROP COLUMN author; DROP TRIGGER ledger_hostile_before; ALTER TABLE ledger DROP COLUMN hostile_before; DROP TRIGGER fixed_due_release; DROP TRIGGER message_release_content; ALTER TABLE message DROP COLUMN released; ALTER TABLE message DROP COLUMN fixed_rules; DROP TRIGGER sender_gate_rev; ALTER TABLE sender DROP COLUMN gate_rev; PRAGMA user_version = 39;",
            )?;
            Ok(())
        })
        .unwrap();
    let store = Store::open(&b._dir.0).unwrap();
    let due: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM talk_due", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(due, 2, "the two understood threads, not the new one");
}

#[test]
fn two_runs_share_one_budget() {
    let b = store("share");
    b.model(true, Some(WORST + 100));
    b.add("1", "a", 1, &["INBOX"]);
    b.add("2", "b", 2, &["INBOX"]);
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!((r.stop.as_deref(), r.calls), (Some("budget"), 1));
    assert_eq!(r.cost_micro_usd, Some(COST));
    assert_eq!(r.reserved_micro_usd, 0);
    assert_eq!(b.budget(), Some(WORST + 100 - COST));
    // The refused message waits for a larger budget, so the second run
    // reads nothing and makes no call.
    assert_eq!(
        b.col("SELECT count(*) FROM message WHERE waits_model = 1"),
        Some(1)
    );
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!((r.stop.as_deref(), r.calls), (Some("done"), 0));
    assert_eq!(b.script.calls(), 1);
}

/// The end of the wait after a model error clears only the marks of that
/// wait: a budget refusal of the same run keeps its mark.
#[test]
fn the_end_of_a_model_wait_keeps_the_marks_of_a_budget_refusal() {
    let b = store("wait-end");
    b.model(true, Some(WORST + 100));
    let waited = b.add("1", "a", 1, &["INBOX"]);
    b.add("2", "b", 2, &["INBOX"]);
    // The state after a model error whose wait has ended, with mail that
    // waited for it.
    b.store
        .write(|tx| {
            tx.execute(
                "UPDATE model_setting SET retry_failures = 1, retry_at = 0",
                [],
            )?;
            tx.execute("UPDATE message SET waits_model = 2 WHERE id = ?1", [waited])?;
            Ok(())
        })
        .unwrap();
    let limits = Limits {
        keep_idle: false,
        wait_after_error: true,
        ..Limits::default()
    };
    let r = b.run(&limits).unwrap();
    assert_eq!((r.stop.as_deref(), r.calls), (Some("budget"), 1));
    assert_eq!(
        b.col("SELECT count(*) FROM message WHERE waits_model = 1"),
        Some(1),
        "the budget refusal keeps its mark"
    );
    assert_eq!(
        b.col("SELECT retry_failures FROM model_setting"),
        Some(0),
        "the call ended the wait"
    );
    assert!(b.run(&limits).unwrap().is_idle());
    assert_eq!(b.script.calls(), 1);
}

#[test]
fn unknown_usage_keeps_the_worst_case_and_stops() {
    let b = store("unknown");
    b.model(true, Some(10_000));
    // No usage and an answer that is not valid: the second try is refused.
    b.script.push(Ok(("not json".into(), Usage::default())));
    let m = b.add("1", "a", 1, &["INBOX"]);
    b.add("2", "b", 2, &["INBOX"]);
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("unknown"));
    assert_eq!(b.script.calls(), 1);
    assert_eq!((r.calls, r.unknown_calls), (0, 1));
    assert_eq!(r.unknown_cost_micro_usd, Some(WORST));
    assert_eq!(b.budget(), Some(10_000 - WORST));
    assert!(!b.understood(m));
    assert_eq!(
        b.col(&format!("SELECT tries FROM message WHERE id = {m}")),
        Some(0)
    );
}

#[test]
fn errors_with_no_usage_stop_the_run_with_the_worst_case() {
    for e in [
        ModelError::Transport("time-out".into()),
        ModelError::Server(529),
        ModelError::RateLimited { retry_after: None },
        ModelError::BadAnswer("no usage".into()),
    ] {
        let b = store("errors");
        b.model(true, Some(10_000));
        b.script.push(Err(e));
        b.add("1", "a", 1, &["INBOX"]);
        let r = b.run(&Limits::default()).unwrap();
        assert_eq!(r.stop.as_deref(), Some("unknown"));
        assert_eq!(b.script.calls(), 1);
        assert_eq!(b.budget(), Some(10_000 - WORST));
    }
}

#[test]
fn a_refused_key_gives_the_reservation_back() {
    let b = store("key");
    b.model(true, Some(10_000));
    b.script.push(Err(ModelError::Rejected {
        status: 401,
        message: "bad key".into(),
    }));
    let m = b.add("1", "a", 1, &["INBOX"]);
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("model"));
    assert_eq!((r.calls, r.input_tokens, r.unknown_calls), (1, 0, 0));
    assert_eq!(b.budget(), Some(10_000));
    assert!(!b.understood(m));
}

/// A priced model stops the run after a refusal for size (the charge is
/// not known), so the call with the small data budget (#101) cannot go
/// out. The refusal then counts as a failed try, as before: the message
/// does not wait for a model change.
#[test]
fn a_priced_refusal_for_size_of_large_data_counts_a_failed_try() {
    let b = store("size");
    b.model(true, Some(10_000));
    b.script.push(Err(ModelError::Rejected {
        status: 413,
        message: "too large".into(),
    }));
    let big = format!(
        "From: Ana Lee <ana@example.org>\nTo: owner@example.com\nSubject: Big\n\
         Message-ID: <big@example.org>\nDate: Tue, 30 Dec 2025 08:00:00 +0000\n\
         Content-Type: text/plain\n\nCan you send the slides?\n{}\n",
        "Notes. ".repeat(5000)
    );
    let m = b.add_raw("big", "a", 1, &big, &["INBOX"]);
    b.run(&Limits::default()).unwrap();
    assert_eq!(b.script.calls(), 1);
    assert_eq!(
        b.col(&format!("SELECT tries FROM message WHERE id = {m}")),
        Some(1)
    );
    assert_eq!(
        b.col(&format!("SELECT waits_model FROM message WHERE id = {m}")),
        Some(0)
    );
}

#[test]
fn a_cost_over_the_reservation_stops_the_run() {
    let b = store("overrun");
    b.model(true, Some(2 * WORST));
    b.script.push(Ok((VALID.into(), usage(5_000, 10))));
    b.add("1", "a", 1, &["INBOX"]);
    b.add("2", "b", 2, &["INBOX"]);
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("budget"));
    assert_eq!(r.cost_micro_usd, Some(5_010));
    assert_eq!(b.budget(), Some(2 * WORST - 5_010));
    assert_eq!(b.script.calls(), 1);
}

#[test]
fn a_local_model_needs_no_budget_and_goes_on_with_no_usage() {
    let b = store("local");
    b.model(false, None);
    b.script.push(Ok((VALID.into(), Usage::default())));
    b.add("1", "a", 1, &["INBOX"]);
    b.add("2", "b", 2, &["INBOX"]);
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("done"));
    assert_eq!((r.calls, r.unknown_calls), (1, 1));
    assert_eq!(r.unknown_cost_micro_usd, None);
}

#[test]
fn a_local_server_error_stops_the_run() {
    let b = store("localerr");
    b.model(false, None);
    b.script.push(Err(ModelError::Transport("refused".into())));
    b.add("1", "a", 1, &["INBOX"]);
    b.add("2", "b", 2, &["INBOX"]);
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("model"));
    assert_eq!(b.script.calls(), 1);
}

#[test]
fn a_second_runner_gets_busy() {
    let b = store("lock");
    b.model(false, None);
    b.add("1", "a", 1, &["INBOX"]);
    let held = batch::lock(&b.store).unwrap();
    assert!(matches!(b.run(&Limits::default()), Err(Error::Busy)));
    assert_eq!(b.script.calls(), 0);
    drop(held);
    assert!(b.run(&Limits::default()).is_ok());
}

/// A draft or a question waits for the run lock (#279): the run stops
/// before its next message, the waiter gets the lock, and the next run
/// goes on with the mail that is left.
#[test]
fn a_run_yields_to_a_waiter_before_its_next_message() {
    let b = store("yield");
    b.model(false, None);
    let first = b.add("1", "a", 2, &["INBOX"]);
    let second = b.add("2", "b", 1, &["INBOX"]);
    let dir = b.store.dir().to_path_buf();
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let waiter = std::thread::spawn({
        let dir = dir.clone();
        move || {
            let store = Store::open(&dir).unwrap();
            wait.recv().unwrap();
            batch::lock_wait(&store, std::time::Duration::from_secs(30)).is_ok()
        }
    });
    // During the first call, the other caller starts to wait. The call
    // ends only when the waiter holds the want lock.
    let want = dir.join(batch::WANT_FILE);
    *b.script.during.borrow_mut() = Some(std::boxed::Box::new(move || {
        go.send(()).unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !matches!(
            iris_store::FileLock::probe(&want),
            Ok(iris_store::Probe::Held)
        ) {
            assert!(std::time::Instant::now() < end, "the waiter does not wait");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }));
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!((r.stop.as_deref(), r.messages), (Some("yield"), 1));
    assert!(waiter.join().unwrap(), "the waiter gets the run lock");
    assert!(b.understood(first) != b.understood(second), "one message");
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("done"));
    assert!(
        b.understood(first) && b.understood(second),
        "the next run goes on"
    );
}

/// While a caller waits (it holds the want lock), no new holder takes the
/// run lock with `batch::lock`, and the link and fixed scans stop before
/// their next row and leave the rest of the queue (#279).
#[test]
fn a_waiter_gets_the_lock_first_and_stops_the_scans() {
    let b = store("want");
    b.model(false, None);
    let m = b.add("1", "a", 2, &["INBOX"]);
    b.add("2", "b", 1, &["INBOX"]);
    b.store
        .write(|tx| Ok(tx.execute("INSERT INTO fixed_due(message_id) VALUES (?1)", [m])?))
        .unwrap();
    let links = b.col("SELECT count(*) FROM link_check").unwrap();
    assert!(links > 0, "new mail waits for its link check");
    let want = iris_store::FileLock::try_take(&b.store.dir().join(batch::WANT_FILE))
        .unwrap()
        .unwrap();
    assert!(matches!(batch::lock(&b.store), Err(Error::Busy)));
    assert!(matches!(b.run(&Limits::default()), Err(Error::Busy)));
    assert_eq!(b.script.calls(), 0);
    assert_eq!(
        iris_understand::store::recheck_links(&b.store, NOW, None).unwrap(),
        0
    );
    assert_eq!(b.col("SELECT count(*) FROM link_check"), Some(links));
    assert_eq!(
        iris_understand::store::recheck_fixed(&b.store, NOW, None).unwrap(),
        0
    );
    assert_eq!(b.col("SELECT count(*) FROM fixed_due"), Some(1));
    drop(want);
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("done"));
    assert_eq!(b.col("SELECT count(*) FROM link_check"), Some(0));
    assert_eq!(b.col("SELECT count(*) FROM fixed_due"), Some(0));
}

/// A waiter that comes after the last message stops the fixed check at the
/// end of the pass: the run says `yield`, not `done`, and the queued row
/// waits for the next run (#279).
#[test]
fn a_waiter_stops_the_last_fixed_check_and_the_run_says_yield() {
    let b = store("wantfixed");
    b.model(false, None);
    let m = b.add("1", "a", 1, &["INBOX"]);
    let want_path = b.store.dir().join(batch::WANT_FILE);
    let held = std::rc::Rc::new(std::cell::RefCell::new(None));
    let during = {
        let held = held.clone();
        let dir = b.store.dir().to_path_buf();
        move || {
            // A sender change queues the message for a fixed check, and a
            // draft starts to wait, during the only call of the pass.
            let s = Store::open(&dir).unwrap();
            s.write(|tx| Ok(tx.execute("INSERT INTO fixed_due(message_id) VALUES (?1)", [m])?))
                .unwrap();
            *held.borrow_mut() = iris_store::FileLock::try_take(&want_path).unwrap();
        }
    };
    *b.script.during.borrow_mut() = Some(std::boxed::Box::new(during));
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!((r.stop.as_deref(), r.messages), (Some("yield"), 1));
    assert_eq!(b.col("SELECT count(*) FROM fixed_due"), Some(1));
    drop(held.borrow_mut().take());
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("done"));
    assert_eq!(b.col("SELECT count(*) FROM fixed_due"), Some(0));
}

/// Two waiters take the run lock in turn: the one that waits first (it
/// holds the want lock) gets the lock first, and the second one gets it
/// after the first lets go (#279).
#[test]
fn two_waiters_take_the_lock_in_turn() {
    let b = store("twowait");
    let dir = b.store.dir().to_path_buf();
    let held = batch::lock(&b.store).unwrap();
    let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let waiter = |name: &'static str| {
        let (dir, order) = (dir.clone(), order.clone());
        std::thread::spawn(move || {
            let s = Store::open(&dir).unwrap();
            let l = batch::lock_wait(&s, std::time::Duration::from_secs(30)).unwrap();
            order.lock().unwrap().push(name);
            std::thread::sleep(std::time::Duration::from_millis(200));
            drop(l);
        })
    };
    let first = waiter("first");
    let want = dir.join(batch::WANT_FILE);
    while !matches!(
        iris_store::FileLock::probe(&want),
        Ok(iris_store::Probe::Held)
    ) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let second = waiter("second");
    std::thread::sleep(std::time::Duration::from_millis(200));
    drop(held);
    first.join().unwrap();
    second.join().unwrap();
    assert_eq!(*order.lock().unwrap(), ["first", "second"]);
}

/// A take of `batch::lock` holds a shared lock on the want file until it
/// ends. A waiter cannot signal during that time, so the two never take
/// the run lock in an order that the waiter cannot see (#279).
#[test]
fn a_take_in_progress_holds_back_a_waiter() {
    let b = store("inflight");
    drop(batch::lock(&b.store).unwrap());
    let want = b.store.dir().join(batch::WANT_FILE);
    let Ok(iris_store::Probe::Free(shared)) = iris_store::FileLock::probe(&want) else {
        panic!("the want file exists and is free");
    };
    assert!(matches!(
        batch::lock_wait(&b.store, std::time::Duration::from_millis(200)),
        Err(Error::Busy)
    ));
    // A second take of `batch::lock` goes on, as two shared locks agree.
    drop(batch::lock(&b.store).unwrap());
    drop(shared);
    assert!(batch::lock_wait(&b.store, std::time::Duration::ZERO).is_ok());
}

/// With no run, [`batch::lock_wait`] takes the lock at once. A holder that
/// does not let go gives `Busy` after the wait.
#[test]
fn lock_wait_takes_a_free_lock_and_ends_after_its_limit() {
    let b = store("lockwait");
    let held = batch::lock_wait(&b.store, std::time::Duration::ZERO).unwrap();
    let t = std::time::Instant::now();
    assert!(matches!(
        batch::lock_wait(&b.store, std::time::Duration::from_millis(200)),
        Err(Error::Busy)
    ));
    assert!(t.elapsed() >= std::time::Duration::from_millis(200));
    drop(held);
    assert!(batch::lock(&b.store).is_ok());
}

#[test]
fn max_messages_stops_in_the_thread() {
    let b = store("max");
    b.model(false, None);
    let first = b.add("1", "a", 2, &["INBOX"]);
    let second = b.add("2", "a", 1, &["INBOX"]);
    let limits = Limits {
        max_messages: Some(1),
        ..Limits::default()
    };
    let r = b.run(&limits).unwrap();
    assert_eq!((r.stop.as_deref(), r.messages), (Some("messages"), 1));
    assert!(b.understood(first) && !b.understood(second), "oldest first");
    let r = b.run(&limits).unwrap();
    assert_eq!(r.messages, 1);
    assert!(b.understood(second), "the next run goes on in the thread");
}

#[test]
fn max_cost_stops_the_run() {
    let b = store("maxcost");
    b.model(true, Some(100_000));
    b.add("1", "a", 1, &["INBOX"]);
    b.add("2", "b", 2, &["INBOX"]);
    let r = b
        .run(&Limits {
            max_cost_micro_usd: Some(1),
            ..Limits::default()
        })
        .unwrap();
    assert_eq!((r.stop.as_deref(), r.calls), (Some("cost"), 1));
}

#[test]
fn the_newest_thread_runs_first_and_old_threads_last() {
    let b = store("order");
    b.model(false, None);
    let old = b.add("1", "old", 200, &["INBOX"]);
    let new = b.add("2", "new", 1, &["INBOX"]);
    let mid = b.add("3", "mid", 30, &["INBOX"]);
    let r = b
        .run(&Limits {
            max_messages: Some(2),
            ..Limits::default()
        })
        .unwrap();
    assert_eq!(r.messages, 2);
    assert!(b.understood(new) && b.understood(mid) && !b.understood(old));
}

#[test]
fn a_later_message_waits_for_an_older_one_that_failed() {
    let b = store("wait");
    b.model(false, None);
    b.script.push(Ok(("x".into(), usage(1, 1))));
    b.script.push(Ok(("x".into(), usage(1, 1))));
    let first = b.add("1", "a", 2, &["INBOX"]);
    let second = b.add("2", "a", 1, &["INBOX"]);
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("done"));
    assert_eq!(b.script.calls(), 2, "two tries of the first message only");
    assert!(!b.understood(first) && !b.understood(second));
}

/// A draft and a copy in a Sent folder (IMAP `UNSENT`, `SENT_COPY`) are not
/// pending, and a later message of the thread does not wait for them.
#[test]
fn drafts_and_sent_copies_are_not_pending() {
    let b = store("copies");
    b.model(false, None);
    b.add("1", "a", 4, &["DRAFT"]);
    b.add("2", "a", 3, &["UNSENT"]);
    b.add("3", "a", 2, &["INBOX", "SENT_COPY"]);
    let last = b.add("4", "a", 1, &["INBOX"]);
    // A draft and a copy with no raw content yet do not hold the thread
    // (issue #686).
    b.store
        .write(|tx| {
            tx.execute(
                "UPDATE message SET content_sha256 = NULL WHERE provider_id IN ('1', '2', '3')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(batch::estimate(&b.store, NOW).unwrap().messages, 1);
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(
        (r.stop.as_deref(), r.messages, r.calls),
        (Some("done"), 1, 1)
    );
    assert!(b.understood(last));
}

#[test]
fn the_estimate_counts_each_message_and_changes_nothing() {
    let b = store("estimate");
    b.model(true, None);
    for i in 0..10 {
        b.add(&format!("m{i}"), "a", 1, &["INBOX"]);
    }
    b.add("old", "b", 200, &["INBOX"]);
    b.add("spam", "c", 1, &["SPAM"]);
    let e = batch::estimate(&b.store, NOW).unwrap();
    assert_eq!((e.messages, e.recent), (12, 11));
    let size = raw("m0").len() as u64;
    assert_eq!(
        e.output_tokens,
        11 * batch::OUTPUT_TOKENS,
        "SPAM needs no call"
    );
    assert!(e.input_tokens >= 11 * (batch::PROMPT_TOKENS + size / batch::BYTES_PER_TOKEN - 1));
    assert_eq!(e.cost_micro_usd, Some(e.input_tokens + e.output_tokens));
    assert_eq!(e.model_version, Some(1));
    assert_eq!(
        b.col("SELECT count(*) FROM message WHERE understood_at IS NOT NULL"),
        Some(0)
    );
    assert_eq!(b.col("SELECT count(*) FROM understand_run"), Some(0));
}

#[test]
fn a_sender_rerun_clears_the_retry_state_and_old_items() {
    let b = store("rerun");
    b.model(false, None);
    let m = b.add("1", "a", 1, &["INBOX"]);
    b.run(&Limits::default()).unwrap();
    assert!(b.understood(m));
    let sender = b
        .col("SELECT id FROM sender WHERE address = 'ana@example.org'")
        .expect("a sender row");
    b.store
        .write(|tx| {
            tx.execute(
                &format!(
                    "UPDATE message SET tries = 3, retry_at = {} WHERE id = {m}",
                    NOW + DAY
                ),
                [],
            )?;
            let reset = iris_understand::store::rerun_senders(tx, &[sender], NOW)?;
            assert_eq!(reset, 1);
            Ok(())
        })
        .unwrap();
    assert!(!b.understood(m));
    assert_eq!(
        b.col(&format!(
            "SELECT tries + coalesce(retry_at, 0) FROM message WHERE id = {m}"
        )),
        Some(0)
    );
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.calls, 1, "the reset message is pending again");
    assert!(b.understood(m));
}

#[test]
fn a_rerun_of_a_visited_thread_gets_one_more_pass() {
    let b = store("pass");
    b.model(false, None);
    // The newest thread runs first: Ana is a stranger then.
    let ana = b.add_raw(
        "1",
        "x",
        1,
        "From: Ana <ana@examp1e.org>\nTo: owner@example.com\nSubject: Hi\n\nCall me today.\n",
        &["INBOX"],
    );
    // The sent message makes example.org known, so examp1e.org is a near
    // copy: Ana becomes hostile in write 1, and her mail is reset.
    b.add_raw(
        "2",
        "y",
        3,
        "From: owner@example.com\nTo: bo@example.org\nSubject: Hi\n\nSee you.\n",
        &["SENT"],
    );
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!(r.stop.as_deref(), Some("done"));
    assert_eq!(r.messages, 3, "Ana, the sent message, then Ana again");
    assert!(
        b.understood(ana),
        "the second pass understood the reset mail"
    );
}

#[test]
fn new_mail_of_a_hostile_sender_does_not_wait() {
    let b = store("hostile-wait");
    b.model(true, None);
    b.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO sender(address, name, kind, decided_by)
                 VALUES ('bad@evil.example', 'Bad', 'hostile', 'person')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    // The older message waits for the budget.
    let older = b.add("1", "a", 2, &["INBOX"]);
    // Sync saves no header row: the From address is in the content only.
    let bad = b.add_raw(
        "2",
        "a",
        1,
        "From: Bad <bad@evil.example>\nTo: owner@example.com\nSubject: Hi\n\nClick here.\n",
        &["INBOX"],
    );
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!((r.stop.as_deref(), b.script.calls()), (Some("budget"), 0));
    assert!(!b.understood(older));
    assert!(b.understood(bad), "hostile mail gets its marks for free");
}

#[test]
fn the_estimate_rounds_each_call_like_the_budget() {
    let b = store("round");
    b.model(true, None);
    b.store
        .write(|tx| {
            tx.execute(
                "UPDATE model_setting SET price_in = 250000, price_out = 250000",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    b.add("1", "a", 1, &["INBOX"]);
    b.add("2", "b", 1, &["INBOX"]);
    let e = batch::estimate(&b.store, NOW).unwrap();
    let tokens = batch::PROMPT_TOKENS
        + raw("1").len() as u64 / batch::BYTES_PER_TOKEN
        + batch::OUTPUT_TOKENS;
    let one = (tokens * 250_000).div_ceil(1_000_000);
    assert_eq!(e.cost_micro_usd, Some(2 * one));
}

#[test]
fn a_hostile_from_after_64_kib_of_header_does_not_wait() {
    let b = store("longhead");
    b.model(true, None);
    b.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO sender(address, name, kind, decided_by)
                 VALUES ('bad@evil.example', 'Bad', 'hostile', 'person')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let older = b.add("1", "a", 2, &["INBOX"]);
    let filler: String = (0..1500)
        .map(|i| format!("X-Filler-{i}: {}\n", "a".repeat(60)))
        .collect();
    assert!(filler.len() > 64 * 1024 && filler.len() < 200 * 1024);
    let raw = format!(
        "{filler}From: Bad <bad@evil.example>\nTo: owner@example.com\nSubject: Hi\n\nClick.\n"
    );
    let bad = b.add_raw("2", "a", 1, &raw, &["INBOX"]);
    b.run(&Limits::default()).unwrap();
    assert!(!b.understood(older));
    assert!(b.understood(bad));
}

#[test]
fn the_sender_refresh_runs_before_the_model_is_made() {
    let b = store("refresh-first");
    b.model(false, None);
    let ana = b.add_raw(
        "1",
        "x",
        1,
        "From: Ana <ana@examp1e.org>\nTo: owner@example.com\nSubject: Hi\n\nCall me.\n",
        &["INBOX"],
    );
    b.run(&Limits::default()).unwrap();
    assert!(b.understood(ana));
    // Evidence that bo@example.org is a person, with no run since: the
    // refresh makes example.org known, so Ana becomes hostile.
    let raw = "From: owner@example.com\nTo: bo@example.org\nSubject: Hi\n\nSee you.\n"
        .replace('\n', "\r\n");
    let sent = b.add_raw("2", "y", 3, &raw, &["SENT"]);
    b.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO sender_evidence(message_id, address, rule, content_sha256)
                 SELECT id, 'bo@example.org', 'sent', content_sha256 FROM message WHERE id = ?1",
                [sent],
            )?;
            Ok(())
        })
        .unwrap();
    let fail = |_: &ModelRow| -> Result<Box<dyn Model>, String> { Err("no model".into()) };
    let r = batch::run(&b.store, &fail, &Limits::default(), NOW);
    assert!(matches!(r, Err(Error::Connect(_))));
    assert!(!b.understood(ana), "the refresh reset the mail of Ana");
}

#[test]
fn an_idle_run_keeps_its_row_only_with_keep_idle() {
    let b = store("idle");
    b.model(false, None);
    b.add("1", "a", 1, &["INBOX"]);
    let rows = |b: &Box_| b.col("SELECT count(*) FROM understand_run");
    let quiet = Limits {
        keep_idle: false,
        ..Limits::default()
    };
    let r = b.run(&quiet).unwrap();
    assert_eq!(r.messages, 1);
    assert_eq!(rows(&b), Some(1), "a run that read a message keeps its row");
    let r = b.run(&quiet).unwrap();
    assert!(r.is_idle());
    assert_eq!(
        rows(&b),
        Some(1),
        "an idle run with keep_idle false leaves no row"
    );
    assert!(b.run(&Limits::default()).unwrap().is_idle());
    assert_eq!(rows(&b), Some(2), "the default keeps the row");
}

/// Property: the mark `waits_model` goes away on each change of an input
/// of a refused call (labels, content, gone state, hostile marks, tries,
/// the model row), by any write path, and not on a write of the same values, and stays when only the budget changes (the
/// runner spends and refunds it; an approval clears in its own write).
#[test]
fn the_waits_model_mark_clears_on_each_input_change() {
    let b = store("waits");
    b.model(true, Some(1000));
    let m = b.add("1", "a", 1, &["INBOX"]);
    // Each change runs for both causes of the mark (1 and 2).
    let mark_as = std::cell::Cell::new(1i64);
    let mark = |b: &Box_| {
        b.store
            .write(|tx| {
                tx.execute(
                    "UPDATE message SET waits_model = ?2 WHERE id = ?1",
                    [m, mark_as.get()],
                )?;
                Ok(())
            })
            .unwrap()
    };
    let waits = |b: &Box_| b.col(&format!("SELECT waits_model FROM message WHERE id = {m}"));
    let changes: &[(&str, &str, bool)] = &[
        (
            "budget down",
            "UPDATE model_setting SET budget_micro_usd = budget_micro_usd - 1",
            false,
        ),
        (
            "budget up",
            "UPDATE model_setting SET budget_micro_usd = budget_micro_usd + 10",
            false,
        ),
        (
            "budget set",
            "UPDATE model_setting SET budget_micro_usd = NULL",
            false,
        ),
        (
            "new version",
            "UPDATE model_setting SET version = version + 1",
            true,
        ),
        (
            "label removed",
            "DELETE FROM message_label WHERE message_id = :m",
            true,
        ),
        (
            "label added",
            "INSERT INTO message_label(message_id, label_id) SELECT :m, id FROM label LIMIT 1",
            true,
        ),
        // A write of the same values is no change (#154): a full resync
        // writes each listed message again.
        (
            "same content",
            "UPDATE message SET content_sha256 = content_sha256, gone_at = gone_at,
               hostile = hostile, tries = tries WHERE id = :m",
            false,
        ),
        (
            "content",
            "UPDATE message SET content_sha256 = 'new' || content_sha256 WHERE id = :m",
            true,
        ),
        (
            "tries",
            "UPDATE message SET tries = tries + 1 WHERE id = :m",
            true,
        ),
        ("gone", "UPDATE message SET gone_at = 1 WHERE id = :m", true),
        (
            "back",
            "UPDATE message SET gone_at = NULL WHERE id = :m",
            true,
        ),
        (
            "hostile",
            "UPDATE message SET hostile = CASE WHEN hostile IS '[]' THEN '[\"spoof\"]' ELSE '[]' END
               WHERE id = :m",
            true,
        ),
        (
            "unrelated",
            "UPDATE message SET size = size WHERE id = :m",
            false,
        ),
    ];
    // Both causes, one pass of the table each.
    let all = [1, 2]
        .into_iter()
        .flat_map(|v| changes.iter().map(move |c| (v, c)));
    for (v, (name, sql, clears)) in all {
        mark_as.set(v);
        mark(&b);
        let sql = sql.replace(":m", &m.to_string());
        b.store
            .write(|tx| {
                tx.execute(&sql, [])?;
                Ok(())
            })
            .unwrap();
        assert_eq!(waits(&b), Some(if *clears { 0 } else { v }), "{name} ({v})");
    }
    // A new model row (`iris model set` replaces the row) clears it too.
    mark(&b);
    b.model(false, None);
    assert_eq!(waits(&b), Some(0), "model set");
}

/// HTML mail with the text of a feed item and one link that shows
/// `bank.example.com` and goes to `target`. No DMARC result.
fn bank(target: &str) -> String {
    format!(
        "From: Bank <alerts@bank-mail.example.org>\nTo: owner@example.com\nSubject: Invoice\n\
         Date: Tue, 30 Dec 2025 08:00:00 +0000\nContent-Type: text/html\n\n\
         <p>Pay the invoice by Friday.</p><p><a href=\"{target}\">https://bank.example.com/pay</a></p>\n"
    )
}

#[test]
fn mail_understood_before_the_link_mark_gets_the_link_check_once() {
    let b = store("link-upgrade");
    b.model(false, None);
    // More messages than one page of the check, and `m` after them.
    let pages = iris_understand::store::LINK_CHECKS as usize + 5;
    let honest = bank("https://bank.example.com/pay");
    let ok = b.add_raw("ok", "t1", 1, &honest, &["INBOX"]);
    for i in 1..pages {
        b.add_raw(
            &format!("ok{i}"),
            &format!("t1-{i}"),
            1,
            &honest,
            &["INBOX"],
        );
    }
    let m = b.add_raw("m", "t2", 1, &honest, &["INBOX"]);
    // A message whose file is missing at the check.
    let lost = b.add_raw("lost", "t3", 1, &honest, &["INBOX"]);
    // A message that the model got 3 times with no valid answer.
    let stuck = b.add_raw("stuck", "t4", 1, &honest, &["INBOX"]);
    // The same answer for each message.
    for _ in 0..pages + 3 {
        b.script.push(Ok((
            r#"{"items": [{"form": "feed", "status": "new", "title": "Invoice",
          "quote": "Pay the invoice by Friday.", "confidence": 0.5}], "phishing": false}"#
                .into(),
            usage(100, 10),
        )));
    }
    b.run(&Limits::default()).unwrap();
    let items = "SELECT count(*) FROM item_source s JOIN item i ON i.id = s.item_id
                 WHERE s.message_id = MSG AND s.role = 'origin' AND i.form <> 'conversation'";
    let items_of = |b: &Box_, m: i64| b.col(&items.replace("MSG", &m.to_string())).unwrap();
    assert_eq!(items_of(&b, m), 1);
    let calls = b.script.calls();
    // The state of a store of the engine before the mark: the file of the
    // message has a deceptive link, and the message is understood.
    let evil = bank("https://evil.example.net/pay").replace('\n', "\r\n");
    let sha = b.store.put_content(evil.as_bytes()).unwrap();
    let evil2 = bank("https://evil2.example.net/pay").replace('\n', "\r\n");
    let sha2 = b.store.put_content(evil2.as_bytes()).unwrap();
    std::fs::remove_file(b.store.content_path(&sha2).unwrap()).unwrap();
    let ok_at = b.col(&format!(
        "SELECT understood_at FROM message WHERE id = {ok}"
    ));
    b.store
        .write(|tx| {
            tx.execute(
                "UPDATE message SET content_sha256 = ?2 WHERE id = ?1",
                rusqlite::params![m, sha],
            )?;
            tx.execute(
                "UPDATE message SET content_sha256 = ?2 WHERE id = ?1",
                rusqlite::params![lost, sha2],
            )?;
            tx.execute(
                "UPDATE message SET content_sha256 = ?2, understood_at = NULL, tries = 3
                 WHERE id = ?1",
                rusqlite::params![stuck, sha],
            )?;
            tx.execute_batch(
                "DROP TRIGGER link_check_insert; DROP TRIGGER link_check_update;
                 DROP TRIGGER link_check_label; DROP TRIGGER link_check_attention_add;
                 DROP TRIGGER link_check_attention_end;
                 DROP TABLE link_check;
                 -- Migrations 21 to 32 run again after 20. The triggers
                 -- of 22 replace themselves.
                 DROP TRIGGER job_session_revoke; DROP TRIGGER job_session_job_state;
                 DROP TRIGGER action_flagged_write;
                 DROP TABLE job_session; DROP TABLE send_switch;
                 DROP VIEW ledger_can_undo; DROP VIEW undo_pair;
                 DROP TRIGGER ledger_undo_action; DROP TRIGGER ledger_undone_by;
                 DROP TABLE ledger_undo; DROP TABLE reply_draft; ALTER TABLE header DROP COLUMN reply_to; ALTER TABLE header DROP COLUMN content_sha256;
                 DROP TRIGGER measure_connect; DROP TRIGGER measure_time_update;
                 DROP TRIGGER person_decision; DROP TABLE person_decision;
                 DROP TABLE use_open; DROP TABLE use_day; DROP TABLE measure_time;
                 DROP TABLE write_send; DROP TABLE comment_delivery;
                 DROP TABLE changeset_comment; DROP TABLE nudge_try;
                 DROP TRIGGER job_notes_end; DROP TABLE job_notes;
                 DROP TRIGGER ledger_stop; DROP TABLE stop_switch;
                 DROP TRIGGER removed_account_update; DROP TRIGGER removed_account_sync_log; DROP TRIGGER removed_account_sync_seen; DROP TRIGGER removed_account_sync_log_update; DROP TRIGGER removed_account_sync_seen_update;
                 ALTER TABLE account DROP COLUMN imap_host;
                 ALTER TABLE account DROP COLUMN imap_port;
                 ALTER TABLE account DROP COLUMN imap_user;
                 ALTER TABLE account DROP COLUMN categories_off_at;
                 DROP TRIGGER understand_owner_label_add; DROP TRIGGER understand_owner_label_remove;
                 DROP TRIGGER header_from_addr_lower_add; DROP TRIGGER header_from_addr_lower; DROP TRIGGER talk_due_sender_add; DROP TRIGGER talk_due_sender; DROP TRIGGER talk_due_sender_remove; DROP TRIGGER model_declined_end; DROP TRIGGER model_declined_end_remove; DROP TABLE model_declined; DROP TRIGGER IF EXISTS spam_drops_items; DROP TRIGGER IF EXISTS spam_back_drops_items; DROP TRIGGER IF EXISTS understand_reset; DROP TRIGGER IF EXISTS understand_new_content; DROP TRIGGER IF EXISTS understand_come_back; DROP TABLE talk_due; DROP TRIGGER sent_copy_keep; DROP TRIGGER sent_copy_add; DROP VIEW iris_sent_copy; DROP INDEX draft_sent_copy; DROP TRIGGER draft_or_copy_drops_items; DROP TRIGGER draft_or_copy_talk_add; DROP TRIGGER draft_or_copy_talk_remove; DROP TRIGGER draft_or_copy_back_drops_items; DROP VIEW item_droppable;
                 DROP TRIGGER changeset_withdraw_by; DROP TRIGGER changeset_withdrawn_by; ALTER TABLE changeset DROP COLUMN withdrawn_by; DROP TRIGGER matter_add_end; DROP TRIGGER matter_item_kept_add; DROP TRIGGER matter_item_kept_update; DROP TRIGGER matter_add_insert; DROP TRIGGER matter_add_update; DROP VIEW matter_add_live; DROP TABLE matter_add; DROP TRIGGER matter_insert; DROP TRIGGER matter_update; DROP TRIGGER matter_decide; DROP TRIGGER matter_item_insert; DROP TRIGGER matter_item_update; DROP TRIGGER matter_delete; ALTER TABLE matter DROP COLUMN author; DROP TRIGGER ledger_hostile_before; ALTER TABLE ledger DROP COLUMN hostile_before; DROP TRIGGER fixed_due_release; DROP TRIGGER message_release_content; ALTER TABLE message DROP COLUMN released; ALTER TABLE message DROP COLUMN fixed_rules; DROP TRIGGER sender_gate_rev; ALTER TABLE sender DROP COLUMN gate_rev; PRAGMA user_version = 19;",
            )?;
            Ok(())
        })
        .unwrap();
    let Box_ {
        _dir,
        store,
        account,
        script,
    } = b;
    drop(store);
    let b = Box_ {
        store: Store::open(&_dir.0).unwrap(),
        _dir,
        account,
        script,
    };
    assert_eq!(
        b.col("SELECT count(*) FROM link_check"),
        Some(pages as i64 + 3)
    );
    // One run, with a model that Iris cannot make: the check needs none.
    let fail = |_: &ModelRow| -> Result<Box<dyn Model>, String> { Err("no model".into()) };
    let _ = batch::run(&b.store, &fail, &Limits::default(), NOW);
    let marks: String = b
        .store
        .read(|c| {
            Ok(
                c.query_row("SELECT hostile FROM message WHERE id = ?1", [m], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(marks, r#"["link"]"#);
    let stuck_marks: String = b
        .store
        .read(|c| {
            Ok(
                c.query_row("SELECT hostile FROM message WHERE id = ?1", [stuck], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(
        stuck_marks, r#"["link"]"#,
        "failed tries do not hold the check"
    );
    assert_eq!(items_of(&b, m), 0, "quarantine keeps no item");
    assert!(b.understood(m));
    assert_eq!(b.script.calls(), calls, "no model call");
    // The other message keeps its understanding, and each row went.
    assert_eq!(
        b.col(&format!(
            "SELECT understood_at FROM message WHERE id = {ok}"
        )),
        ok_at
    );
    // The missing file keeps its row. After a repair with the same
    // bytes, the next run gives the mark.
    assert_eq!(
        b.col("SELECT max(message_id) FROM link_check WHERE (SELECT count(*) FROM link_check) = 1"),
        Some(lost)
    );
    b.store.put_content(evil2.as_bytes()).unwrap();
    let _ = batch::run(&b.store, &fail, &Limits::default(), NOW);
    let marks: String = b
        .store
        .read(|c| {
            Ok(
                c.query_row("SELECT hostile FROM message WHERE id = ?1", [lost], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(marks, r#"["link"]"#);
    assert_eq!(b.col("SELECT count(*) FROM link_check"), Some(0));
}

#[test]
fn new_mail_gets_the_link_check_when_iris_cannot_make_the_model() {
    let b = store("link-new");
    b.model(false, None);
    let m = b.add_raw(
        "m",
        "t",
        1,
        &bank("https://evil.example.net/pay"),
        &["INBOX"],
    );
    let honest = b.add_raw(
        "h",
        "t2",
        1,
        &bank("https://bank.example.com/pay"),
        &["INBOX"],
    );
    let fail = |_: &ModelRow| -> Result<Box<dyn Model>, String> { Err("no model".into()) };
    let _ = batch::run(&b.store, &fail, &Limits::default(), NOW);
    let marks: String = b
        .store
        .read(|c| {
            Ok(
                c.query_row("SELECT hostile FROM message WHERE id = ?1", [m], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(marks, r#"["link"]"#);
    assert!(b.understood(m));
    // The honest message waits for the model, with no row left.
    assert!(!b.understood(honest));
    assert_eq!(b.col("SELECT count(*) FROM link_check"), Some(0));
    // New content needs the check again.
    b.store
        .write(|tx| {
            tx.execute(
                "UPDATE message SET content_sha256 = 'x' WHERE id = ?1",
                [honest],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(b.col("SELECT count(*) FROM link_check"), Some(1));
}

#[test]
fn the_link_check_counts_toward_the_message_limit() {
    let b = store("link-limit");
    b.model(false, None);
    for i in 0..3 {
        let raw = bank(&format!("https://evil{i}.example.net/pay"));
        b.add_raw(&format!("m{i}"), &format!("t{i}"), 1, &raw, &["INBOX"]);
    }
    let marked = "SELECT count(*) FROM message WHERE hostile LIKE '%\"link\"%'";
    let limits = Limits {
        max_messages: Some(1),
        ..Limits::default()
    };
    let r = b.run(&limits).unwrap();
    assert_eq!(r.messages, 1);
    assert_eq!(b.col(marked), Some(1));
    assert_eq!(b.col("SELECT count(*) FROM link_check"), Some(2));
    b.run(&Limits::default()).unwrap();
    assert_eq!(b.col(marked), Some(3));
    assert_eq!(b.script.calls(), 0);
}

#[test]
fn new_honest_content_clears_the_link_mark_of_the_old_content() {
    let b = store("link-clear");
    b.model(false, None);
    let m = b.add_raw(
        "m",
        "t",
        1,
        &bank("https://evil.example.net/pay"),
        &["INBOX"],
    );
    b.run(&Limits::default()).unwrap();
    let marks = |b: &Box_| -> String {
        b.store
            .read(|c| {
                Ok(
                    c.query_row("SELECT hostile FROM message WHERE id = ?1", [m], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap()
    };
    assert_eq!(marks(&b), r#"["link"]"#);
    // New content, as a sync writes it: the old marks stay until a check.
    let honest = bank("https://bank.example.com/pay").replace('\n', "\r\n");
    let sha = b.store.put_content(honest.as_bytes()).unwrap();
    b.store
        .write(|tx| {
            tx.execute(
                "UPDATE message SET content_sha256 = ?2, understood_at = NULL WHERE id = ?1",
                rusqlite::params![m, sha],
            )?;
            Ok(())
        })
        .unwrap();
    let fail = |_: &ModelRow| -> Result<Box<dyn Model>, String> { Err("no model".into()) };
    let _ = batch::run(&b.store, &fail, &Limits::default(), NOW);
    assert_eq!(marks(&b), "[]");
    assert!(!b.understood(m), "the model work still waits");
}

#[test]
fn mail_that_loses_the_sent_label_gets_the_link_check() {
    let b = store("link-sent");
    b.model(false, None);
    let raw = bank("https://evil.example.net/pay");
    let m = b.add_raw("s", "t", 1, &raw, &["SENT"]);
    b.run(&Limits::default()).unwrap();
    assert_eq!(b.col("SELECT count(*) FROM link_check"), Some(0));
    // The same content again, as received mail.
    assert_eq!(b.add_raw("s", "t", 1, &raw, &["INBOX"]), m);
    assert_eq!(b.col("SELECT count(*) FROM link_check"), Some(1));
    let fail = |_: &ModelRow| -> Result<Box<dyn Model>, String> { Err("no model".into()) };
    let _ = batch::run(&b.store, &fail, &Limits::default(), NOW);
    let marks: String = b
        .store
        .read(|c| {
            Ok(
                c.query_row("SELECT hostile FROM message WHERE id = ?1", [m], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(marks, r#"["link"]"#);
}

#[test]
fn a_label_change_during_a_check_queues_the_message_again() {
    let b = store("link-race");
    let raw = bank("https://evil.example.net/pay");
    let m = b.add_raw("s", "t", 1, &raw, &["SENT"]);
    // A check reads this row while the message is your own mail.
    let read = b.col("SELECT id FROM link_check");
    // A sync removes SENT before the check ends: the row is new.
    b.add_raw("s", "t", 1, &raw, &["INBOX"]);
    let now = b.col(&format!("SELECT id FROM link_check WHERE message_id = {m}"));
    assert!(now > read, "{now:?} {read:?}");
    // The end of the old check removes only the row that it read.
    b.store
        .write(|tx| {
            tx.execute("DELETE FROM link_check WHERE id = ?1", [read])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(b.col("SELECT count(*) FROM link_check"), Some(1));
}

/// A failed store write of the counts of a call is no evidence against the
/// model (#156): the message gets no `waits_model` mark and no model wait
/// starts. The next run after a sync reads the message.
#[test]
fn a_failed_count_write_marks_no_mail_and_starts_no_wait() {
    let b = store("count-write");
    b.model(false, None);
    let m = b.add("1", "a", 1, &["INBOX"]);
    let fail = "CREATE TRIGGER fail_counts BEFORE UPDATE OF calls ON understand_run
                WHEN NEW.calls <> OLD.calls BEGIN SELECT RAISE(ABORT, 'disk full'); END";
    b.store.write(|tx| Ok(tx.execute_batch(fail)?)).unwrap();
    let after_sync = Limits {
        wait_after_error: true,
        ..Limits::default()
    };
    assert!(matches!(b.run(&after_sync), Err(Error::Store(_))));
    assert_eq!(
        b.col(&format!("SELECT waits_model FROM message WHERE id = {m}")),
        Some(0)
    );
    assert_eq!(b.col("SELECT retry_at FROM model_setting"), None);

    b.store
        .write(|tx| Ok(tx.execute_batch("DROP TRIGGER fail_counts")?))
        .unwrap();
    b.run(&after_sync).unwrap();
    assert!(b.understood(m));
    assert_eq!(b.script.calls(), 2);
}

/// `iris understand` does not wait (#155): it clears the marks of an
/// automatic wait and calls the repaired model before `retry_at`.
#[test]
fn a_manual_run_reads_mail_that_waits_for_the_model_wait() {
    let b = store("manual-wait");
    b.model(false, None);
    let waited = b.add("1", "a", 1, &["INBOX"]);
    let refused = b.add("2", "b", 1, &["INBOX"]);
    b.store
        .write(|tx| {
            tx.execute(
                "UPDATE model_setting SET retry_failures = 1, retry_at = ?1",
                [NOW + 3_600_000],
            )?;
            tx.execute("UPDATE message SET waits_model = 2 WHERE id = ?1", [waited])?;
            tx.execute(
                "UPDATE message SET waits_model = 1 WHERE id = ?1",
                [refused],
            )?;
            Ok(())
        })
        .unwrap();
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!((r.stop.as_deref(), r.calls), (Some("done"), 1));
    assert!(b.understood(waited));
    assert!(!b.understood(refused), "a refusal mark stays");
    assert_eq!(
        b.col("SELECT retry_at FROM model_setting"),
        None,
        "the call ended the wait"
    );
}

/// A sync write between a refusal and its mark (#153): the mark is only
/// set if the message is still in the state that the refusal read, so the
/// changed message is read again by the next run.
#[test]
fn a_change_during_a_refused_call_leaves_no_mark() {
    let b = store("refusal-race");
    b.model(false, None);
    let m = b.add("1", "a", 1, &["INBOX"]);
    let db = b._dir.0.join("iris.db");
    b.script.push(Err(ModelError::Budget("refused".into())));
    *b.script.during.borrow_mut() = Some(std::boxed::Box::new(move || {
        rusqlite::Connection::open(&db)
            .unwrap()
            .execute(
                "INSERT INTO message_label(message_id, label_id)
                 SELECT ?1, id FROM label WHERE provider_id = 'UNSENT'",
                [m],
            )
            .unwrap();
    }));
    b.run(&Limits::default()).unwrap();
    assert_eq!(b.script.calls(), 1);
    let marks = b.col("SELECT count(*) FROM message WHERE waits_model <> 0");
    assert_eq!(marks, Some(0), "the changed message has no mark");
}

/// A decline of the sender during a refused call (decision D29): the mark
/// of the refusal does not hold the mail of a declined stranger, so the next
/// run understands it with no model call.
#[test]
fn a_decline_during_a_refused_call_does_not_hold_the_mail() {
    let b = store("refusal-decline");
    b.model(false, None);
    let m = b.add("1", "a", 1, &["INBOX"]);
    let db = b._dir.0.join("iris.db");
    b.script.push(Err(ModelError::Budget("refused".into())));
    *b.script.during.borrow_mut() = Some(std::boxed::Box::new(move || {
        rusqlite::Connection::open(&db)
            .unwrap()
            .execute(
                "INSERT INTO sender(address, kind, decided_by, declined_at)
                 VALUES ('ana@example.org', 'stranger', 'engine', 1)
                 ON CONFLICT(address) DO UPDATE SET declined_at = 1, kind = 'stranger'",
                [],
            )
            .unwrap();
    }));
    b.run(&Limits::default()).unwrap();
    assert_eq!(b.script.calls(), 1);
    b.run(&Limits::default()).unwrap();
    assert_eq!(b.script.calls(), 1, "no second call");
    assert!(b.understood(m));
}

/// A decline and a reopen around a refused call (#934): each change of
/// the gate during the call is a changed input, also a decline and a
/// reopen that both end before the call returns. The refusal writes no
/// mark, and the next run calls the model.
#[test]
fn a_decline_and_a_reopen_around_a_refused_call_leave_no_mark() {
    for reopen_in_call in [false, true] {
        let b = store(if reopen_in_call {
            "refusal-reopen-in"
        } else {
            "refusal-reopen-after"
        });
        b.model(false, None);
        let m = b.add("1", "a", 1, &["INBOX"]);
        let db = b._dir.0.join("iris.db");
        b.script.push(Err(ModelError::Budget("refused".into())));
        *b.script.during.borrow_mut() = Some(std::boxed::Box::new(move || {
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute(
                "INSERT INTO sender(address, kind, decided_by, declined_at)
                 VALUES ('ana@example.org', 'stranger', 'engine', 1)
                 ON CONFLICT(address) DO UPDATE SET declined_at = 1, kind = 'stranger'",
                [],
            )
            .unwrap();
            if reopen_in_call {
                c.execute("UPDATE sender SET declined_at = NULL", [])
                    .unwrap();
            }
        }));
        b.run(&Limits::default()).unwrap();
        assert_eq!(b.script.calls(), 1);
        assert_eq!(
            b.col(&format!("SELECT waits_model FROM message WHERE id = {m}")),
            Some(0),
            "the gate changed during the call (reopen in call: {reopen_in_call})"
        );
        b.store
            .write(|tx| {
                tx.execute("UPDATE sender SET declined_at = NULL", [])?;
                Ok(())
            })
            .unwrap();
        b.run(&Limits::default()).unwrap();
        assert_eq!(b.script.calls(), 2, "after the reopen the model reads it");
    }
}

/// A kind change and its undo during a refused call, for a sender with a
/// decline (#934): the decline applied for a moment, so the gate changed.
/// The refusal writes no mark.
#[test]
fn a_kind_change_and_its_undo_around_a_refused_call_leave_no_mark() {
    let b = store("refusal-kind-flip");
    b.model(false, None);
    let m = b.add("1", "a", 1, &["INBOX"]);
    let db = b._dir.0.join("iris.db");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute(
            "INSERT INTO sender(address, kind, decided_by, declined_at)
             VALUES ('ana@example.org', 'person', 'person', 1)
             ON CONFLICT(address) DO UPDATE SET declined_at = 1, kind = 'person',
               decided_by = 'person'",
            [],
        )
        .unwrap();
    b.script.push(Err(ModelError::Budget("refused".into())));
    *b.script.during.borrow_mut() = Some(std::boxed::Box::new(move || {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute("UPDATE sender SET kind = 'stranger'", [])
            .unwrap();
        c.execute("UPDATE sender SET kind = 'person'", []).unwrap();
    }));
    b.run(&Limits::default()).unwrap();
    assert_eq!(b.script.calls(), 1);
    assert_eq!(
        b.col(&format!("SELECT waits_model FROM message WHERE id = {m}")),
        Some(0)
    );
}

/// A change of the thread context during a refused call (a SPAM label on
/// an earlier message) leaves no mark either (#153).
#[test]
fn a_context_change_during_a_refused_call_leaves_no_mark() {
    let b = store("refusal-context");
    b.model(false, None);
    let earlier = b.add("0", "a", 2, &["INBOX"]);
    b.run(&Limits::default()).unwrap();
    assert!(b.understood(earlier));
    let m = b.add("1", "a", 1, &["INBOX"]);
    let db = b._dir.0.join("iris.db");
    b.script.push(Err(ModelError::Budget("refused".into())));
    *b.script.during.borrow_mut() = Some(std::boxed::Box::new(move || {
        rusqlite::Connection::open(&db)
            .unwrap()
            .execute(
                "INSERT INTO message_label(message_id, label_id)
                 SELECT ?1, id FROM label WHERE provider_id = 'SPAM'",
                [earlier],
            )
            .unwrap();
    }));
    b.run(&Limits::default()).unwrap();
    assert_eq!(
        b.col(&format!("SELECT waits_model FROM message WHERE id = {m}")),
        Some(0)
    );
}

/// A sender rerun resets the mail of the sender for a new understanding,
/// also when its retry fields have the reset values already: the mark goes.
#[test]
fn a_sender_rerun_clears_the_mark() {
    let b = store("rerun-mark");
    b.model(true, None);
    let m = b.add("1", "a", 1, &["INBOX"]);
    b.run(&Limits::default()).unwrap();
    assert_eq!(
        b.col(&format!("SELECT waits_model FROM message WHERE id = {m}")),
        Some(1),
        "no budget: a refusal"
    );
    b.store
        .write(|tx| {
            let sender: i64 = tx.query_row(
                "SELECT s.id FROM sender s JOIN header h ON h.from_addr = s.address
                 WHERE h.message_id = ?1",
                [m],
                |r| r.get(0),
            )?;
            iris_understand::store::rerun_senders(tx, &[sender], NOW)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        b.col(&format!("SELECT waits_model FROM message WHERE id = {m}")),
        Some(0)
    );
}

/// Decision D29: the mail of a declined stranger makes no model call, so it
/// waits for nothing that only the model needs: a budget refusal, the try
/// limit, or an older message of the thread that waits for the model.
#[test]
fn the_mail_of_a_declined_stranger_does_not_wait_for_the_model() {
    let b = store("declined-wait");
    b.model(true, None);
    let far = |n: &str| {
        format!("From: Far <x@far.example>\nTo: owner@example.com\nSubject: Offer {n}\n\nCan you send the price list?\n")
    };
    // A message of the stranger that a budget refusal marked, and one at the
    // try limit, before the decline.
    let refused = b.add_raw("1", "b", 3, &far("1"), &["INBOX"]);
    let limit = b.add_raw("2", "c", 3, &far("2"), &["INBOX"]);
    let r = b.run(&Limits::default()).unwrap();
    assert_eq!((r.stop.as_deref(), b.script.calls()), (Some("budget"), 0));
    assert!(!b.understood(refused) && !b.understood(limit));
    b.store
        .write(|tx| {
            tx.execute(
                "UPDATE message SET waits_model = 1 WHERE id = ?1 AND waits_model = 0",
                [refused],
            )?;
            tx.execute("UPDATE message SET tries = 99 WHERE id = ?1", [limit])?;
            Ok(())
        })
        .unwrap();
    // New mail of the stranger behind an older message that waits.
    let older = b.add("3", "a", 2, &["INBOX"]);
    let behind = b.add_raw("4", "a", 1, &far("4"), &["INBOX"]);
    b.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO sender(address, kind, decided_by, declined_at)
                 VALUES ('x@far.example', 'stranger', 'engine', 1)
                 ON CONFLICT(address) DO UPDATE SET declined_at = 1",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    // A model wait written after the decline (a store from before this
    // rule, or a refusal that raced the decline) holds nothing either.
    b.store
        .write(|tx| {
            tx.execute(
                "UPDATE message SET waits_model = 1, retry_at = ?2 WHERE id = ?1",
                rusqlite::params![limit, NOW + 86_400_000],
            )?;
            Ok(())
        })
        .unwrap();
    // The estimate counts no call for the mail of a declined stranger with
    // a header. Mail with no header yet (`behind`) counts as a call: the
    // estimate is an upper bound.
    let e = batch::estimate(&b.store, NOW).unwrap();
    assert_eq!(
        e.output_tokens,
        2 * batch::OUTPUT_TOKENS,
        "older and behind"
    );
    b.run(&Limits::default()).unwrap();
    assert_eq!(b.script.calls(), 0);
    assert!(!b.understood(older), "the older message still waits");
    for m in [refused, limit, behind] {
        assert!(b.understood(m), "message {m}");
    }
}

/// Decision D31: a kind or name change that the sender refresh of a run
/// makes gets its fixed check before the run ends. Here the refresh makes
/// Bo Smith a known person, and the mail of another address under the same
/// name gets the spoof mark in the same run.
#[test]
fn a_name_change_of_the_refresh_gets_the_fixed_check_in_the_same_run() {
    let b = store("refresh-fixed");
    b.model(false, None);
    let bo = "From: Bo Smith <bo@example.org>\nTo: owner@example.com\nSubject: Hi\n\nCall me.\n";
    b.add_raw("1", "x", 1, bo, &["INBOX"]);
    let other =
        "From: Bo Smith <bo@elsewhere.example>\nTo: owner@example.com\nSubject: Pay\n\nPay now.\n";
    let spoof = b.add_raw("2", "y", 2, other, &["INBOX"]);
    b.run(&Limits::default()).unwrap();
    let marks = |b: &Box_| -> String {
        b.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT coalesce(hostile, '[]') FROM message WHERE id = ?1",
                    [spoof],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    };
    assert_eq!(marks(&b), "[]", "no known name yet");
    // Evidence that bo@example.org is a person, with no run since.
    let raw = "From: owner@example.com\nTo: bo@example.org\nSubject: Re: Hi\n\nSure.\n"
        .replace('\n', "\r\n");
    let sent = b.add_raw("3", "x", 3, &raw, &["SENT"]);
    b.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO sender_evidence(message_id, address, rule, content_sha256)
                 SELECT id, 'bo@example.org', 'sent', content_sha256 FROM message WHERE id = ?1",
                [sent],
            )?;
            Ok(())
        })
        .unwrap();
    b.run(&Limits::default()).unwrap();
    assert!(
        marks(&b).contains("spoof"),
        "the run made the spoof mark: {:?}",
        marks(&b)
    );
    assert_eq!(b.col("SELECT count(*) FROM fixed_due"), Some(0));
}

fn hostile_of(b: &Box_, m: i64) -> String {
    b.store
        .read(|c| {
            Ok(c.query_row(
                "SELECT coalesce(hostile, '[]') FROM message WHERE id = ?1",
                [m],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

/// Decision D31: the fixed check runs before the model is made, so a model
/// that cannot be made does not hold back a new spoof mark.
#[test]
fn the_fixed_check_runs_before_the_model_is_made() {
    let b = store("fixed-first");
    b.model(false, None);
    let other =
        "From: Bo Smith <bo@elsewhere.example>\nTo: owner@example.com\nSubject: Pay\n\nPay now.\n";
    let spoof = b.add_raw("1", "y", 2, other, &["INBOX"]);
    b.run(&Limits::default()).unwrap();
    assert_eq!(hostile_of(&b, spoof), "[]");
    // A known person of the same name, set by the person.
    b.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO sender(address, kind, name, decided_by)
                 VALUES ('bo@example.org', 'person', 'Bo Smith', 'person')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let fail = |_: &ModelRow| -> Result<Box<dyn Model>, String> { Err("no model".into()) };
    let r = batch::run(&b.store, &fail, &Limits::default(), NOW);
    assert!(matches!(r, Err(Error::Connect(_))));
    assert!(
        hostile_of(&b, spoof).contains("spoof"),
        "{}",
        hostile_of(&b, spoof)
    );
}

/// Decision D31: a name change of the main pass (here a sent message that
/// names a known person under a new name) clears the spoof mark of mail
/// under the old name. That mail waits for the model, and the same run
/// reads it.
#[test]
fn mail_that_the_fixed_check_of_the_main_pass_resets_gets_the_second_pass() {
    let b = store("fixed-pass");
    b.model(false, None);
    let to = |n: &str| {
        format!("From: owner@example.com\nTo: {n} <ana@example.org>\nSubject: Hi\n\nSee you.\n")
    };
    b.add_raw("1", "x", 10, &to("Ana Lee"), &["SENT"]);
    let other =
        "From: Ana Lee <ana@elsewhere.example>\nTo: owner@example.com\nSubject: Pay\n\nPay now.\n";
    let spoof = b.add_raw("2", "y", 5, other, &["INBOX"]);
    b.run(&Limits::default()).unwrap();
    assert!(
        hostile_of(&b, spoof).contains("spoof"),
        "{}",
        hostile_of(&b, spoof)
    );
    // A sent message renames Ana in the main pass of the next run.
    let m = b.add_raw("3", "x", 1, &to("Ana Bo"), &["SENT"]);
    b.run(&Limits::default()).unwrap();
    assert!(b.understood(m));
    let name = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT name FROM sender WHERE address = 'ana@example.org'",
                [],
                |r| r.get::<_, Option<String>>(0),
            )?)
        })
        .unwrap();
    assert_eq!(
        name.as_deref(),
        Some("Ana Bo"),
        "the sent message renamed Ana"
    );
    assert_eq!(hostile_of(&b, spoof), "[]");
    assert!(b.understood(spoof), "the same run read the reset mail");
}

/// Issues 369, 373 and 374: the person releases a message that the mark
/// `link` holds in quarantine. The fixed check reads it again without the
/// mark, the model reads it as new mail, and its items show. The end of the
/// release gives the mark back, and the items go. New content ends the
/// release. A mark that the person cannot release (`phishing`) stays.
#[test]
fn a_released_message_leaves_quarantine_and_the_undo_brings_it_back() {
    use iris_understand::store::{release, unrelease, Release};
    let b = store("release");
    b.model(false, None);
    let m = b.add_raw(
        "m",
        "t",
        1,
        &bank("https://evil.example.net/pay"),
        &["INBOX"],
    );
    b.run(&Limits::default()).unwrap();
    let hostile = |b: &Box_| {
        b.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT coalesce(hostile, '[]') FROM message WHERE id = ?1",
                    [m],
                    |r| r.get::<_, String>(0),
                )?)
            })
            .unwrap()
    };
    let items = |b: &Box_| {
        b.col(&format!(
            "SELECT count(*) FROM item_source s JOIN item i ON i.id = s.item_id
             WHERE s.message_id = {m} AND s.role = 'origin' AND i.form <> 'conversation'"
        ))
        .unwrap()
    };
    assert_eq!(hostile(&b), r#"["link"]"#);
    assert_eq!((b.script.calls(), items(&b)), (0, 0));

    // The release: the next run reads the mail with the model, also when
    // the model used each try of the message before the mark held it.
    b.store
        .write(|tx| {
            tx.execute("UPDATE message SET tries = 3 WHERE id = ?1", [m])?;
            Ok(())
        })
        .unwrap();
    // A link check waits at the time of the release: it cannot take the
    // place of the understanding.
    b.store
        .write(|tx| {
            tx.execute("INSERT INTO link_check(message_id) VALUES (?1)", [m])?;
            Ok(())
        })
        .unwrap();
    let r = b.store.write(|tx| release(tx, m)).unwrap();
    assert_eq!(
        r,
        Release::Done {
            released: vec!["link".into()],
            kept: vec![]
        }
    );
    // A second release before the run: Iris reads the mail again now.
    assert_eq!(
        b.store.write(|tx| release(tx, m)).unwrap(),
        Release::Reading
    );
    let feed = || {
        Ok((
            r#"{"items": [{"form": "feed", "status": "new", "title": "Invoice",
              "quote": "Pay the invoice by Friday.", "confidence": 0.5}], "phishing": false}"#
                .to_string(),
            usage(100, 10),
        ))
    };
    b.script.push(feed());
    b.run(&Limits::default()).unwrap();
    assert_eq!(hostile(&b), "[]");
    assert_eq!((b.script.calls(), items(&b)), (1, 1));
    // The released message is in the conversation of its thread.
    let talk = |b: &Box_| {
        b.col(&format!(
            "SELECT count(*) FROM item_source s JOIN item i ON i.id = s.item_id
             WHERE s.message_id = {m} AND i.form = 'conversation' AND i.status <> 'gone'"
        ))
        .unwrap()
    };
    assert!(talk(&b) > 0);
    // The link check of the same content keeps the release.
    iris_understand::store::recheck_links(&b.store, NOW, None).unwrap();
    assert_eq!(hostile(&b), "[]");

    // A link check reads the release, and the undo comes before its write.
    b.store
        .write(|tx| {
            tx.execute("INSERT INTO link_check(message_id) VALUES (?1)", [m])?;
            Ok(())
        })
        .unwrap();
    let read = iris_understand::store::link_read(&b.store, m).unwrap();
    assert!(!read.mark);

    // The end of the release: the mark comes back with no model call.
    assert!(matches!(
        b.store.write(|tx| unrelease(tx, m)).unwrap(),
        Release::Done { .. }
    ));
    assert_eq!(
        b.store.write(|tx| unrelease(tx, m)).unwrap(),
        Release::Nothing
    );
    // The undo holds the mail at once, before the fixed check runs: each
    // gate reads `message.hostile`.
    assert_eq!(hostile(&b), r#"["link"]"#);
    // The write of that link check keeps its queue row and the mark.
    let row = b
        .col(&format!("SELECT id FROM link_check WHERE message_id = {m}"))
        .unwrap();
    assert!(!b
        .store
        .write(|tx| iris_understand::store::link_check_done(tx, row, m, &read))
        .unwrap());
    assert_eq!(hostile(&b), r#"["link"]"#);
    assert_eq!(
        b.col(&format!(
            "SELECT count(*) FROM link_check WHERE message_id = {m}"
        )),
        Some(1)
    );
    assert_eq!(
        b.col(&format!(
            "SELECT count(*) FROM message m WHERE m.id = {m} AND {}",
            iris_store::may_be_hostile!()
        )),
        Some(1)
    );
    b.run(&Limits::default()).unwrap();
    assert_eq!(hostile(&b), r#"["link"]"#);
    assert_eq!((b.script.calls(), items(&b)), (1, 0));
    // The undo takes the message out of the conversation (section 7.3).
    assert_eq!(talk(&b), 0);

    // New content ends a release.
    b.store.write(|tx| release(tx, m)).unwrap();
    let other = bank("https://evil2.example.net/pay").replace('\n', "\r\n");
    let sha = b.store.put_content(other.as_bytes()).unwrap();
    b.store
        .write(|tx| {
            tx.execute(
                // As the sync does: new content clears `understood_at`.
                "UPDATE message SET content_sha256 = ?2, understood_at = NULL WHERE id = ?1",
                rusqlite::params![m, sha],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        b.col(&format!(
            "SELECT released IS NULL FROM message WHERE id = {m}"
        )),
        Some(1)
    );
    // The old marks stay until Iris reads the new content: no release of
    // them, so the new content gets its own marks.
    assert_eq!(
        b.store.write(|tx| release(tx, m)).unwrap(),
        Release::Reading
    );
    b.run(&Limits::default()).unwrap();
    assert_eq!(hostile(&b), r#"["link"]"#);
    assert_eq!(b.script.calls(), 1);

    // Only `phishing`: nothing to release. With `link` too, `phishing` stays.
    b.store
        .write(|tx| {
            tx.execute(
                r#"UPDATE message SET hostile = '["phishing"]' WHERE id = ?1"#,
                [m],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        b.store.write(|tx| release(tx, m)).unwrap(),
        Release::Nothing
    );
    b.store
        .write(|tx| {
            tx.execute(
                r#"UPDATE message SET hostile = '["link","phishing"]' WHERE id = ?1"#,
                [m],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        b.store.write(|tx| release(tx, m)).unwrap(),
        Release::Done {
            released: vec!["link".into()],
            kept: vec!["phishing".into()]
        }
    );
    b.run(&Limits::default()).unwrap();
    assert_eq!(hostile(&b), r#"["phishing"]"#);
    assert_eq!((b.script.calls(), items(&b)), (1, 0));
}
