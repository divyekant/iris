//! One draft as one run of the budget of understanding
//! (docs/specs/2026-11-16-cost-control.md): the lock of cost control, an
//! `understand_run` row with the prompt `draft/0.1` for the counts of the
//! call, and the `Budgeted` model that reserves the worst case first. The
//! command `iris write` and the local API (`POST /api/threads/{t}/draft`)
//! both call [`draft`], so both pay for the call the same way.

use iris_store::Store;
use iris_understand::batch;
use iris_understand::budget::{model_row, Budgeted, ModelRow};
use iris_understand::Model;
use rusqlite::params;

use crate::{write_reply, Form, Saved, PROMPT};

/// Makes the model of a `model_setting` row.
pub type Connect<'a> = &'a dyn Fn(&ModelRow) -> Result<Box<dyn Model>, String>;

/// A saved draft, and the warnings of the budget for its call.
#[derive(Debug)]
pub struct Drafted {
    pub saved: Saved,
    /// The cost of the call is not known. The budget kept its worst case.
    pub unknown_cost: bool,
    /// The call cost more than its reservation.
    pub overrun: bool,
}

/// Why [`draft`] saved no draft.
#[derive(Debug)]
pub enum Failure {
    /// An understanding run or a model change holds the lock of cost control.
    Busy,
    /// No model is set.
    NoModel,
    /// The model client could not start.
    Connect(String),
    /// The budget refused the call, or the call went over the budget.
    Budget(crate::Error),
    /// The draft failed: see [`crate::Error`].
    Draft(crate::Error),
    Store(iris_store::Error),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Busy => f.write_str(
                "an understanding run or a model change is in progress: try again in a minute",
            ),
            Failure::NoModel => f.write_str("No model is set. Use `iris model set`."),
            Failure::Connect(e) => f.write_str(e),
            Failure::Budget(e) | Failure::Draft(e) => write!(f, "{e}"),
            Failure::Store(e) => write!(f, "store: {e}"),
        }
    }
}

/// Writes and saves the draft of the reply to `thread`, with one model call
/// through the budget, and one more if the answer is not a valid draft. `now_ms` is the time of the start; `clock` gives the
/// time of the end of the run (never before the start).
pub fn draft(
    store: &Store,
    thread: i64,
    form: Form<'_>,
    connect: Connect,
    now_ms: i64,
    clock: &dyn Fn() -> i64,
) -> Result<Drafted, Failure> {
    // The lock of cost control: no `iris model set` and no budget approval
    // can change the model row or the budget during the call.
    // A running understanding run stops before its next message (#279).
    let _lock = batch::lock_wait(store, batch::WAIT).map_err(|e| match e {
        batch::Error::Busy => Failure::Busy,
        e => Failure::Connect(e.to_string()),
    })?;
    let row = store
        .read(|c| Ok(model_row(c)?))
        .map_err(Failure::Store)?
        .ok_or(Failure::NoModel)?;
    let model = connect(&row).map_err(Failure::Connect)?;
    let started = now_ms;
    let run = store
        .write(|tx| {
            tx.execute(
                "INSERT INTO understand_run(started_at, model, prompt, cost_micro_usd,
                                            unknown_cost_micro_usd)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![started, row.model, PROMPT, row.price.is_some().then_some(0)],
            )?;
            Ok(tx.last_insert_rowid())
        })
        .map_err(Failure::Store)?;
    let budgeted = Budgeted::new(model.as_ref(), store, run, Some(&row));
    let result = write_reply(store, thread, form, &budgeted, started);
    let s = budgeted.state();
    // The same stop names as a run of understanding. A valid draft can
    // still have an unknown charge or a cost above its reservation.
    let stop = if s.refused || s.overrun {
        "budget"
    } else if s.unknown {
        "unknown"
    } else if s.key_refused || s.server_failed {
        "model"
    } else if result.is_ok() {
        "done"
    } else {
        "error"
    };
    // The wait after a model error is shared with understanding: only
    // evidence of the model changes it. An error of the draft itself (a
    // hostile thread, an invalid answer) is no evidence against the model.
    let failed = matches!(stop, "unknown" | "model");
    let end = store.write(|tx| {
        let ended = started.max(clock());
        tx.execute(
            "UPDATE understand_run SET ended_at = ?2, stop = ?3 WHERE id = ?1",
            params![run, ended, stop],
        )?;
        let r = batch::report(tx, run)?;
        batch::update_wait(tx, failed, r.calls > 0, ended)?;
        Ok(())
    });
    if let Some(e) = budgeted.take_error() {
        return Err(Failure::Store(e));
    }
    let saved = result.map_err(|e| match stop {
        "budget" => Failure::Budget(e),
        _ => Failure::Draft(e),
    })?;
    end.map_err(Failure::Store)?;
    Ok(Drafted {
        saved,
        unknown_cost: s.unknown,
        overrun: s.overrun,
    })
}
