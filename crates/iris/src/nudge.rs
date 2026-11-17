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

/// The threads and newest messages that a nudge call was made for. A
/// thread gets one call for each newest message, also if the call failed,
/// so an invalid answer does not cost a call each turn.
/// shortcut: the set is in memory, so a restart of the engine gives each late thread one more call; store it if restarts are frequent.
pub type Tried = HashSet<(i64, i64)>;

/// One nudge step: a call for each late thread ([`iris_voice::late`]) that
/// `tried` does not have, at most [`MAX_PER_TURN`]. No call when another
/// run holds the lock, when no model is set, or during the wait after a
/// model error. A budget refusal stops the step and leaves no row and no
/// mark, so the thread gets its call when a budget is approved. Gives the
/// count of saved drafts.
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
    let late: Vec<_> = late.into_iter().filter(|k| !tried.contains(k)).collect();
    if late.is_empty() || wait.is_some_and(|t| t > now_ms) {
        return Ok(0);
    }
    let model = connect(&row)?;
    let mut saved = 0;
    for (thread, newest) in late.into_iter().take(MAX_PER_TURN) {
        match call(store, &row, model.as_ref(), thread, now_ms)? {
            Outcome::Refused => break,
            Outcome::Saved => saved += 1,
            Outcome::Failed => {}
            Outcome::ModelFailed => {
                tried.insert((thread, newest));
                break;
            }
        }
        tried.insert((thread, newest));
    }
    Ok(saved)
}

enum Outcome {
    /// The budget refused the call. No row stays.
    Refused,
    Saved,
    /// No draft, and no evidence against the model.
    Failed,
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
    thread: i64,
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
                    iris_voice::PROMPT,
                    row.price.is_some().then_some(0)
                ],
            )?;
            Ok(tx.last_insert_rowid())
        })
        .map_err(|e| e.to_string())?;
    let budgeted = Budgeted::new(model, store, run, Some(row));
    let result = iris_voice::nudge(store, thread, &budgeted, now_ms);
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

        fn ask(&self, _: &str, _: &str) -> Result<Reply, iris_model::Error> {
            self.0.set(self.0.get() + 1);
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

    #[test]
    fn a_late_ask_gets_one_nudge_through_the_budget() {
        let dir = std::env::temp_dir().join(format!("iris-nudge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let account = store.add_account("gmail", "me@example.com").unwrap();
        store
            .put_labels(account, &[("SENT", "SENT", "system")])
            .unwrap();
        let ids = store
            .put_messages(
                account,
                &[NewMessage {
                    provider_id: "ask".into(),
                    thread_provider_id: "t1".into(),
                    internal_date: Some(1_000),
                    labels: vec!["SENT".into()],
                    header: Header {
                        from_addr: Some("me@example.com".into()),
                        to_addrs: Some("ana@example.org".into()),
                        ..Default::default()
                    },
                    body: Some(Body {
                        subject: "Slides".into(),
                        sender: "me@example.com".into(),
                        text: "Ana, can you send the slides?".into(),
                    }),
                    ..Default::default()
                }],
            )
            .unwrap();
        store
            .write(|tx| {
                tx.execute("UPDATE message SET understood_at = 1", [])?;
                tx.execute(
                    "INSERT INTO sender(address, kind) VALUES ('ana@example.org', 'person')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                     VALUES ('commitment', 1, 'open', json_object('direction', 'owed_to_you',
                             'owner', json_object('kind', 'sender', 'sender', 's-1')), 1, 1)",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO item_source(item_id, message_id, role) VALUES (1, ?1, 'origin')",
                    [ids[0]],
                )?;
                // A priced model with no approved budget.
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
        // An invalid answer: one call, then no call again for the same
        // newest message.
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
}
