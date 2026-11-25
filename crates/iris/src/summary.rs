//! Thread summaries (issue 478, `docs/specs/2026-11-25-thread-summary.md`):
//! for a long thread of an item that needs the person, the engine prepares
//! a short summary ([`iris_summary::summarize`]), through the one budget of
//! understanding. It sends nothing and writes nothing to a provider.

use iris_store::Store;
use iris_understand::batch;
use iris_understand::budget::model_row;

use crate::nudge::{call, Done, Outcome};
use crate::reply::Connect;

/// The most summary calls in one turn of the understanding thread: one
/// summary and its retry, so a summary never takes the room of
/// understanding.
pub const MAX_PER_TURN: usize = 2;

/// The most candidate threads that one step reads.
pub const PAGE: usize = 20;

/// One summary step, after the nudge step: a call for each candidate of
/// one page of [`iris_summary::candidates`] (the oldest try first), at most
/// [`MAX_PER_TURN`] calls. A thread starts only when its two calls (a call
/// and one retry) fit in the rest of the limit. No call when another run
/// holds the lock, when no model is set, or during the wait after a model
/// error. Each try is in the store ([`iris_summary::tried`]): it moves the
/// thread to the end of the order, so a thread that the budget cannot pay
/// does not stop the others. Only an answer that is not a valid summary
/// blocks the same basis and model version. Gives the count of saved
/// summaries.
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
    let items = iris_api::needs_you_sql(now_ms);
    let candidates = store
        .read(|c| Ok(iris_summary::candidates(c, &items, PAGE)?))
        .map_err(|e| e.to_string())?;
    if candidates.is_empty() {
        return Ok(0);
    }
    let model = connect(&row)?;
    let mut saved = 0;
    let mut calls = 0;
    for cand in candidates {
        if MAX_PER_TURN - calls < 2 {
            break;
        }
        let (outcome, made) = call(
            store,
            &row,
            model.as_ref(),
            iris_summary::PROMPT,
            now_ms,
            |m| match iris_summary::summarize(store, &cand, m, now_ms) {
                Ok(_) => Done::Saved,
                Err(iris_summary::Error::BadAnswer(_)) => Done::Invalid,
                Err(_) => Done::Other,
            },
        )?;
        calls += made;
        store
            .write(|tx| {
                Ok(iris_summary::tried(
                    tx,
                    &cand,
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use iris_model::{Reply, Stop, Usage};
    use iris_store::{Body, Header, NewMessage};
    use iris_understand::budget::ModelRow;
    use iris_understand::Model;

    use super::*;

    const GOOD: &str = r#"{"summary": "Ana asked for the venue. You must book it."}"#;

    /// A model that counts its calls and answers `answer`.
    struct Fake(Rc<Cell<u32>>, &'static str);

    impl Model for Fake {
        fn name(&self) -> &str {
            "fake"
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

    fn exec(store: &Store, sql: &str) {
        store
            .write(|tx| {
                tx.execute(sql, [])?;
                Ok(())
            })
            .unwrap();
    }

    /// A store with a priced model and a budget, a known person Ana and a
    /// stranger, and no thread yet.
    fn open(name: &str) -> (Store, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("iris-summary-step-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let account = store.add_account("gmail", "me@example.com").unwrap();
        store
            .put_labels(
                account,
                &[("SENT", "SENT", "system"), ("INBOX", "INBOX", "system")],
            )
            .unwrap();
        exec(
            &store,
            "INSERT INTO sender(address, kind) VALUES ('ana@example.org', 'person'),
                                                      ('zed@example.org', 'stranger')",
        );
        exec(
            &store,
            "INSERT INTO model_setting(id, provider, base_url, model, version, price_in,
               price_out, max_tokens, timeout_s, budget_micro_usd)
             VALUES (1, 'anthropic', 'https://api.anthropic.com/v1', 'm', 1, 3000000,
               15000000, 1000, 60, 1000000)",
        );
        (store, dir)
    }

    /// A thread `thread` of `n` messages, understood, and an open commitment
    /// that you owe to the sender row `counter` (`s-1` Ana, `s-2` a
    /// stranger), from its first message.
    fn thread(store: &Store, thread: &str, n: usize, counter: &str) {
        let msgs: Vec<NewMessage> = (0..n)
            .map(|i| NewMessage {
                provider_id: format!("{thread}-{i}"),
                thread_provider_id: thread.into(),
                internal_date: Some(1_000 + i as i64),
                labels: vec!["INBOX".into()],
                header: Header {
                    from_addr: Some("ana@example.org".into()),
                    ..Default::default()
                },
                body: Some(Body {
                    subject: thread.into(),
                    sender: "ana@example.org".into(),
                    text: format!("Message {i} about the venue."),
                }),
                ..Default::default()
            })
            .collect();
        store.put_messages(1, &msgs).unwrap();
        store
            .write(|tx| {
                tx.execute(
                    "UPDATE message SET understood_at = 1 WHERE id IN (SELECT message_id FROM body)",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                     VALUES ('commitment', 1, 'open', json_object('direction', 'you_owe',
                             'title', 'Book the venue',
                             'counterparty', json_object('kind', 'sender', 'sender', ?1)), 1, 1)",
                    [counter],
                )?;
                tx.execute(
                    "INSERT INTO item_source(item_id, message_id, role)
                     SELECT last_insert_rowid(), id, 'origin' FROM message WHERE provider_id = ?1",
                    [format!("{thread}-0")],
                )?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn a_long_thread_of_an_item_that_needs_you_gets_one_summary_through_the_budget() {
        let (store, _dir) = open("one");
        thread(&store, "t1", 5, "s-1");
        thread(&store, "t2", 4, "s-1");
        thread(&store, "t3", 6, "s-2");
        let calls = Rc::new(Cell::new(0));
        let answer = Cell::new(GOOD);
        let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> {
            Ok(Box::new(Fake(calls.clone(), answer.get())))
        };
        // Only t1: t2 is short, and the item of t3 is at the stranger gate.
        assert_eq!(step(&store, &connect, 10_000), Ok(1));
        assert_eq!(calls.get(), 1);
        assert_eq!(count(&store, "SELECT count(*) FROM thread_summary"), 1);
        assert_eq!(
            count(
                &store,
                &format!(
                    "SELECT count(*) FROM understand_run WHERE prompt = '{}'",
                    iris_summary::PROMPT
                )
            ),
            1
        );
        assert!(count(&store, "SELECT budget_micro_usd FROM model_setting") < 1_000_000);
        // The summary is ready: no new call.
        assert_eq!(step(&store, &connect, 20_000), Ok(0));
        assert_eq!(calls.get(), 1);
    }

    /// While a draft or a question waits for the lock (#279), the step
    /// takes no lock and makes no call. One step makes the calls of one
    /// thread only (`MAX_PER_TURN`), so it needs no stop in its loop.
    #[test]
    fn a_step_does_not_start_while_a_waiter_waits() {
        let (store, dir) = open("want");
        thread(&store, "t1", 5, "s-1");
        let calls = Rc::new(Cell::new(0));
        let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> {
            Ok(Box::new(Fake(calls.clone(), GOOD)))
        };
        let want = iris_store::FileLock::try_take(&dir.join(batch::WANT_FILE))
            .unwrap()
            .unwrap();
        assert_eq!(step(&store, &connect, 10_000), Ok(0));
        assert_eq!(calls.get(), 0);
        drop(want);
        assert_eq!(step(&store, &connect, 10_000), Ok(1));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn an_invalid_answer_twice_stops_the_calls_and_a_refusal_or_a_failure_does_not() {
        let (store, _dir) = open("invalid");
        thread(&store, "t1", 5, "s-1");
        let calls = Rc::new(Cell::new(0));
        let answer = Cell::new("not json");
        let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> {
            Ok(Box::new(Fake(calls.clone(), answer.get())))
        };

        // No budget: the call is refused. No run row stays, and the try
        // does not block.
        exec(&store, "UPDATE model_setting SET budget_micro_usd = NULL");
        assert_eq!(step(&store, &connect, 10_000), Ok(0));
        assert_eq!(calls.get(), 0);
        assert_eq!(count(&store, "SELECT count(*) FROM understand_run"), 0);
        assert_eq!(
            count(
                &store,
                "SELECT count(*) FROM thread_summary_try WHERE invalid = 1"
            ),
            0
        );
        exec(
            &store,
            "UPDATE model_setting SET budget_micro_usd = 1000000",
        );

        // A model failure: the shared wait starts, and the try does not block.
        answer.set("fail");
        assert_eq!(step(&store, &connect, 10_000), Ok(0));
        assert_eq!(calls.get(), 1);
        let wait = count(&store, "SELECT retry_at FROM model_setting");
        assert!(wait > 10_000);
        assert_eq!(step(&store, &connect, 10_000), Ok(0));
        assert_eq!(calls.get(), 1, "no call during the wait");

        // An invalid answer: one call and its retry, then no third call.
        answer.set("not json");
        assert_eq!(step(&store, &connect, wait), Ok(0));
        assert_eq!(calls.get(), 3);
        assert_eq!(
            count(
                &store,
                "SELECT count(*) FROM thread_summary_try WHERE invalid = 1"
            ),
            1
        );
        assert_eq!(step(&store, &connect, wait + 1), Ok(0));
        assert_eq!(calls.get(), 3);

        // The lock is held: no call, also with a new model version.
        exec(&store, "UPDATE model_setting SET version = 2");
        answer.set(GOOD);
        let lock = batch::lock(&store).unwrap();
        assert_eq!(step(&store, &connect, wait + 2), Ok(0));
        assert_eq!(calls.get(), 3);
        drop(lock);
        assert_eq!(step(&store, &connect, wait + 3), Ok(1));
        assert_eq!(calls.get(), 4);
    }

    #[test]
    fn no_model_no_call() {
        let (store, _dir) = open("no-model");
        thread(&store, "t1", 5, "s-1");
        exec(&store, "DELETE FROM model_setting");
        let calls = Rc::new(Cell::new(0));
        let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> {
            Ok(Box::new(Fake(calls.clone(), GOOD)))
        };
        assert_eq!(step(&store, &connect, 10_000), Ok(0));
        assert_eq!(calls.get(), 0);
    }
}
