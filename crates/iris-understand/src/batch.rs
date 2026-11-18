//! The batch runner of `docs/specs/2026-11-16-cost-control.md`: it runs
//! [`crate::store::understand`] on the pending messages of the store, thread
//! by thread, with one run lock, one budget for all runs and the counts of
//! each run. [`estimate`] gives the size of the work and changes nothing.

use std::fmt;

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
const BASE: &str = concat!(
    "m.gone_at IS NULL AND m.understood_at IS NULL
  AND m.content_sha256 IS NOT NULL AND m.waits_model = 0
  AND m.tries < :max_tries AND (m.retry_at IS NULL OR m.retry_at <= :now)
  AND NOT ",
    iris_store::draft_or_copy!("m"),
    "
  AND NOT EXISTS (SELECT 1 FROM message o WHERE o.thread_id = m.thread_id
                  AND o.gone_at IS NULL AND o.content_sha256 IS NULL
                  AND NOT ",
    iris_store::draft_or_copy!("o"),
    ")"
);

/// The thread rule: no older message of the thread waits. Hostile mail
/// (label `SPAM`, or a `hostile` sender) does not wait.
const THREAD_RULE: &str = concat!(
    "(EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
          WHERE ml.message_id = m.id AND l.provider_id = 'SPAM')
   OR EXISTS (SELECT 1 FROM header h JOIN sender s ON s.address = h.from_addr
          WHERE h.message_id = m.id AND s.kind = 'hostile')
   OR NOT EXISTS (SELECT 1 FROM message o WHERE o.thread_id = m.thread_id AND o.id <> m.id
          AND o.gone_at IS NULL AND o.understood_at IS NULL AND o.tries < :max_tries
          AND (coalesce(o.internal_date, :no_date), o.id) < (coalesce(m.internal_date, :no_date), m.id)
          AND NOT ",
    iris_store::draft_or_copy!("o"),
    "))"
);

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
pub struct RunLock(#[allow(dead_code)] iris_store::FileLock);

/// Takes the run lock, or gives [`Error::Busy`].
pub fn lock(store: &Store) -> Result<RunLock, Error> {
    match iris_store::FileLock::try_take(&store.dir().join(LOCK_FILE)) {
        Ok(Some(l)) => Ok(RunLock(l)),
        Ok(None) => Err(Error::Busy),
        Err(e) => Err(Error::Store(e.into())),
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
    /// False for the run after a sync: a run that read no message and made
    /// no call deletes its row, so a run each minute adds no row.
    pub keep_idle: bool,
    /// True for the run after a sync: honor the wait after a model error
    /// of [`retry_after`]. During the wait the run makes no model call, and
    /// each refused message gets the mark `waits_model`, as with no model.
    /// The first run after the wait clears the marks and calls the model.
    /// The run takes these decisions under its lock. Every run that can
    /// call the model updates the wait at its end ([`end_wait`]).
    pub wait_after_error: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            batch: BATCH,
            max_messages: None,
            max_cost_micro_usd: None,
            keep_idle: true,
            wait_after_error: false,
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

impl Report {
    /// True if the run read no message, made no call and holds no
    /// reservation.
    pub fn is_idle(&self) -> bool {
        self.messages == 0
            && self.calls == 0
            && self.unknown_calls == 0
            && self.reserved_micro_usd == 0
    }
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
    // The row, the wait and the clear of the marks, in one write under the
    // lock: no other run can mark or clear between them.
    let (row, calls, run) = store.write(|tx| {
        let row = model_row(tx)?;
        let wait = match &row {
            Some(_) if limits.wait_after_error => retry_after(tx)?,
            _ => None,
        };
        let calls = wait.is_none_or(|t| now_ms >= t);
        if calls && wait.is_some() {
            // The wait has ended: this run takes it, so the next run does
            // not clear the marks again. The count of errors stays, so a new
            // error doubles the wait.
            tx.execute("UPDATE model_setting SET retry_at = NULL", [])?;
            clear_retry_marks(tx)?;
        }
        tx.execute(
            "INSERT INTO understand_run(started_at, model, prompt, cost_micro_usd,
                                        unknown_cost_micro_usd)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![
                now_ms,
                row.as_ref()
                    .filter(|_| calls)
                    .map_or("", |r| r.model.as_str()),
                PROMPT,
                row.as_ref().is_some_and(|r| r.price.is_some()).then_some(0)
            ],
        )?;
        Ok((row, calls, tx.last_insert_rowid()))
    })?;
    let result = run_messages(store, connect, row.as_ref(), calls, run, limits, now_ms);
    let stop = result.as_ref().map_or(Stop::Error, |s| *s);
    let keep_idle = limits.keep_idle || result.is_err();
    let report = store.write(|tx| {
        let ended = now_ms.max(clock_ms());
        tx.execute(
            "UPDATE understand_run SET ended_at = ?2, stop = ?3 WHERE id = ?1",
            params![run, ended, stop.as_str()],
        )?;
        let r = report(tx, run)?;
        if calls && row.is_some() {
            end_wait(tx, &stop, &r, ended)?;
        }
        if !keep_idle && r.is_idle() {
            tx.execute("DELETE FROM understand_run WHERE id = ?1", [run])?;
        }
        Ok(r)
    })?;
    result?;
    Ok(report)
}

/// The wall clock in milliseconds since the Unix epoch.
pub fn clock_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn run_messages(
    store: &Store,
    connect: Connect,
    row: Option<&ModelRow>,
    calls: bool,
    run: i64,
    limits: &Limits,
    now_ms: i64,
) -> Result<Stop, Error> {
    // Step 1: the link check of each new or old content (migration 20),
    // with no model, so a model error cannot hold it back. Each message
    // that it understands counts toward the run limit.
    let done =
        store::recheck_links(store, now_ms, limits.max_messages).map_err(Error::Understand)?;
    store.write(|tx| {
        tx.execute(
            "UPDATE understand_run SET messages = messages + ?2 WHERE id = ?1",
            params![run, i64_of(done)],
        )?;
        Ok(())
    })?;
    // Step 2 before step 3: the refresh needs no model, so a model that
    // cannot be made does not keep stale items of a changed sender. Its
    // resets come before the main pass, which sees them.
    refresh_senders(store, now_ms)?;
    let inner: Box<dyn Model> = match row {
        Some(r) if calls => connect(r).map_err(Error::Connect)?,
        _ => Box::new(NoModel),
    };
    let model = Budgeted::new(inner.as_ref(), store, run, row);
    let mut r = Runner {
        store,
        model: &model,
        mark: if row.is_some() && !calls {
            WAITS_RETRY
        } else {
            WAITS_INPUT
        },
        run,
        limits,
        now_ms,
        done,
        since_refresh: 0,
        rerun: false,
    };
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

/// Step 2: the sender refresh and its reruns, in one write. Gives the
/// count of reset messages.
fn refresh_senders(store: &Store, now_ms: i64) -> Result<u64, Error> {
    Ok(store.write(|tx| {
        let rows = iris_senders::refresh(tx, now_ms)?;
        rerun_senders(tx, &rows, now_ms)
    })?)
}

struct Runner<'a> {
    store: &'a Store,
    model: &'a Budgeted<'a>,
    run: i64,
    limits: &'a Limits,
    now_ms: i64,
    /// The cause that a refused message records in `waits_model`.
    mark: i64,
    done: u64,
    since_refresh: usize,
    /// A sender rerun reset mail in this run: one more pass.
    rerun: bool,
}

impl Runner<'_> {
    /// Step 5: step 2 again after a batch of messages.
    fn refresh(&mut self) -> Result<(), Error> {
        let reset = refresh_senders(self.store, self.now_ms)?;
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
                .read(|c| Ok(next_pending(self.store, c, thread, &called, self.now_ms)?))?;
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
                // The call was refused (no model, no budget, or the wait
                // after a model error): the message waits until that cause
                // ends, so later runs do not read it again. The mark names
                // the cause.
                Err(store::Error::Model(iris_model::Error::Budget(_), rows)) => {
                    self.store.write(|tx| {
                        tx.execute(
                            "UPDATE message SET waits_model = ?2
                             WHERE id = ?1 AND understood_at IS NULL",
                            params![message, self.mark],
                        )?;
                        Ok(())
                    })?;
                    rows
                }
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
           HAVING max(m.understood_at IS NULL AND m.content_sha256 IS NOT NULL
                      AND m.waits_model = 0) = 1)
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

/// The oldest pending message of `thread` that is not in `skip`. A message
/// that the thread rule holds still runs when it is hostile mail: `SPAM`,
/// or a `From` sender of the kind `hostile`. Sync saves no header row, so
/// the `From` address of such a message comes from the head of its content
/// file (fixed rules, no model).
fn next_pending(
    store: &Store,
    c: &Connection,
    thread: i64,
    skip: &[i64],
    now_ms: i64,
) -> rusqlite::Result<Option<i64>> {
    let sql = format!(
        "SELECT m.id, {THREAD_RULE}, m.content_sha256, h.from_addr FROM message m
         LEFT JOIN header h ON h.message_id = m.id
         WHERE m.thread_id = :thread AND {BASE}
           AND m.id NOT IN (SELECT value FROM json_each(:skip))
         ORDER BY coalesce(m.internal_date, :no_date), m.id"
    );
    let mut st = c.prepare_cached(&sql)?;
    let mut rows = st.query(rusqlite::named_params! {
        ":thread": thread,
        ":now": now_ms,
        ":max_tries": MAX_TRIES,
        ":no_date": NO_DATE,
        ":skip": serde_json::to_string(skip).unwrap_or_else(|_| "[]".into()),
    })?;
    while let Some(r) = rows.next()? {
        let (id, free): (i64, bool) = (r.get(0)?, r.get(1)?);
        if free {
            return Ok(Some(id));
        }
        let from: Option<String> = match r.get::<_, Option<String>>(3)? {
            Some(a) => Some(a),
            None => r
                .get::<_, Option<String>>(2)?
                .and_then(|sha| store.content_path(&sha))
                .and_then(|path| from_of(&path)),
        };
        if let Some(from) = from {
            let kind: Option<String> = c
                .prepare_cached("SELECT kind FROM sender WHERE address = ?1")?
                .query_row([&from], |r| r.get(0))
                .optional()?
                .flatten();
            if kind.as_deref() == Some("hostile") {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}

/// The bare `From` address in the head of a content file, lowercase.
fn from_of(path: &std::path::Path) -> Option<String> {
    use std::io::Read;
    let mut raw = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(crate::head::HEAD_LIMIT as u64)
        .read_to_end(&mut raw)
        .ok()?;
    crate::Head::parse(&raw, None).from_addr
}

/// `waits_model`: the call was refused for an input of the store (no
/// model, or no budget). A change of that input, or an approval, clears it.
pub const WAITS_INPUT: i64 = 1;
/// `waits_model`: the run made no call in the wait after a model error.
/// Only the end of that wait clears it.
pub const WAITS_RETRY: i64 = 2;

/// Clears each mark `waits_model` (an approval), so the next run reads that
/// mail again. Gives the count of messages.
pub fn clear_waiting(tx: &Transaction) -> rusqlite::Result<usize> {
    tx.execute(
        "UPDATE message SET waits_model = 0 WHERE waits_model <> 0",
        [],
    )
}

/// Clears the marks of the wait after a model error, at the end of that
/// wait. The marks of a budget or model refusal stay.
fn clear_retry_marks(tx: &Transaction) -> rusqlite::Result<usize> {
    tx.execute(
        "UPDATE message SET waits_model = 0 WHERE waits_model = ?1",
        [WAITS_RETRY],
    )
}

/// The first wait after a run that stopped on `model`, `unknown` or
/// `error`, in milliseconds. Each such stop in a row doubles it.
pub const RETRY_FIRST_MS: i64 = 60_000;
/// The longest wait, in milliseconds.
pub const RETRY_MAX_MS: i64 = 3_600_000;

/// The end of the wait after model errors of the current model row
/// (`model_setting.retry_at`), or `None`. `iris model set` writes a new row,
/// so a new model has no wait.
pub fn retry_after(c: &Connection) -> rusqlite::Result<Option<i64>> {
    Ok(
        c.query_row("SELECT retry_at FROM model_setting", [], |r| r.get(0))
            .optional()?
            .flatten(),
    )
}

/// The wait state after a run that could call the model, in the end write
/// of the run (under its lock). Only evidence of the model changes it: a
/// stop on `model`, `unknown` or `error` starts or doubles the wait; a run
/// that made a call and did not stop on an error ends it and clears the
/// marks of that wait (`WAITS_RETRY` only: a budget refusal of the same
/// run keeps its mark). A run that made no call changes nothing.
fn end_wait(tx: &Transaction, stop: &Stop, r: &Report, ended: i64) -> rusqlite::Result<()> {
    update_wait(
        tx,
        matches!(stop, Stop::Model | Stop::Unknown | Stop::Error),
        r.calls > 0,
        ended,
    )
}

/// The one writer of the wait after model errors, for each caller that can
/// call the model (the runner and the proposal step of matters), under the
/// run lock. `failed`: the model or the call failed (a refused key, a
/// server error, an unknown result); this starts or doubles the wait.
/// Else `called` (the model answered) ends the wait and clears its marks.
pub fn update_wait(
    tx: &Transaction,
    failed: bool,
    called: bool,
    ended: i64,
) -> rusqlite::Result<()> {
    if failed {
        tx.execute(
            "UPDATE model_setting SET retry_failures = retry_failures + 1,
                    retry_at = ?1 + min(?2 << min(retry_failures, 6), ?3)",
            params![ended, RETRY_FIRST_MS, RETRY_MAX_MS],
        )?;
    } else if called {
        let waited: bool =
            tx.query_row("SELECT retry_failures > 0 FROM model_setting", [], |r| {
                r.get(0)
            })?;
        if waited {
            tx.execute(
                "UPDATE model_setting SET retry_failures = 0, retry_at = NULL",
                [],
            )?;
            clear_retry_marks(tx)?;
        }
    }
    Ok(())
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
    let price = model_row(tx)?;
    let version = price.as_ref().map(|r| r.version);
    let price = price.and_then(|r| r.price);
    // One row for each reachable message, read as a stream: the memory
    // stays bounded. Each call is rounded on its own, like the budget.
    let sql = format!(
        "SELECT (SELECT max(t.internal_date) FROM message t
                 WHERE t.thread_id = m.thread_id AND t.gone_at IS NULL) >= :recent,
                EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                        WHERE ml.message_id = m.id AND l.provider_id = 'SPAM'),
                min(coalesce(m.size, :max_data), :max_data)
         FROM message m WHERE {}",
        // Mail that waits for a model is the work that an approval pays for.
        BASE.replace(" AND m.waits_model = 0", "")
    );
    let mut st = tx.prepare(&sql)?;
    let mut rows = st.query(rusqlite::named_params! {
        ":now": now_ms,
        ":max_tries": MAX_TRIES,
        ":recent": now_ms.saturating_sub(RECENT_MS),
        ":max_data": MAX_DATA as i64,
    })?;
    let mut e = Estimate {
        cost_micro_usd: price.map(|_| 0),
        model_version: version,
        ..Estimate::default()
    };
    while let Some(r) = rows.next()? {
        let (recent, spam, size): (Option<bool>, bool, i64) = (r.get(0)?, r.get(1)?, r.get(2)?);
        e.messages += 1;
        e.recent += u64::from(recent == Some(true));
        if spam {
            continue;
        }
        let call = Usage {
            input_tokens: PROMPT_TOKENS + size.max(0) as u64 / BYTES_PER_TOKEN,
            output_tokens: OUTPUT_TOKENS,
            ..Usage::default()
        };
        e.input_tokens = e.input_tokens.saturating_add(call.input_tokens);
        e.output_tokens = e.output_tokens.saturating_add(call.output_tokens);
        if let (Some(sum), Some(p)) = (e.cost_micro_usd.as_mut(), price) {
            *sum = sum.saturating_add(cost(call, p));
        }
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A thread whose only unread mail waits for a model gives no page, so
    /// an idle run reads no thread (PR #137 review round 5).
    #[test]
    fn a_thread_with_only_waiting_mail_gives_no_page() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE message(id INTEGER PRIMARY KEY, thread_id INTEGER,
               internal_date INTEGER, gone_at INTEGER, understood_at INTEGER,
               content_sha256 BLOB, waits_model INTEGER NOT NULL DEFAULT 0);
             INSERT INTO message VALUES (1, 10, 5, NULL, NULL, x'00', 1);
             INSERT INTO message VALUES (2, 10, 6, NULL, 7, x'00', 0);
             INSERT INTO message VALUES (3, 20, 5, NULL, NULL, x'00', 1);
             INSERT INTO message VALUES (4, 20, 6, NULL, NULL, x'00', 0);",
        )
        .unwrap();
        let start = (-1, 0, 0);
        let threads: Vec<i64> = threads_after(&c, start, 10)
            .unwrap()
            .into_iter()
            .map(|(_, t)| t)
            .collect();
        assert_eq!(threads, [20]);
        c.execute("UPDATE message SET waits_model = 1 WHERE id = 4", [])
            .unwrap();
        assert!(threads_after(&c, start, 10).unwrap().is_empty());
    }
}
