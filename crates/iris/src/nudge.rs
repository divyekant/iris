//! Nudges (card R3-07): when a person is late on an item owed to you, the
//! engine prepares a reminder in your voice ([`iris_voice::write`] with
//! [`iris_voice::NUDGE_INTENT`]), through the one budget of understanding.
//! It sends nothing: the draft stays in the store until the person acts.

use iris_store::Store;
use iris_understand::batch;
use iris_understand::budget::{model_row, Budgeted, ModelRow};
use iris_understand::Model;
use rusqlite::params;

use crate::reply::Connect;

/// The most nudge calls in one turn of the understanding thread.
pub const MAX_PER_TURN: usize = 3;

/// The most late threads that one step reads.
pub const PAGE: usize = 20;

/// One nudge step: a call for each late thread of one page of
/// [`iris_voice::late`] (at most [`PAGE`] threads, the oldest try first),
/// at most [`MAX_PER_TURN`] calls. A thread starts only when its two calls
/// (a call and one retry) fit in the rest of the limit. No call when another run holds the
/// lock, when no model is set, or during the wait after a model error.
/// Each try is in the store ([`iris_voice::tried`]): it moves the thread to
/// the end of the order, so a thread that the budget cannot pay does not
/// stop the others. Only an answer that is not a valid draft blocks the
/// same key and model version. A budget refusal leaves no run row. Gives
/// the count of saved drafts.
pub fn step(store: &Store, connect: Connect, now_ms: i64) -> Result<usize, String> {
    let _lock = match batch::lock(store) {
        Ok(l) => l,
        Err(batch::Error::Busy) => return Ok(0),
        Err(e) => return Err(e.to_string()),
    };
    let (row, wait) = store
        .read(|c| Ok((model_row(c)?, batch::retry_after(c)?)))
        .map_err(|e| e.to_string())?;
    let Some(row) = row else { return Ok(0) };
    if wait.is_some_and(|t| t > now_ms) {
        return Ok(0);
    }
    let late = store
        .read(|c| Ok(iris_voice::late(c, now_ms, None, PAGE)?))
        .map_err(|e| e.to_string())?;
    if late.is_empty() {
        return Ok(0);
    }
    let model = connect(&row)?;
    let mut saved = 0;
    let mut calls = 0;
    for l in late {
        // A nudge can make two calls (a call and the retry of an invalid
        // answer). With less room in the turn, the thread waits for the
        // next step, so its retry is not lost.
        if MAX_PER_TURN - calls < 2 {
            break;
        }
        let (outcome, made) = call(store, &row, model.as_ref(), &l, now_ms)?;
        calls += made;
        store
            .write(|tx| {
                Ok(iris_voice::tried(
                    tx,
                    &l,
                    row.version,
                    matches!(outcome, Outcome::Invalid),
                    now_ms,
                )?)
            })
            .map_err(|e| e.to_string())?;
        match outcome {
            // The next thread can still fit the budget.
            Outcome::Refused => continue,
            Outcome::ModelFailed => break,
            Outcome::Saved => saved += 1,
            Outcome::Invalid | Outcome::Other => {}
        }
    }
    Ok(saved)
}

enum Outcome {
    /// The budget refused a call. No row stays if no call went out.
    Refused,
    Saved,
    /// The model answered, and the answer is not a valid draft.
    Invalid,
    /// No draft for another reason (the thread changed, a store error):
    /// the next try can succeed.
    Other,
    /// The model failed: the shared wait starts.
    ModelFailed,
}

/// One nudge call under the budget, as `iris write` makes it
/// (`crate::reply`): an `understand_run` row for the counts of the call,
/// and the shared model wait at its end. The caller holds the run lock.
/// Gives the outcome and the count of model calls that went out.
fn call(
    store: &Store,
    row: &ModelRow,
    model: &dyn Model,
    late: &iris_voice::Late,
    now_ms: i64,
) -> Result<(Outcome, usize), String> {
    let run = store
        .write(|tx| {
            tx.execute(
                "INSERT INTO understand_run(started_at, model, prompt, cost_micro_usd,
                                            unknown_cost_micro_usd)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![
                    now_ms,
                    row.model,
                    iris_voice::NUDGE_PROMPT,
                    row.price.is_some().then_some(0)
                ],
            )?;
            Ok(tx.last_insert_rowid())
        })
        .map_err(|e| e.to_string())?;
    let budgeted = Budgeted::new(model, store, run, Some(row));
    let result = iris_voice::nudge(store, late, &budgeted, now_ms);
    let s = budgeted.state();
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
    let failed = matches!(stop, "unknown" | "model");
    // A refusal before the call leaves no row: the step runs each turn.
    let refused = s.refused && !s.store_failed;
    let made = store
        .write(|tx| {
            let r = batch::report(tx, run)?;
            // A call with unknown usage went out too (#947).
            let made = r.calls + r.unknown_calls;
            if refused && made == 0 {
                tx.execute("DELETE FROM understand_run WHERE id = ?1", [run])?;
                return Ok(0);
            }
            let ended = now_ms.max(crate::engine::now_ms());
            tx.execute(
                "UPDATE understand_run SET ended_at = ?2, stop = ?3 WHERE id = ?1",
                params![run, ended, stop],
            )?;
            batch::update_wait(tx, failed, made > 0, ended)?;
            Ok(made.max(0) as usize)
        })
        .map_err(|e| e.to_string())?;
    if let Some(e) = budgeted.take_error() {
        return Err(format!("store: {e}"));
    }
    let outcome = match result {
        _ if refused => Outcome::Refused,
        _ if failed => Outcome::ModelFailed,
        Ok(_) => Outcome::Saved,
        Err(iris_voice::Error::BadAnswer(_)) => Outcome::Invalid,
        Err(_) => Outcome::Other,
    };
    Ok((outcome, made))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use iris_model::{Reply, Stop, Usage};
    use iris_store::{Body, Header, NewMessage};
    use iris_understand::budget::ModelRow;
    use iris_understand::Model;

    use super::*;

    /// A model that counts its calls and answers `answer`.
    struct Fake(Rc<Cell<u32>>, &'static str);

    impl Model for Fake {
        fn name(&self) -> &str {
            "fake"
        }

        fn request_bytes(&self, instruction: &str, data: &str) -> u64 {
            // The thread "t-big" costs more than any budget of the tests.
            // The thread "t-retry" does too, after the first call.
            let big = data.contains(r#""subject":"t-big""#)
                || (data.contains(r#""subject":"t-retry""#) && self.0.get() > 0);
            match big {
                true => 1 << 40,
                false => (instruction.len() + data.len()) as u64,
            }
        }

        fn ask(&self, _: &str, _: &str) -> Result<Reply, iris_model::Error> {
            self.0.set(self.0.get() + 1);
            if self.1 == "fail" {
                return Err(iris_model::Error::Server(500));
            }
            // "unknown:" gives the rest of the text with unknown usage.
            let (text, known) = match self.1.strip_prefix("unknown:") {
                Some(t) => (t, false),
                None => (self.1, true),
            };
            Ok(Reply {
                text: text.into(),
                usage: Usage {
                    input_tokens: 100,
                    output_tokens: 10,
                    cost_micro_usd: None,
                    known,
                },
                stop: Stop::End,
            })
        }
    }

    /// The count of blocked tries.
    fn blocked(store: &Store) -> i64 {
        count(store, "SELECT count(*) FROM nudge_try WHERE blocked = 1")
    }

    fn count(store: &Store, sql: &str) -> i64 {
        store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    /// A store with a priced model and no approved budget.
    fn open(name: &str) -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("iris-nudge-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let account = store.add_account("gmail", "me@example.com").unwrap();
        store
            .put_labels(account, &[("SENT", "SENT", "system")])
            .unwrap();
        store
            .write(|tx| {
                tx.execute(
                    "INSERT INTO sender(address, kind) VALUES ('ana@example.org', 'person')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO model_setting(id, provider, base_url, model, version, price_in,
                       price_out, max_tokens, timeout_s, budget_micro_usd)
                     VALUES (1, 'anthropic', 'https://api.anthropic.com/v1', 'm', 1, 3000000,
                       15000000, 1000, 60, NULL)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        (store, dir)
    }

    /// An ask that the owner sent to Ana at the time 1000 in the thread
    /// `thread`, and its commitment.
    fn ask(store: &Store, thread: &str, text: &str) {
        let ids = store
            .put_messages(
                1,
                &[NewMessage {
                    provider_id: format!("ask-{thread}"),
                    thread_provider_id: thread.into(),
                    internal_date: Some(1_000),
                    labels: vec!["SENT".into()],
                    header: Header {
                        from_addr: Some("me@example.com".into()),
                        to_addrs: Some("ana@example.org".into()),
                        ..Default::default()
                    },
                    body: Some(Body {
                        subject: thread.into(),
                        sender: "me@example.com".into(),
                        text: text.into(),
                    }),
                    ..Default::default()
                }],
            )
            .unwrap();
        store
            .write(|tx| {
                tx.execute(
                    "UPDATE message SET understood_at = 1 WHERE id = ?1",
                    [ids[0]],
                )?;
                tx.execute(
                    "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                     VALUES ('commitment', 1, 'open', json_object('direction', 'owed_to_you',
                             'owner', json_object('kind', 'sender', 'sender', 's-1')), 1, 1)",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO item_source(item_id, message_id, role)
                     VALUES (last_insert_rowid(), ?1, 'origin')",
                    [ids[0]],
                )?;
                tx.execute(
                    "INSERT INTO sender_evidence(message_id, address, rule, content_sha256)
                     SELECT id, 'ana@example.org', 'sent', content_sha256 FROM message
                      WHERE id = ?1",
                    [ids[0]],
                )?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn a_late_ask_gets_one_nudge_through_the_budget() {
        let (store, dir) = open("one");
        ask(&store, "t1", "Ana, can you send the slides?");
        let calls = Rc::new(Cell::new(0));
        let answer = Cell::new(r#"{"body": "Hi Ana, any news on the slides? D", "named": [1]}"#);
        let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> {
            Ok(Box::new(Fake(calls.clone(), answer.get())))
        };
        let late = 1_000 + iris_voice::LATE_AFTER_MS;

        // Not late yet: no call.
        assert_eq!(step(&store, &connect, late - 1), Ok(0));
        // No budget: the call is refused. No run row stays, and the try
        // does not block.
        assert_eq!(step(&store, &connect, late), Ok(0));
        assert_eq!((calls.get(), blocked(&store)), (0, 0));
        assert_eq!(count(&store, "SELECT count(*) FROM understand_run"), 0);

        // With a budget: one call, one draft with the nudge intent, and
        // the cost comes from the budget.
        store
            .write(|tx| {
                tx.execute("UPDATE model_setting SET budget_micro_usd = 1000000", [])?;
                Ok(())
            })
            .unwrap();
        // A model failure: the shared wait starts, and no mark stays, so
        // the thread gets its call after the wait.
        answer.set("fail");
        assert_eq!(step(&store, &connect, late), Ok(0));
        assert_eq!((calls.get(), blocked(&store)), (1, 0));
        let wait = count(&store, "SELECT retry_at FROM model_setting");
        assert!(wait > late);
        assert_eq!(step(&store, &connect, late), Ok(0));
        assert_eq!(calls.get(), 1, "no call during the wait");
        calls.set(0);
        // An invalid answer after the wait: one call and its one retry
        // (decision D7), then no call again for the same newest message.
        let late = wait;
        answer.set("not json");
        assert_eq!(step(&store, &connect, late), Ok(0));
        assert_eq!(step(&store, &connect, late), Ok(0));
        assert_eq!((calls.get(), blocked(&store)), (2, 1));
        // A store that the engine opens again keeps the try: no new call.
        let again = Store::open(&dir).unwrap();
        assert_eq!(step(&again, &connect, late), Ok(0));
        assert_eq!(calls.get(), 2);
        store
            .write(|tx| {
                tx.execute("DELETE FROM nudge_try", [])?;
                Ok(())
            })
            .unwrap();
        answer.set(r#"{"body": "Hi Ana, any news on the slides? D", "named": [1]}"#);
        assert_eq!(step(&store, &connect, late), Ok(1));
        assert_eq!(calls.get(), 3);
        assert_eq!(
            count(
                &store,
                &format!(
                    "SELECT count(*) FROM reply_draft WHERE intent = '{}'",
                    iris_voice::NUDGE_INTENT
                )
            ),
            1
        );
        assert!(count(&store, "SELECT budget_micro_usd FROM model_setting") < 1_000_000);
        // The draft is ready: no call again, also with no try.
        store
            .write(|tx| {
                tx.execute("DELETE FROM nudge_try", [])?;
                Ok(())
            })
            .unwrap();
        assert_eq!(step(&store, &connect, late + 60_000), Ok(0));
        assert_eq!(calls.get(), 3);

        // The lock is held: no call.
        store
            .write(|tx| {
                tx.execute("DELETE FROM reply_draft", [])?;
                Ok(())
            })
            .unwrap();
        let lock = batch::lock(&store).unwrap();
        assert_eq!(step(&store, &connect, late), Ok(0));
        drop(lock);
        assert_eq!(calls.get(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_thread_that_the_budget_cannot_pay_does_not_stop_the_others_and_a_new_model_tries_again() {
        let (store, dir) = open("order");
        ask(&store, "t-big", "Ana, can you send the deck?");
        ask(&store, "t-small", "Ana, can you send the slides?");
        store
            .write(|tx| {
                tx.execute("UPDATE model_setting SET budget_micro_usd = 1000000", [])?;
                Ok(())
            })
            .unwrap();
        let calls = Rc::new(Cell::new(0));
        let answer = Cell::new("not json");
        let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> {
            Ok(Box::new(Fake(calls.clone(), answer.get())))
        };
        let late = 1_000 + iris_voice::LATE_AFTER_MS;
        // The big thread comes first and is refused; the small one gets its
        // call and its retry. The answer is not valid: no draft.
        assert_eq!(step(&store, &connect, late), Ok(0));
        assert_eq!((calls.get(), blocked(&store)), (2, 1));
        assert_eq!(step(&store, &connect, late), Ok(0));
        assert_eq!(calls.get(), 2, "one try for one model version");
        // A new model version: the small thread gets a new call.
        store
            .write(|tx| {
                tx.execute("UPDATE model_setting SET version = 2", [])?;
                Ok(())
            })
            .unwrap();
        answer.set(r#"{"body": "Hi Ana, any news? D", "named": [1]}"#);
        assert_eq!(step(&store, &connect, late), Ok(1));
        assert_eq!(calls.get(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_step_starts_a_thread_only_when_its_retry_fits() {
        let (store, dir) = open("limit");
        for t in ["t1", "t2", "t3"] {
            ask(&store, t, "Ana, can you send the slides?");
        }
        store
            .write(|tx| {
                tx.execute("UPDATE model_setting SET budget_micro_usd = 1000000", [])?;
                Ok(())
            })
            .unwrap();
        let calls = Rc::new(Cell::new(0));
        let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> {
            Ok(Box::new(Fake(calls.clone(), "not json")))
        };
        let late = 1_000 + iris_voice::LATE_AFTER_MS;
        // The first thread gets a call and its retry. One call of the limit
        // is left, too few for a call and a retry, so the other threads
        // wait and stay open for the next step.
        assert_eq!(step(&store, &connect, late), Ok(0));
        assert_eq!((calls.get(), blocked(&store)), (2, 1));
        // The next step gives the second thread its call and its retry.
        assert_eq!(step(&store, &connect, late), Ok(0));
        assert_eq!((calls.get(), blocked(&store)), (4, 2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refused_retry_counts_the_first_call() {
        let (store, dir) = open("refused-retry");
        ask(&store, "t-retry", "Ana, can you send the deck?");
        ask(&store, "t-next", "Ana, can you send the slides?");
        store
            .write(|tx| {
                tx.execute("UPDATE model_setting SET budget_micro_usd = 1000000", [])?;
                Ok(())
            })
            .unwrap();
        let calls = Rc::new(Cell::new(0));
        let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> {
            Ok(Box::new(Fake(calls.clone(), "not json")))
        };
        let late = 1_000 + iris_voice::LATE_AFTER_MS;
        // "t-retry": one paid call, then the budget refuses the retry. The
        // run row stays with its call. "t-next": a call and its retry.
        assert_eq!(step(&store, &connect, late), Ok(0));
        assert_eq!(calls.get() as usize, MAX_PER_TURN);
        assert_eq!(
            count(&store, "SELECT coalesce(sum(calls), 0) FROM understand_run"),
            MAX_PER_TURN as i64
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn calls_of_unknown_usage_count_toward_the_turn() {
        let (store, dir) = open("unknown-turn");
        for t in ["t-a", "t-b", "t-c", "t-d"] {
            ask(&store, t, "Ana, can you send the deck?");
        }
        // A local model is free, and its server gives no usage.
        store
            .write(|tx| {
                tx.execute(
                    "UPDATE model_setting SET provider = 'local',
                       base_url = 'http://127.0.0.1:1/v1', price_in = NULL, price_out = NULL",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let calls = Rc::new(Cell::new(0));
        let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> {
            Ok(Box::new(Fake(calls.clone(), "unknown:not json")))
        };
        let late = 1_000 + iris_voice::LATE_AFTER_MS;
        // The first thread makes its call and its retry. The next thread
        // waits, because the turn has less room than two calls (#947).
        assert_eq!(step(&store, &connect, late), Ok(0));
        assert_eq!(calls.get(), 2);
        assert_eq!(
            count(
                &store,
                "SELECT coalesce(sum(unknown_calls), 0) FROM understand_run"
            ),
            2
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A priced model whose invalid answer has unknown usage: the budget
    /// refuses the retry, the run stops as `unknown`, the shared wait
    /// starts, and the step stops (review of #964).
    #[test]
    fn a_priced_call_of_unknown_usage_starts_the_wait() {
        let (store, dir) = open("unknown-priced");
        ask(&store, "t-a", "Ana, can you send the deck?");
        ask(&store, "t-b", "Ana, can you send the slides?");
        store
            .write(|tx| {
                tx.execute("UPDATE model_setting SET budget_micro_usd = 1000000", [])?;
                Ok(())
            })
            .unwrap();
        let calls = Rc::new(Cell::new(0));
        let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> {
            Ok(Box::new(Fake(calls.clone(), "unknown:not json")))
        };
        let late = 1_000 + iris_voice::LATE_AFTER_MS;
        assert_eq!(step(&store, &connect, late), Ok(0));
        assert_eq!(calls.get(), 1, "no retry and no next thread");
        assert_eq!(
            count(
                &store,
                "SELECT count(*) FROM understand_run WHERE stop = 'unknown' AND unknown_calls = 1"
            ),
            1
        );
        assert_eq!(
            count(&store, "SELECT retry_at IS NOT NULL FROM model_setting"),
            1,
            "the shared wait starts"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
