//! The batch runner of `docs/specs/2026-11-16-cost-control.md`: it runs
//! [`crate::store::understand`] on the pending messages of the store, thread
//! by thread, with one run lock, one budget for all runs and the counts of
//! each run. [`estimate`] gives the size of the work and changes nothing.

use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};

use iris_model::{cost, Usage};
use iris_store::Store;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::budget::{i64_of, model_row, Budgeted, ModelRow, NoModel};

use crate::prompt::{MAX_DATA, PROMPT};
use crate::store::{self, rerun_senders, MAX_TRIES};
use crate::Model;

/// The run lock file in the data folder.
pub const LOCK_FILE: &str = "understand.lock";
/// Messages between two sender refreshes (step 5 of the runner).
pub const BATCH: usize = 50;
/// The age limit of the first group of threads.
pub const RECENT_MS: i64 = 90 * 86_400_000;
/// Estimate constants. shortcut: not measured yet (spec "Assumptions"),
/// measure them with the eval set; a wrong value moves only the estimate.
pub const BYTES_PER_TOKEN: u64 = 4;
pub const PROMPT_TOKENS: u64 = 1_500;
pub const OUTPUT_TOKENS: u64 = 400;

/// Threads in one read of the thread order.
const PAGE: i64 = 500;
/// The sort value of a message with no `internal_date`: the oldest.
const NO_DATE: i64 = -9_223_372_036_854_775_807;

/// The pending conditions other than the thread rule, on message `m`.
const BASE: &str = "m.gone_at IS NULL AND m.understood_at IS NULL
  AND m.content_sha256 IS NOT NULL
  AND m.tries < :max_tries AND (m.retry_at IS NULL OR m.retry_at <= :now)
  AND NOT EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                  WHERE ml.message_id = m.id AND l.provider_id = 'DRAFT')
  AND NOT EXISTS (SELECT 1 FROM message o WHERE o.thread_id = m.thread_id
                  AND o.gone_at IS NULL AND o.content_sha256 IS NULL)";

/// The thread rule: no older message of the thread waits. Hostile mail
/// (label `SPAM`, or a `hostile` sender) does not wait.
const THREAD_RULE: &str = "(EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
          WHERE ml.message_id = m.id AND l.provider_id = 'SPAM')
   OR EXISTS (SELECT 1 FROM header h JOIN sender s ON s.address = h.from_addr
          WHERE h.message_id = m.id AND s.kind = 'hostile')
   OR NOT EXISTS (SELECT 1 FROM message o WHERE o.thread_id = m.thread_id AND o.id <> m.id
          AND o.gone_at IS NULL AND o.understood_at IS NULL AND o.tries < :max_tries
          AND (coalesce(o.internal_date, :no_date), o.id) < (coalesce(m.internal_date, :no_date), m.id)
          AND NOT EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                          WHERE ml.message_id = o.id AND l.provider_id = 'DRAFT')))";

#[derive(Debug)]
pub enum Error {
    /// Another runner, or `iris model set`, holds the run lock. No call.
    Busy,
    Store(iris_store::Error),
    Understand(store::Error),
    /// The program could not make the model from the row.
    Connect(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Busy => write!(f, "a run is in progress"),
            Error::Store(e) => write!(f, "store: {e}"),
            Error::Understand(e) => write!(f, "{e}"),
            Error::Connect(e) => write!(f, "model: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<iris_store::Error> for Error {
    fn from(e: iris_store::Error) -> Self {
        Error::Store(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Store(e.into())
    }
}

/// The run lock: an OS file lock on [`LOCK_FILE`]. While a value exists, no
/// other runner can start, in this process or in another one. The OS
/// releases it when the process ends, also after a crash.
pub struct RunLock(#[allow(dead_code)] File);

/// Takes the run lock, or gives [`Error::Busy`].
pub fn lock(store: &Store) -> Result<RunLock, Error> {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(store.dir().join(LOCK_FILE))
        .map_err(|e| Error::Store(e.into()))?;
    match f.try_lock() {
        Ok(()) => Ok(RunLock(f)),
        Err(TryLockError::WouldBlock) => Err(Error::Busy),
        Err(TryLockError::Error(e)) => Err(Error::Store(e.into())),
    }
}

/// The limits of one run.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Messages between two sender refreshes.
    pub batch: usize,
    pub max_messages: Option<u64>,
    /// A run can pass this limit by the cost of one message.
    pub max_cost_micro_usd: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            batch: BATCH,
            max_messages: None,
            max_cost_micro_usd: None,
        }
    }
}

/// Why a run ended (`understand_run.stop`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// No pending message is left that the run could reach.
    Done,
    /// The budget refused a call, or a call cost more than its reservation.
    Budget,
    /// No model is set: the free steps ran, each model call was refused.
    NoModel,
    /// `Limits::max_cost_micro_usd`.
    Cost,
    /// `Limits::max_messages`.
    Messages,
    /// A refused key, or an error of a local server.
    Model,
    /// A call to a priced model with an unknown result or unknown usage.
    Unknown,
    Error,
}

impl Stop {
    pub fn as_str(self) -> &'static str {
        match self {
            Stop::Done => "done",
            Stop::Budget => "budget",
            Stop::NoModel => "no_model",
            Stop::Cost => "cost",
            Stop::Messages => "messages",
            Stop::Model => "model",
            Stop::Unknown => "unknown",
            Stop::Error => "error",
        }
    }
}

/// The `understand_run` row of a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub run: i64,
    pub stop: Option<String>,
    pub messages: i64,
    pub calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// `None` for a model with no price.
    pub cost_micro_usd: Option<i64>,
    pub unknown_calls: i64,
    pub unknown_cost_micro_usd: Option<i64>,
    /// The worst cases taken from the budget and not settled: not 0 only
    /// after a crash between a reservation and its settle write.
    pub reserved_micro_usd: i64,
}

pub fn report(c: &Connection, run: i64) -> rusqlite::Result<Report> {
    c.query_row(
        "SELECT stop, messages, calls, input_tokens, output_tokens, cost_micro_usd,
                unknown_calls, unknown_cost_micro_usd, reserved_micro_usd
         FROM understand_run WHERE id = ?1",
        [run],
        |r| {
            Ok(Report {
                run,
                stop: r.get(0)?,
                messages: r.get(1)?,
                calls: r.get(2)?,
                input_tokens: r.get(3)?,
                output_tokens: r.get(4)?,
                cost_micro_usd: r.get(5)?,
                unknown_calls: r.get(6)?,
                unknown_cost_micro_usd: r.get(7)?,
                reserved_micro_usd: r.get(8)?,
            })
        },
    )
}

/// Makes the model of the row. The program builds an `iris-model` client
/// (and unseals the key); tests give a fake.
pub type Connect<'a> = &'a dyn Fn(&ModelRow) -> Result<Box<dyn Model>, String>;

/// Runs understanding on the pending messages of the store (spec "Batch
/// runner"). The run row keeps the counts and the stop; the report is that
/// row. On an error, the row gets `stop = error` and the error goes to the
/// caller.
pub fn run(store: &Store, connect: Connect, limits: &Limits, now_ms: i64) -> Result<Report, Error> {
    let _lock = lock(store)?;
    let row = store.read(|c| Ok(model_row(c)?))?;
    let priced = row.as_ref().is_some_and(|r| r.price.is_some());
    let name = row.as_ref().map_or("", |r| r.model.as_str());
    let run = store.write(|tx| {
        tx.execute(
            "INSERT INTO understand_run(started_at, model, prompt, cost_micro_usd,
                                        unknown_cost_micro_usd)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![now_ms, name, PROMPT, priced.then_some(0)],
        )?;
        Ok(tx.last_insert_rowid())
    })?;
    let result = run_messages(store, connect, row.as_ref(), run, limits, now_ms);
    let stop = result.as_ref().map_or(Stop::Error, |s| *s);
    store.write(|tx| {
        tx.execute(
            "UPDATE understand_run SET ended_at = ?2, stop = ?3 WHERE id = ?1",
            params![run, now_ms.max(clock_ms()), stop.as_str()],
        )?;
        Ok(())
    })?;
    result?;
    Ok(store.read(|c| Ok(report(c, run)?))?)
}

fn clock_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn run_messages(
    store: &Store,
    connect: Connect,
    row: Option<&ModelRow>,
    run: i64,
    limits: &Limits,
    now_ms: i64,
) -> Result<Stop, Error> {
    let inner: Box<dyn Model> = match row {
        Some(r) => connect(r).map_err(Error::Connect)?,
        None => Box::new(NoModel),
    };
    let model = Budgeted::new(inner.as_ref(), store, run, row);
    let mut r = Runner {
        store,
        model: &model,
        run,
        limits,
        now_ms,
        done: 0,
        since_refresh: 0,
        rerun: false,
    };
    r.refresh()?;
    // Main pass, then one more pass if a sender rerun reset mail. Reruns of
    // the second pass wait for the next run, so a message gets at most 2
    // calls of `store::understand` in one run.
    for pass in 0..2 {
        if pass == 1 && !r.rerun {
            break;
        }
        // The threads in order, read in pages after a cursor, so only one
        // page of thread IDs is in memory.
        let mut cursor = (-1i64, 0i64, 0i64);
        loop {
            let page = store.read(|c| Ok(threads_after(c, cursor, now_ms)?))?;
            if page.is_empty() {
                break;
            }
            for (key, thread) in page {
                cursor = key;
                if let Some(stop) = r.thread(thread)? {
                    return Ok(stop);
                }
            }
        }
    }
    let s = model.state();
    Ok(if !s.refused {
        Stop::Done
    } else if row.is_none() {
        Stop::NoModel
    } else {
        Stop::Budget
    })
}

struct Runner<'a> {
    store: &'a Store,
    model: &'a Budgeted<'a>,
    run: i64,
    limits: &'a Limits,
    now_ms: i64,
    done: u64,
    since_refresh: usize,
    /// A sender rerun reset mail in this run: one more pass.
    rerun: bool,
}

impl Runner<'_> {
    /// Step 2: the sender refresh and its reruns, in one write.
    fn refresh(&mut self) -> Result<(), Error> {
        let now = self.now_ms;
        let reset = self.store.write(|tx| {
            let rows = iris_senders::refresh(tx, now)?;
            rerun_senders(tx, &rows, now)
        })?;
        self.since_refresh = 0;
        self.rerun |= reset > 0;
        Ok(())
    }

    /// Runs the pending messages of one thread. The pending rule is read
    /// again before each message, and each message gets at most one call
    /// in one visit. Gives a stop, or `None` to go on with the next thread.
    fn thread(&mut self, thread: i64) -> Result<Option<Stop>, Error> {
        let mut called: Vec<i64> = Vec::new();
        loop {
            let next = self
                .store
                .read(|c| Ok(next_pending(c, thread, &called, self.now_ms)?))?;
            let Some(message) = next else {
                return Ok(None);
            };
            if self.limits.max_messages.is_some_and(|n| self.done >= n) {
                return Ok(Some(Stop::Messages));
            }
            if let Some(max) = self.limits.max_cost_micro_usd {
                let spent = self.store.read(|c| Ok(run_cost(c, self.run)?))?;
                if spent >= i64_of(max) {
                    return Ok(Some(Stop::Cost));
                }
            }
            called.push(message);
            let rows = match store::understand(self.store, message, self.model, self.now_ms) {
                Ok(o) => o.rerun,
                Err(store::Error::Model(_, rows)) => rows,
                Err(e) => return Err(Error::Understand(e)),
            };
            self.done += 1;
            self.since_refresh += 1;
            self.store.write(|tx| {
                tx.execute(
                    "UPDATE understand_run SET messages = messages + 1 WHERE id = ?1",
                    [self.run],
                )?;
                Ok(())
            })?;
            if let Some(e) = self.model.take_error() {
                return Err(Error::Store(e));
            }
            self.rerun |= !rows.is_empty();
            let s = self.model.state();
            if s.store_failed {
                return Ok(Some(Stop::Error));
            }
            if s.key_refused || s.server_failed {
                return Ok(Some(Stop::Model));
            }
            if s.unknown {
                return Ok(Some(Stop::Unknown));
            }
            if s.overrun {
                return Ok(Some(Stop::Budget));
            }
            if self.since_refresh >= self.limits.batch.max(1) {
                self.refresh()?;
            }
        }
    }
}

/// The run cost for the limit: `cost + unknown_cost`.
fn run_cost(c: &Connection, run: i64) -> rusqlite::Result<i64> {
    c.query_row(
        "SELECT coalesce(cost_micro_usd, 0) + coalesce(unknown_cost_micro_usd, 0)
         FROM understand_run WHERE id = ?1",
        [run],
        |r| r.get(0),
    )
}

/// The next page of threads with mail that is not understood, after
/// `cursor` in the order of spec 1 section 7.1: the 90-day group first,
/// then the thread with the newest message first. The key is
/// `(group, newest, thread)`.
#[allow(clippy::type_complexity)]
fn threads_after(
    c: &Connection,
    cursor: (i64, i64, i64),
    now_ms: i64,
) -> rusqlite::Result<Vec<((i64, i64, i64), i64)>> {
    let mut st = c.prepare_cached(
        "SELECT grp, newest, thread_id FROM (
           SELECT m.thread_id, coalesce(max(m.internal_date), :no_date) AS newest,
                  CASE WHEN max(m.internal_date) >= :recent THEN 0 ELSE 1 END AS grp
           FROM message m WHERE m.gone_at IS NULL GROUP BY m.thread_id
           HAVING max(m.understood_at IS NULL AND m.content_sha256 IS NOT NULL) = 1)
         WHERE (grp, -newest, -thread_id) > (:g, -:n, -:t)
         ORDER BY grp, newest DESC, thread_id DESC LIMIT :page",
    )?;
    let rows = st.query_map(
        rusqlite::named_params! {
            ":no_date": NO_DATE,
            ":recent": now_ms.saturating_sub(RECENT_MS),
            ":g": cursor.0,
            ":n": cursor.1,
            ":t": cursor.2,
            ":page": PAGE,
        },
        |r| {
            let key = (r.get(0)?, r.get(1)?, r.get(2)?);
            Ok((key, key.2))
        },
    )?;
    rows.collect()
}

/// The oldest pending message of `thread` that is not in `skip`.
fn next_pending(
    c: &Connection,
    thread: i64,
    skip: &[i64],
    now_ms: i64,
) -> rusqlite::Result<Option<i64>> {
    let sql = format!(
        "SELECT m.id FROM message m
         WHERE m.thread_id = :thread AND {BASE} AND {THREAD_RULE}
           AND m.id NOT IN (SELECT value FROM json_each(:skip))
         ORDER BY coalesce(m.internal_date, :no_date), m.id LIMIT 1"
    );
    c.prepare_cached(&sql)?
        .query_row(
            rusqlite::named_params! {
                ":thread": thread,
                ":now": now_ms,
                ":max_tries": MAX_TRIES,
                ":no_date": NO_DATE,
                ":skip": serde_json::to_string(skip).unwrap_or_else(|_| "[]".into()),
            },
            |r| r.get(0),
        )
        .optional()
}

/// The size of the work before a run (spec "Estimate").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Estimate {
    /// Reachable messages: each pending condition but the thread rule.
    pub messages: u64,
    /// Reachable messages in threads of the 90-day group.
    pub recent: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// At the price of `model_setting`; `None` with no price or no model.
    pub cost_micro_usd: Option<u64>,
    /// `model_setting.version`, for the receipt of the approval.
    pub model_version: Option<i64>,
}

/// Counts one call for each reachable message that is not `SPAM`. It runs
/// the sender refresh of the runner in a transaction that it rolls back, so
/// it changes nothing.
pub fn estimate(store: &Store, now_ms: i64) -> Result<Estimate, Error> {
    Ok(store.dry_write(|tx| estimate_in(tx, now_ms))?)
}

fn estimate_in(tx: &Transaction, now_ms: i64) -> iris_store::Result<Estimate> {
    let rows = iris_senders::refresh(tx, now_ms)?;
    rerun_senders(tx, &rows, now_ms)?;
    let sql = format!(
        "SELECT count(*),
           coalesce(sum((SELECT max(t.internal_date) FROM message t
                         WHERE t.thread_id = m.thread_id AND t.gone_at IS NULL) >= :recent), 0),
           coalesce(sum(CASE WHEN spam THEN 0 ELSE 1 END), 0),
           coalesce(sum(CASE WHEN spam THEN 0
                    ELSE min(coalesce(m.size, :max_data), :max_data) / :bpt END), 0)
         FROM (SELECT m.*, EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                                   WHERE ml.message_id = m.id AND l.provider_id = 'SPAM') AS spam
               FROM message m) m
         WHERE {BASE}"
    );
    let (messages, recent, calls, data_tokens): (i64, i64, i64, i64) = tx.query_row(
        &sql,
        rusqlite::named_params! {
            ":now": now_ms,
            ":max_tries": MAX_TRIES,
            ":recent": now_ms.saturating_sub(RECENT_MS),
            ":max_data": MAX_DATA as i64,
            ":bpt": BYTES_PER_TOKEN as i64,
        },
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let [messages, recent, calls, data_tokens] =
        [messages, recent, calls, data_tokens].map(|v| v.max(0) as u64);
    let input_tokens = calls
        .saturating_mul(PROMPT_TOKENS)
        .saturating_add(data_tokens);
    let output_tokens = calls.saturating_mul(OUTPUT_TOKENS);
    let row = model_row(tx)?;
    let cost_micro_usd = row.as_ref().and_then(|r| r.price).map(|p| {
        cost(
            Usage {
                input_tokens,
                output_tokens,
                ..Usage::default()
            },
            p,
        )
    });
    Ok(Estimate {
        messages,
        recent,
        input_tokens,
        output_tokens,
        cost_micro_usd,
        model_version: row.map(|r| r.version),
    })
}
