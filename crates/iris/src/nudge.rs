//! Nudges (card R3-07): when a person is late on an item owed to you, the
//! engine prepares a reminder in your voice ([`iris_voice::write`] with
//! [`iris_voice::NUDGE_INTENT`]), through the one budget of understanding.
//! It sends nothing: the draft stays in the store until the person acts.

use std::collections::HashSet;

use iris_store::Store;
use iris_understand::batch;
use iris_understand::budget::{model_row, Budgeted, ModelRow};
use iris_understand::Model;
use rusqlite::params;

use crate::reply::Connect;

/// The most nudge calls in one turn of the understanding thread.
pub const MAX_PER_TURN: usize = 3;

/// The late threads ([`iris_voice::Late`]) that a nudge call was made for,
/// with the version of the model setting. A thread gets one call for each
/// newest message, person, commitment and model version,
/// also if the answer was not valid, so an invalid answer does not cost a
/// call each turn. A model failure marks nothing: the shared wait governs.
/// A change during the call marks nothing: the change gives a new chance.
/// shortcut: the set is in memory, so a restart of the engine gives each late thread one more call; store it if restarts are frequent.
pub type Tried = HashSet<(iris_voice::Late, i64)>;

/// One nudge step: a call for each late thread ([`iris_voice::late`]) that
/// `tried` does not have, at most [`MAX_PER_TURN`] calls. No call when
/// another run holds the lock, when no model is set, or during the wait
/// after a model error. A budget refusal leaves no row and no mark, so the
/// thread gets its call when the budget can pay it, and the step goes on
/// with the next thread. Gives the count of saved drafts.
pub fn step(
    store: &Store,
    connect: Connect,
    tried: &mut Tried,
    now_ms: i64,
) -> Result<usize, String> {
    let _lock = match batch::lock(store) {
        Ok(l) => l,
        Err(batch::Error::Busy) => return Ok(0),
        Err(e) => return Err(e.to_string()),
    };
    let (row, wait, late) = store
        .read(|c| {
            Ok((
                model_row(c)?,
                batch::retry_after(c)?,
                iris_voice::late(c, now_ms)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let Some(row) = row else { return Ok(0) };
    let late: Vec<_> = late
        .into_iter()
        .map(|l| (l, row.version))
        .filter(|k| !tried.contains(k))
        .collect();
    if late.is_empty() || wait.is_some_and(|t| t > now_ms) {
        return Ok(0);
    }
    let model = connect(&row)?;
    let mut saved = 0;
    let mut calls = 0;
    for key in late {
        if calls == MAX_PER_TURN {
            break;
        }
        // A refusal or a model failure marks nothing: the budget and the
        // shared wait decide when the thread gets its call. A refusal
        // makes no call, so the next thread can still fit the budget.
        match call(store, &row, model.as_ref(), &key.0, now_ms)? {
            Outcome::Refused => continue,
            Outcome::ModelFailed => break,
            Outcome::Changed => {}
            Outcome::Saved => {
                saved += 1;
                tried.insert(key);
            }
            Outcome::Failed => {
                tried.insert(key);
            }
        }
        calls += 1;
    }
    Ok(saved)
}

enum Outcome {
    /// The budget refused the call. No row stays.
    Refused,
    Saved,
    /// No draft, and no evidence against the model.
    Failed,
    /// The thread or the commitment changed during the call. No mark.
    Changed,
    /// The model failed: the shared wait starts.
    ModelFailed,
}

/// One nudge call under the budget, as `iris write` makes it
/// (`crate::reply`): an `understand_run` row for the counts of the call,
/// and the shared model wait at its end. The caller holds the run lock.
fn call(
    store: &Store,
    row: &ModelRow,
    model: &dyn Model,
    late: &iris_voice::Late,
    now_ms: i64,
) -> Result<Outcome, String> {
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
    store
        .write(|tx| {
            let r = batch::report(tx, run)?;
            if refused && r.calls == 0 {
                tx.execute("DELETE FROM understand_run WHERE id = ?1", [run])?;
                return Ok(());
            }
            let ended = now_ms.max(crate::engine::now_ms());
            tx.execute(
                "UPDATE understand_run SET ended_at = ?2, stop = ?3 WHERE id = ?1",
                params![run, ended, stop],
            )?;
            batch::update_wait(tx, failed, r.calls > 0, ended)?;
            Ok(())
        })
        .map_err(|e| e.to_string())?;
    if let Some(e) = budgeted.take_error() {
        return Err(format!("store: {e}"));
    }
    Ok(match result {
        _ if refused => Outcome::Refused,
        _ if failed => Outcome::ModelFailed,
        Ok(_) => Outcome::Saved,
        // The mail or the commitment changed during the call: the next
        // turn reads the thread again.
        Err(iris_voice::Error::Changed) => Outcome::Changed,
        Err(_) => Outcome::Failed,
    })
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
            match data.contains(r#""subject":"t-big""#) {
                true => 1 << 40,
                false => (instruction.len() + data.len()) as u64,
            }
        }

        fn ask(&self, _: &str, _: &str) -> Result<Reply, iris_model::Error> {
            self.0.set(self.0.get() + 1);
            if self.1 == "fail" {
                return Err(iris_model::Error::Server(500));
            }
            Ok(Reply {
                text: self.1.into(),
                usage: Usage {
                    input_tokens: 100,
                    output_tokens: 10,
                    cost_micro_usd: None,
                    known: true,
                },
                stop: Stop::End,
            })
        }
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
        let answer = Cell::new(r#"{"body": "Hi Ana, any news on the slides? D"}"#);
        let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> {
            Ok(Box::new(Fake(calls.clone(), answer.get())))
        };
        let late = 1_000 + iris_voice::LATE_AFTER_MS;
        let mut tried = Tried::new();

        // Not late yet: no call.
        assert_eq!(step(&store, &connect, &mut tried, late - 1), Ok(0));
        // No budget: the call is refused, and no row and no mark stay.
        assert_eq!(step(&store, &connect, &mut tried, late), Ok(0));
        assert_eq!((calls.get(), tried.len()), (0, 0));
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
        assert_eq!(step(&store, &connect, &mut tried, late), Ok(0));
        assert_eq!((calls.get(), tried.len()), (1, 0));
        let wait = count(&store, "SELECT retry_at FROM model_setting");
        assert!(wait > late);
        assert_eq!(step(&store, &connect, &mut tried, late), Ok(0));
        assert_eq!(calls.get(), 1, "no call during the wait");
        calls.set(0);
        // An invalid answer after the wait: one call, then no call again
        // for the same newest message.
        let late = wait;
        answer.set("not json");
        assert_eq!(step(&store, &connect, &mut tried, late), Ok(0));
        assert_eq!(step(&store, &connect, &mut tried, late), Ok(0));
        assert_eq!(calls.get(), 1);
        tried.clear();
        answer.set(r#"{"body": "Hi Ana, any news on the slides? D"}"#);
        assert_eq!(step(&store, &connect, &mut tried, late), Ok(1));
        assert_eq!(calls.get(), 2);
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
        // The draft is ready: no call again, also with an empty set.
        tried.clear();
        assert_eq!(step(&store, &connect, &mut tried, late + 60_000), Ok(0));
        assert_eq!(calls.get(), 2);

        // The lock is held: no call.
        store
            .write(|tx| {
                tx.execute("DELETE FROM reply_draft", [])?;
                Ok(())
            })
            .unwrap();
        let lock = batch::lock(&store).unwrap();
        assert_eq!(step(&store, &connect, &mut tried, late), Ok(0));
        drop(lock);
        assert_eq!(calls.get(), 2);
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
        let mut tried = Tried::new();
        // The big thread comes first and is refused; the small one gets its
        // call. The answer is not valid: no draft.
        assert_eq!(step(&store, &connect, &mut tried, late), Ok(0));
        assert_eq!((calls.get(), tried.len()), (1, 1));
        assert_eq!(step(&store, &connect, &mut tried, late), Ok(0));
        assert_eq!(calls.get(), 1, "one call for one model version");
        // A new model version: the small thread gets a new call.
        store
            .write(|tx| {
                tx.execute("UPDATE model_setting SET version = 2", [])?;
                Ok(())
            })
            .unwrap();
        answer.set(r#"{"body": "Hi Ana, any news? D"}"#);
        assert_eq!(step(&store, &connect, &mut tried, late), Ok(1));
        assert_eq!(calls.get(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
