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
/// of a refused call (labels, content, gone state, hostile marks, the model
/// row), by any write path, and stays when only the budget changes (the
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
        (
            "content",
            "UPDATE message SET content_sha256 = content_sha256 WHERE id = :m",
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
            "UPDATE message SET hostile = '[]' WHERE id = :m",
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
                 -- Migrations 21 to 27 run again after 20. The triggers
                 -- of 22 replace themselves.
                 DROP TRIGGER job_session_revoke; DROP TRIGGER job_session_job_state;
                 DROP TRIGGER action_flagged_write;
                 DROP TABLE job_session; DROP TABLE send_switch;
                 DROP VIEW ledger_can_undo; DROP VIEW undo_pair;
                 DROP TRIGGER ledger_undo_action; DROP TRIGGER ledger_undone_by;
                 DROP TABLE ledger_undo; DROP TABLE reply_draft;
                 DROP TRIGGER measure_connect; DROP TRIGGER measure_time_update;
                 DROP TRIGGER person_decision; DROP TABLE person_decision;
                 DROP TABLE use_open; DROP TABLE use_day; DROP TABLE measure_time;
                 PRAGMA user_version = 19;",
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
