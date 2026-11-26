//! The reservation of docs/specs/2026-11-16-cost-control.md: the `Budgeted`
//! model with a fake model. No key, no network.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;

use iris_model::{Error as ModelError, Reply, Stop, Usage};
use iris_store::Store;
use iris_understand::budget::{model_row, Budgeted, NoModel, State};
use iris_understand::Model;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-budget-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn usage(input: u64, output: u64) -> Usage {
    Usage {
        input_tokens: input,
        output_tokens: output,
        cost_micro_usd: None,
        known: true,
    }
}

/// Gives the queued answers in order and counts the calls. The body is
/// always 1000 bytes.
#[derive(Default)]
struct Fake {
    answers: RefCell<VecDeque<Result<Usage, ModelError>>>,
    calls: RefCell<u32>,
    /// A data folder whose run table the call drops, so the settle write
    /// after the call fails.
    drop_runs: RefCell<Option<PathBuf>>,
}

impl Fake {
    fn with(answers: Vec<Result<Usage, ModelError>>) -> Fake {
        Fake {
            answers: RefCell::new(answers.into()),
            ..Fake::default()
        }
    }
}

impl Model for Fake {
    fn name(&self) -> &str {
        "fake-1"
    }

    fn request_bytes(&self, _: &str, _: &str) -> u64 {
        1000
    }

    fn ask(&self, _: &str, _: &str) -> Result<Reply, ModelError> {
        *self.calls.borrow_mut() += 1;
        if let Some(dir) = self.drop_runs.borrow().as_ref() {
            rusqlite::Connection::open(dir.join("iris.db"))
                .unwrap()
                .execute_batch("DROP TABLE understand_run")
                .unwrap();
        }
        let usage = self
            .answers
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(usage(100, 10)))?;
        Ok(Reply {
            stop: Stop::End,
            text: "{}".into(),
            usage,
        })
    }
}

// The worst case of one call: 1000 bytes + 100 max tokens, 1 micro-USD each.
const WORST: i64 = 1100;
// The cost of the default usage (100 in, 10 out).
const COST: i64 = 110;

struct Run {
    _dir: TempDir,
    store: Store,
    run: i64,
}

/// A store with a model row (priced: 1 micro-USD a token; or local) and a
/// run row.
fn setup(name: &str, priced: bool, budget: Option<i64>) -> Run {
    let dir = TempDir::new(name);
    let store = Store::open(&dir.0).unwrap();
    let run = store
        .write(|tx| {
            tx.execute(
                "INSERT INTO model_setting(id, provider, base_url, model, version, price_in,
                   price_out, max_tokens, timeout_s, budget_micro_usd)
                 VALUES (1, ?1, 'http://127.0.0.1:1/v1', 'fake-1', 1, ?2, ?2, 100, 30, ?3)",
                rusqlite::params![
                    if priced { "anthropic" } else { "local" },
                    priced.then_some(1_000_000),
                    budget
                ],
            )?;
            tx.execute(
                "INSERT INTO understand_run(started_at, model, prompt, cost_micro_usd,
                   unknown_cost_micro_usd) VALUES (0, 'fake-1', 'p', ?1, ?1)",
                [priced.then_some(0)],
            )?;
            Ok(tx.last_insert_rowid())
        })
        .unwrap();
    Run {
        _dir: dir,
        store,
        run,
    }
}

impl Run {
    fn model<'a>(&'a self, fake: &'a dyn Model) -> Budgeted<'a> {
        let row = self.store.read(|c| Ok(model_row(c)?)).unwrap();
        Budgeted::new(fake, &self.store, self.run, row.as_ref())
    }

    fn budget(&self) -> Option<i64> {
        self.one("SELECT budget_micro_usd FROM model_setting")
    }

    fn one(&self, sql: &str) -> Option<i64> {
        self.store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    /// calls, input, output, cost, unknown calls, unknown cost, reserved.
    fn counts(&self) -> [Option<i64>; 7] {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT calls, input_tokens, output_tokens, cost_micro_usd, unknown_calls,
                            unknown_cost_micro_usd, reserved_micro_usd FROM understand_run",
                    [],
                    |r| {
                        Ok([
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                            r.get(6)?,
                        ])
                    },
                )?)
            })
            .unwrap()
    }
}

fn is_budget(r: Result<Reply, ModelError>) -> bool {
    matches!(r, Err(ModelError::Budget(_)))
}

#[test]
fn no_budget_refuses_each_paid_call() {
    let r = setup("null", true, None);
    let fake = Fake::default();
    let m = r.model(&fake);
    assert!(is_budget(m.ask("i", "d")));
    assert_eq!(*fake.calls.borrow(), 0, "no request goes out");
    assert!(m.state().refused);
    assert_eq!(r.budget(), None);
}

#[test]
fn each_call_reserves_its_worst_case_and_settles_its_cost() {
    let r = setup("settle", true, Some(WORST + 100));
    let fake = Fake::default();
    let m = r.model(&fake);
    assert!(m.ask("i", "d").is_ok());
    assert_eq!(r.budget(), Some(WORST + 100 - COST));
    let c = r.counts();
    assert_eq!(
        c,
        [
            Some(1),
            Some(100),
            Some(10),
            Some(COST),
            Some(0),
            Some(0),
            Some(0)
        ]
    );
    // The budget left (1090) cannot hold a second worst case (1100).
    assert!(is_budget(m.ask("i", "d")));
    assert_eq!(*fake.calls.borrow(), 1);
    assert_eq!(
        m.state(),
        State {
            refused: true,
            sent: true,
            ..State::default()
        }
    );
}

#[test]
fn the_budget_is_one_for_all_runs() {
    let r = setup("shared", true, Some(WORST + 100));
    let fake = Fake::default();
    assert!(r.model(&fake).ask("i", "d").is_ok());
    // A second wrapper (a second run) takes from the same budget.
    assert!(is_budget(r.model(&fake).ask("i", "d")));
    assert_eq!(*fake.calls.borrow(), 1);
}

#[test]
fn unknown_usage_keeps_the_worst_case_and_refuses_later_calls() {
    for answer in [
        Ok(Usage::default()),
        Err(ModelError::Transport("time-out".into())),
        Err(ModelError::Server(529)),
        Err(ModelError::RateLimited { retry_after: None }),
        Err(ModelError::BadAnswer("no usage".into())),
        Err(ModelError::Rejected {
            status: 400,
            message: "bad".into(),
        }),
    ] {
        let r = setup("unknown", true, Some(10_000));
        let fake = Fake::with(vec![answer]);
        let m = r.model(&fake);
        let _ = m.ask("i", "d");
        assert!(m.state().unknown);
        assert_eq!(r.budget(), Some(10_000 - WORST));
        let c = r.counts();
        assert_eq!(
            (c[0], c[4], c[5], c[6]),
            (Some(0), Some(1), Some(WORST), Some(0))
        );
        assert!(is_budget(m.ask("i", "d")), "the next call is refused");
        assert_eq!(*fake.calls.borrow(), 1);
    }
}

#[test]
fn a_refused_key_gives_the_whole_reservation_back() {
    for status in [401, 403] {
        let r = setup("key", true, Some(10_000));
        let fake = Fake::with(vec![Err(ModelError::Rejected {
            status,
            message: "bad key".into(),
        })]);
        let m = r.model(&fake);
        assert!(m.ask("i", "d").is_err());
        assert!(m.state().key_refused);
        assert_eq!(r.budget(), Some(10_000));
        let c = r.counts();
        assert_eq!(
            (c[0], c[1], c[4], c[6]),
            (Some(1), Some(0), Some(0), Some(0))
        );
    }
}

#[test]
fn a_cost_over_the_reservation_takes_the_real_cost() {
    let r = setup("overrun", true, Some(2 * WORST));
    let fake = Fake::with(vec![Ok(usage(5_000, 10))]);
    let m = r.model(&fake);
    assert!(m.ask("i", "d").is_ok());
    assert!(m.state().overrun);
    assert_eq!(r.budget(), Some(2 * WORST - 5_010));
    assert_eq!(r.counts()[3], Some(5_010));
    // A second try of the same message cannot go out after the overrun.
    assert!(is_budget(m.ask("i", "d")));
    assert_eq!(*fake.calls.borrow(), 1);
}

#[test]
fn a_hosted_model_must_have_a_price() {
    let r = setup("noprice", true, None);
    for sql in [
        "UPDATE model_setting SET price_in = NULL, price_out = NULL",
        "UPDATE model_setting SET price_out = NULL",
        "UPDATE model_setting SET provider = 'local'",
    ] {
        let e = r.store.write(|tx| Ok(tx.execute(sql, [])?));
        assert!(e.is_err(), "{sql}");
    }
    let fake = Fake::default();
    assert!(is_budget(r.model(&fake).ask("i", "d")));
    assert_eq!(*fake.calls.borrow(), 0);
}

#[test]
fn a_local_model_needs_no_budget() {
    let r = setup("local", false, None);
    let fake = Fake::with(vec![Ok(usage(7, 3)), Ok(Usage::default())]);
    let m = r.model(&fake);
    assert!(m.ask("i", "d").is_ok());
    // No usage: the tokens are unknown, but no charge is possible.
    assert!(m.ask("i", "d").is_ok());
    assert!(m.ask("i", "d").is_ok());
    assert_eq!(
        m.state(),
        State {
            sent: true,
            ..State::default()
        }
    );
    let c = r.counts();
    assert_eq!(
        c,
        [Some(2), Some(107), Some(13), None, Some(1), None, Some(0)]
    );
}

#[test]
fn a_local_server_error_is_marked() {
    let r = setup("localerr", false, None);
    let fake = Fake::with(vec![Err(ModelError::Transport("refused".into()))]);
    let m = r.model(&fake);
    assert!(m.ask("i", "d").is_err());
    assert!(m.state().server_failed && !m.state().unknown);
}

#[test]
fn no_model_refuses_each_call() {
    let r = setup("nomodel", false, None);
    let m = Budgeted::new(&NoModel, &r.store, r.run, None);
    assert!(is_budget(m.ask("i", "d")));
    assert!(m.state().refused);
}

#[test]
fn a_failed_settle_write_drops_the_reply_and_refuses_later_calls() {
    for priced in [true, false] {
        let r = setup("settlefail", priced, priced.then_some(10_000));
        let fake = Fake::default();
        *fake.drop_runs.borrow_mut() = Some(r._dir.0.clone());
        let m = r.model(&fake);
        assert!(is_budget(m.ask("i", "d")), "a reply with no saved counts");
        assert!(m.state().store_failed);
        assert!(m.take_error().is_some());
        *fake.drop_runs.borrow_mut() = None;
        assert!(is_budget(m.ask("i", "d")), "no call after the failure");
        assert!(m.state().store_failed);
        assert_eq!(*fake.calls.borrow(), 1);
    }
}
