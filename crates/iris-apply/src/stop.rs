//! One Stop for all agent work (docs/specs/2026-11-17-stop.md): the switch
//! of migration 33, the end of each open session, and the resume.
//!
//! The checks are in the claim of the apply and of the release (`gate`),
//! in the cutoff before each later call of an apply, and in the apply of a
//! state verb. The triggers `job_session_stop` and `ledger_stop` are the
//! backstop in the store.

use iris_store::Store;

use crate::{lock, now, Error, Result};

/// The reason of an action that a stop cut off after its first call.
pub const STOPPED_CALL: &str =
    "You pressed Stop. The calls before the stop ran, and no later call runs.";

/// What a stop did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stopped {
    /// The time of the stop, in milliseconds since 1970. A second stop
    /// keeps the time of the first.
    pub since: i64,
    /// The open sessions that this stop ended.
    pub sessions: usize,
    /// The sessions that are not reaped at the end of the wait, also
    /// sessions that an earlier stop ended. A process of such a session can
    /// still run, but the session socket refuses each of its requests.
    pub live: usize,
    /// A Gmail call still ran when the wait for the apply lock ended. Its
    /// result goes to the ledger, and no later call starts.
    pub busy: bool,
}

/// Stop: set the switch and end each open session with `stop`, in one
/// transaction. Then wait up to [`crate::send::OFF_WAIT`] in total for the
/// apply lock (`busy` if a Gmail call still runs) and for the reap of each
/// session (`live`). The first step never waits. The stop is complete only
/// when `live` is 0 and `busy` is false.
pub fn stop(store: &Store) -> Result<Stopped> {
    stop_wait(store, crate::send::OFF_WAIT)
}

/// [`stop`] with the longest wait.
pub fn stop_wait(store: &Store, wait: std::time::Duration) -> Result<Stopped> {
    let (since, sessions) = store.write(|tx| {
        // The clock of SQLite, read after the write lock, as in `resume`.
        tx.execute(
            "UPDATE stop_switch SET
               stopped_at = coalesce(stopped_at, CAST(unixepoch('subsec') * 1000 AS INTEGER)),
               changed_at = CAST(unixepoch('subsec') * 1000 AS INTEGER) WHERE id = 1",
            [],
        )?;
        let open: Vec<i64> = {
            let mut st = tx.prepare("SELECT id FROM job_session WHERE ended_at IS NULL")?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for &session in &open {
            iris_job::end_in(tx, session, iris_job::EndReason::Stop, None, now()).map_err(|e| {
                match e {
                    iris_job::Error::Store(e) => e,
                    other => iris_store::Error::Query(other.to_string()),
                }
            })?;
        }
        let since: i64 =
            tx.query_row("SELECT stopped_at FROM stop_switch WHERE id = 1", [], |r| {
                r.get(0)
            })?;
        Ok((since, open.len()))
    })?;
    let (live, busy) = measure(store, wait)?;
    Ok(Stopped {
        since,
        sessions,
        live,
        busy,
    })
}

/// The state of the stop now, with no write and no wait: `None` while Iris
/// runs. `sessions` is 0. Each word that says that a stop is complete
/// reads `live` and `busy` from here or from [`stop`], never the switch
/// alone (docs/specs/2026-11-17-stop.md, section 2).
pub fn state(store: &Store) -> Result<Option<Stopped>> {
    let Some(since) = store.read(iris_store::stopped)? else {
        return Ok(None);
    };
    let (live, busy) = measure(store, std::time::Duration::ZERO)?;
    Ok(Some(Stopped {
        since,
        sessions: 0,
        live,
        busy,
    }))
}

/// The sessions that are not reaped, and whether a Gmail call runs (the
/// apply lock is held), now, with no wait and with or without a stop. Each
/// answer of the Stop API reads them (docs/specs/2026-11-17-stop.md,
/// section 5).
pub fn measure_now(store: &Store) -> Result<(usize, bool)> {
    measure(store, std::time::Duration::ZERO)
}

/// Wait up to `wait` in total for the apply lock and for the reap of each
/// session. Gives the sessions that are not reaped, and whether a Gmail
/// call still runs (the apply lock is held).
fn measure(store: &Store, wait: std::time::Duration) -> Result<(usize, bool)> {
    let end = std::time::Instant::now() + wait;
    let pause = || std::thread::sleep(std::time::Duration::from_millis(100));
    let busy = loop {
        match lock(store) {
            Ok(_lock) => break false,
            Err(Error::Busy) if std::time::Instant::now() < end => pause(),
            Err(Error::Busy) => break true,
            Err(e) => return Err(e),
        }
    };
    let live = loop {
        let n: i64 = store.read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM job_session WHERE reaped_at IS NULL",
                [],
                |r| r.get(0),
            )?)
        })?;
        if n == 0 || std::time::Instant::now() >= end {
            break n as usize;
        }
        pause();
    };
    Ok((live, busy))
}

/// Resume: clear the switch. If sending is on, its `on_at` moves to the
/// time of the resume, so each held send waits a full hold again and the
/// person can cancel it. Gives `false` if Iris was not stopped.
pub fn resume(store: &Store) -> Result<bool> {
    // SQLite reads the clock in the statement, after it holds the write
    // lock: the hold counts from the resume, also after a wait for another
    // writer (as `set_sending`, issue #420).
    Ok(store.write(|tx| {
        let n = tx.execute(
            "UPDATE stop_switch SET stopped_at = NULL,
               changed_at = CAST(unixepoch('subsec') * 1000 AS INTEGER)
             WHERE id = 1 AND stopped_at IS NOT NULL",
            [],
        )?;
        if n == 1 {
            tx.execute(
                "UPDATE send_switch SET
                   on_at = (SELECT changed_at FROM stop_switch WHERE id = 1),
                   changed_at = (SELECT changed_at FROM stop_switch WHERE id = 1)
                 WHERE id = 1 AND on_at IS NOT NULL",
                [],
            )?;
        }
        Ok(n == 1)
    })?)
}
