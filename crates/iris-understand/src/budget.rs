//! The budget of understanding (docs/specs/2026-11-16-cost-control.md,
//! "Model and budget" and "Reservation"): the `model_setting` row, and
//! [`Budgeted`], the model that reserves the worst case of each call from
//! the one budget before the call and settles after it, with the counts of
//! each call in the `understand_run` row.

use std::cell::{Cell, RefCell};

use iris_model::{cost, Price, Usage};
use iris_store::Store;
use rusqlite::{params, Connection, OptionalExtension};

use crate::Model;

#[derive(Clone, Debug)]
pub struct ModelRow {
    /// `anthropic` or `local`.
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub version: i64,
    /// `None` only for a local model.
    pub price: Option<Price>,
    /// The key, sealed with the store key.
    pub api_key: Option<Vec<u8>>,
    pub max_tokens: u32,
    pub timeout_s: i64,
    pub budget_micro_usd: Option<i64>,
}

pub fn model_row(c: &Connection) -> rusqlite::Result<Option<ModelRow>> {
    c.query_row(
        "SELECT provider, base_url, model, version, price_in, price_out, api_key,
                max_tokens, timeout_s, budget_micro_usd
         FROM model_setting WHERE id = 1",
        [],
        |r| {
            let provider: String = r.get(0)?;
            let (pin, pout): (Option<i64>, Option<i64>) = (r.get(4)?, r.get(5)?);
            // Only a local model is free. A missing price of another model
            // (the schema refuses it) gives the largest price, so the
            // budget refuses each call.
            let price = (provider != "local").then(|| Price {
                input_micro_usd_per_mtok: pin.map_or(u64::MAX, |p| p.max(0) as u64),
                output_micro_usd_per_mtok: pout.map_or(u64::MAX, |p| p.max(0) as u64),
            });
            Ok(ModelRow {
                provider,
                base_url: r.get(1)?,
                model: r.get(2)?,
                version: r.get(3)?,
                price,
                api_key: r.get(6)?,
                max_tokens: r.get(7)?,
                timeout_s: r.get(8)?,
                budget_micro_usd: r.get(9)?,
            })
        },
    )
    .optional()
}

/// The model when no `model_setting` row exists: each call is refused.
pub struct NoModel;

impl Model for NoModel {
    fn name(&self) -> &str {
        ""
    }

    fn ask(&self, _: &str, _: &str) -> Result<iris_model::Reply, iris_model::Error> {
        Err(iris_model::Error::Budget("no model is set".into()))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// A call was refused before it went out (budget, or no model).
    pub refused: bool,
    /// A call to a priced model had an unknown result or unknown usage.
    pub unknown: bool,
    /// HTTP 401 or 403.
    pub key_refused: bool,
    /// A model with no price failed with no answer (time-out, 5xx, 429).
    pub server_failed: bool,
    /// A known cost was larger than its reservation.
    pub overrun: bool,
    /// A reservation or a settle write failed: the counts of a call can be
    /// missing, so no later call goes out.
    pub store_failed: bool,
    /// A call stopped for a waiter of the run lock ([`Budgeted::yielding`],
    /// issue #1126). The result of the call is unknown: a priced call keeps
    /// its worst case as unknown cost. It is no model error. No later call
    /// goes out.
    pub cancelled: bool,
}

/// The model that `store::understand` gets from the runner (spec
/// "Reservation"). For a priced model, it reserves the worst case of each
/// call from the budget before the call and settles after it.
pub struct Budgeted<'a> {
    inner: &'a dyn Model,
    store: &'a Store,
    run: i64,
    price: Option<Price>,
    max_tokens: u64,
    /// Stop the wait for an answer when a caller of `batch::lock_wait` waits.
    yields: bool,
    state: Cell<State>,
    /// A store error of a reservation or a settle write: the run stops.
    error: RefCell<Option<iris_store::Error>>,
}

/// The counts of one call for the run row.
#[derive(Default)]
struct Counts {
    calls: i64,
    input: i64,
    output: i64,
    cost: i64,
    unknown_calls: i64,
    unknown_cost: i64,
}

fn refuse(why: &str) -> iris_model::Error {
    iris_model::Error::Budget(why.to_string())
}

pub(crate) fn i64_of(v: u64) -> i64 {
    v.min(i64::MAX as u64) as i64
}

impl<'a> Budgeted<'a> {
    /// The model for run `run` (an `understand_run` row). `row` is the
    /// `model_setting` row that made `inner`.
    pub fn new(inner: &'a dyn Model, store: &'a Store, run: i64, row: Option<&ModelRow>) -> Self {
        Budgeted {
            inner,
            store,
            run,
            price: row.and_then(|r| r.price),
            max_tokens: row.map_or(0, |r| r.max_tokens as u64),
            yields: false,
            state: Cell::default(),
            error: RefCell::new(None),
        }
    }

    /// The model for a holder of the run lock that took it with
    /// `batch::lock` (a run, the matter step, the nudge and summary steps):
    /// the wait for an answer stops when a draft or a question waits for the
    /// run lock (`batch::wanted`, issue #1126).
    pub fn yielding(mut self) -> Self {
        self.yields = true;
        self
    }

    /// Calls the inner model, with the cancel of [`Budgeted::yielding`].
    fn call(&self, instruction: &str, data: &str) -> Result<iris_model::Reply, iris_model::Error> {
        if self.yields {
            let store = self.store;
            self.inner
                .ask_cancel(instruction, data, &|| crate::batch::wanted(store))
        } else {
            self.inner.ask(instruction, data)
        }
    }

    /// What the calls of this run met so far.
    pub fn state(&self) -> State {
        self.state.get()
    }

    /// The first store error of a reservation or a settle write. The run
    /// must stop: the counts of a call can be missing. `State::store_failed`
    /// stays set after this call.
    pub fn take_error(&self) -> Option<iris_store::Error> {
        self.error.take()
    }

    fn set(&self, f: impl FnOnce(&mut State)) {
        let mut s = self.state.get();
        f(&mut s);
        self.state.set(s);
    }

    /// Writes the counts of a call, and gives `give_back` to the budget
    /// (it can be negative: an overrun) and takes `reserved` from the
    /// reservation of the run.
    /// Gives false if the write failed: the caller must not use the reply.
    fn settle(&self, reserved: i64, give_back: i64, n: Counts) -> bool {
        let r = self.store.write(|tx| {
            if reserved != 0 || give_back != 0 {
                tx.execute(
                    "UPDATE model_setting SET budget_micro_usd = budget_micro_usd + ?1 WHERE id = 1",
                    [give_back],
                )?;
            }
            tx.execute(
                "UPDATE understand_run SET reserved_micro_usd = reserved_micro_usd - ?2,
                   calls = calls + ?3, input_tokens = input_tokens + ?4,
                   output_tokens = output_tokens + ?5,
                   cost_micro_usd = cost_micro_usd + ?6, unknown_calls = unknown_calls + ?7,
                   unknown_cost_micro_usd = unknown_cost_micro_usd + ?8
                 WHERE id = ?1",
                params![
                    self.run,
                    reserved,
                    n.calls,
                    n.input,
                    n.output,
                    n.cost,
                    n.unknown_calls,
                    n.unknown_cost
                ],
            )?;
            Ok(())
        });
        match r {
            Ok(()) => true,
            Err(e) => {
                self.fail(e);
                false
            }
        }
    }

    fn fail(&self, e: iris_store::Error) {
        self.error.borrow_mut().get_or_insert(e);
        self.set(|s| s.store_failed = true);
    }

    fn ask_free(
        &self,
        instruction: &str,
        data: &str,
    ) -> Result<iris_model::Reply, iris_model::Error> {
        let r = self.call(instruction, data);
        let mut n = Counts::default();
        match &r {
            // The request can have gone out: one call with unknown tokens.
            Err(iris_model::Error::Cancelled) => {
                n.unknown_calls = 1;
                self.set(|s| s.cancelled = true);
            }
            Ok(reply) if reply.usage.known => {
                n.calls = 1;
                n.input = i64_of(reply.usage.input_tokens);
                n.output = i64_of(reply.usage.output_tokens);
            }
            // No price: no charge is possible. The tokens are not known.
            Ok(_) | Err(iris_model::Error::BadAnswer(_)) => n.unknown_calls = 1,
            Err(iris_model::Error::Rejected { status, .. }) => {
                n.calls = 1;
                if matches!(status, 401 | 403) {
                    self.set(|s| s.key_refused = true);
                }
            }
            Err(
                iris_model::Error::Transport(_)
                | iris_model::Error::Server(_)
                | iris_model::Error::RateLimited { .. },
            ) => {
                n.unknown_calls = 1;
                self.set(|s| s.server_failed = true);
            }
            Err(iris_model::Error::Budget(_)) => self.set(|s| s.refused = true),
            Err(iris_model::Error::NoKey | iris_model::Error::NotJson) => {}
        }
        // The cost columns are NULL for a model with no price: `+ 0` keeps NULL.
        if !self.settle(0, 0, n) {
            return Err(refuse("the counts of the call were not saved"));
        }
        r
    }
}

impl Model for Budgeted<'_> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn request_bytes(&self, instruction: &str, data: &str) -> u64 {
        self.inner.request_bytes(instruction, data)
    }

    fn ask(&self, instruction: &str, data: &str) -> Result<iris_model::Reply, iris_model::Error> {
        // After a failed store write, the runner must stop: no call.
        if self.state.get().store_failed {
            return Err(refuse("a store write of this run failed"));
        }
        // After a cancel, the holder stops: no refusal (it is no budget
        // cause, so no mail gets a mark), the same cancel again.
        if self.state.get().cancelled {
            return Err(iris_model::Error::Cancelled);
        }
        // A waiter that came before the request (for example during the
        // call before a retry) gets the lock now: no request starts, so no
        // cost counts (issue #1126).
        if self.yields && crate::batch::wanted(self.store) {
            self.set(|s| s.cancelled = true);
            return Err(iris_model::Error::Cancelled);
        }
        let Some(price) = self.price else {
            return self.ask_free(instruction, data);
        };
        // After a call whose charge was not inside its reservation, no
        // call goes out until the runner stops.
        let s = self.state.get();
        if s.unknown || s.overrun {
            return Err(refuse("an earlier call of this run passed its reservation"));
        }
        let worst = cost(
            Usage {
                input_tokens: self.inner.request_bytes(instruction, data),
                output_tokens: self.max_tokens,
                ..Usage::default()
            },
            price,
        );
        // A worst case that does not fit in i64 is larger than any budget.
        let Ok(worst) = i64::try_from(worst) else {
            self.set(|s| s.refused = true);
            return Err(refuse("the worst case of the call overflows"));
        };
        // Step 1: one conditional write takes the worst case. NULL (no
        // approval) or a smaller budget changes no row: a refusal.
        let reserved = self.store.write(|tx| {
            let n = tx.execute(
                "UPDATE model_setting SET budget_micro_usd = budget_micro_usd - ?1
                 WHERE id = 1 AND budget_micro_usd IS NOT NULL AND budget_micro_usd >= ?1",
                [worst],
            )?;
            if n == 1 {
                tx.execute(
                    "UPDATE understand_run SET reserved_micro_usd = reserved_micro_usd + ?2
                     WHERE id = ?1",
                    [self.run, worst],
                )?;
            }
            Ok(n == 1)
        });
        match reserved {
            Ok(true) => {}
            Ok(false) => {
                self.set(|s| s.refused = true);
                return Err(refuse("the budget cannot pay the worst case of the call"));
            }
            Err(e) => {
                self.fail(e);
                return Err(refuse("the reservation failed"));
            }
        }
        // Step 2.
        let r = self.call(instruction, data);
        // Step 3: the real cost, or the worst case for unknown usage.
        let mut n = Counts::default();
        let spent = match &r {
            Ok(reply) if reply.usage.known => {
                let real = i64_of(cost(reply.usage, price));
                n.calls = 1;
                n.input = i64_of(reply.usage.input_tokens);
                n.output = i64_of(reply.usage.output_tokens);
                n.cost = real;
                if real > worst {
                    self.set(|s| s.overrun = true);
                }
                real
            }
            // A refused key: a definite rejection before the model runs, no
            // charge. The whole reservation goes back.
            Err(iris_model::Error::Rejected {
                status: 401 | 403, ..
            }) => {
                n.calls = 1;
                self.set(|s| s.key_refused = true);
                0
            }
            // No request went out.
            Err(iris_model::Error::NoKey | iris_model::Error::Budget(_)) => 0,
            // A cancel for a waiter (issue #1126): the provider can charge
            // the request, so the worst case stays taken, as for an unknown
            // result. It is no model error, so it starts no model wait.
            Err(iris_model::Error::Cancelled) => {
                n.unknown_calls = 1;
                n.unknown_cost = worst;
                self.set(|s| s.cancelled = true);
                worst
            }
            // Every other answer has no reliable usage: the provider can
            // charge. The worst case stays taken, and the run stops.
            _ => {
                n.unknown_calls = 1;
                n.unknown_cost = worst;
                self.set(|s| s.unknown = true);
                worst
            }
        };
        // A reply whose counts are not saved is not used: write 3 must not
        // save items of a call that the run row does not count.
        if !self.settle(worst, worst - spent, n) {
            return Err(refuse("the counts of the call were not saved"));
        }
        r
    }
}
