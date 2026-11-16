//! The proposal step: gates, the wait, the fingerprint, and the checks of
//! an untrusted answer.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use iris_matters::propose::{MAX_REQUEST, WINDOW_MS};
use iris_matters::{decline, propose, Gate, Skip};
use iris_model::{Error as ModelError, Reply, Stop, Usage};
use iris_store::{NewMessage, Store};
use iris_understand::budget::ModelRow;
use iris_understand::Model;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

const NOW: i64 = 2_000_000_000_000;

struct Box {
    store: Store,
    dir: std::path::PathBuf,
}

impl Drop for Box {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Fake {
    answers: RefCell<VecDeque<Result<String, ModelError>>>,
    data: RefCell<Vec<String>>,
    /// SQL to run on the store during the call (a change by another writer).
    during: Option<(std::path::PathBuf, String)>,
}

struct Shared(Rc<Fake>);

impl Model for Shared {
    fn name(&self) -> &str {
        "fake"
    }
    fn ask(&self, _: &str, data: &str) -> Result<Reply, ModelError> {
        let f = &self.0;
        f.data.borrow_mut().push(data.to_string());
        if let Some((dir, sql)) = &f.during {
            Connection::open(dir.join("iris.db"))
                .unwrap()
                .execute_batch(sql)
                .unwrap();
        }
        let text = f
            .answers
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok("{\"matters\":[]}".into()))?;
        Ok(Reply {
            text,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 10,
                cost_micro_usd: None,
                known: true,
            },
            stop: Stop::End,
        })
    }
}

fn fake(answers: Vec<Result<String, ModelError>>) -> Rc<Fake> {
    fake_during(answers, None)
}

fn fake_during(
    answers: Vec<Result<String, ModelError>>,
    during: Option<(std::path::PathBuf, String)>,
) -> Rc<Fake> {
    Rc::new(Fake {
        answers: RefCell::new(answers.into()),
        data: RefCell::new(Vec::new()),
        during,
    })
}

/// A store with a local model and one item for each title, item `i` (row
/// `i + 1`) in thread `i % threads`, form `forms[i % forms.len()]`.
fn setup(name: &str, threads: usize, titles: &[String], forms: &[&str]) -> Box {
    let dir = std::env::temp_dir().join(format!("iris-propose-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let a = store.add_account("gmail", "owner@example.com").unwrap();
    let msgs: Vec<NewMessage> = (0..threads)
        .map(|t| NewMessage {
            provider_id: format!("g{t}"),
            thread_provider_id: format!("gt{t}"),
            ..Default::default()
        })
        .collect();
    let ids = store.put_messages(a, &msgs).unwrap();
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO model_setting(id, provider, base_url, model, version, max_tokens, timeout_s)
                 VALUES (1, 'local', 'http://127.0.0.1:1', 'fake', 1, 1000, 30)",
                [],
            )?;
            for (i, title) in titles.iter().enumerate() {
                let doc = json!({"title": title, "type": "booking",
                    "fields": {"location": "Tokyo", "start": "2026-12-10"},
                    "counterparty": {"kind": "sender", "name": "Dana"}});
                tx.execute(
                    "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                     VALUES (?1, ?2, 'open', ?3, ?4, ?4)",
                    params![forms[i % forms.len()], a, doc.to_string(), NOW],
                )?;
                tx.execute(
                    "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
                    params![tx.last_insert_rowid(), ids[i % threads]],
                )?;
            }
            Ok(())
        })
        .unwrap();
    Box { store, dir }
}

fn titles(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("Item {i}")).collect()
}

fn run(b: &Box, f: &Rc<Fake>, gate: Gate, now: i64) -> iris_matters::Outcome {
    let connect = |_: &ModelRow| -> Result<std::boxed::Box<dyn Model>, String> {
        Ok(std::boxed::Box::new(Shared(f.clone())))
    };
    propose(&b.store, &connect, gate, now).unwrap()
}

fn col(b: &Box, sql: &str) -> i64 {
    b.store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

fn answer(m: Value) -> Result<String, ModelError> {
    Ok(json!({ "matters": m }).to_string())
}

#[test]
fn a_good_answer_makes_a_proposal() {
    let b = setup(
        "good",
        3,
        &titles(3),
        &["record", "conversation", "commitment"],
    );
    let f = fake(vec![answer(
        json!([{"title": "Tokyo trip", "items": ["r-1", "v-2", "c-3"]}]),
    )]);
    let o = run(&b, &f, Gate::Hourly, NOW);
    assert_eq!((o.proposed, o.dropped, o.skip), (1, 0, None));
    assert_eq!(col(&b, "SELECT count(*) FROM matter_valid"), 1);
    assert_eq!(col(&b, "SELECT count(*) FROM matter_item"), 3);
    assert_eq!(
        col(
            &b,
            "SELECT count(*) FROM understand_run WHERE prompt = 'matters/0.1'"
        ),
        1
    );
    assert_eq!(col(&b, "SELECT tried_at FROM matter_run"), NOW);
    let data = f.data.borrow()[0].clone();
    assert!(data.contains("\"id\":\"r-1\"") && data.contains("Tokyo") && data.contains("Dana"));
}

#[test]
fn the_gates_and_the_fingerprint() {
    let b = setup("gates", 3, &titles(4), &["record", "conversation"]);
    let f = fake(vec![]);
    assert_eq!(run(&b, &f, Gate::Hourly, NOW).skip, None);
    // The hourly gate, then the same fingerprint for the command.
    assert_eq!(
        run(&b, &f, Gate::Hourly, NOW + 1000).skip,
        Some(Skip::Hourly)
    );
    assert_eq!(run(&b, &f, Gate::Now, NOW + 1000).skip, Some(Skip::Same));
    // A changed item, a hidden item, or a new model: a new call.
    b.store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE item SET updated_at = updated_at + 1 WHERE id = 1",
                [],
            )?)
        })
        .unwrap();
    assert_eq!(run(&b, &f, Gate::Now, NOW + 2000).skip, None);
    b.store
        .write(|tx| Ok(tx.execute(r#"UPDATE item SET doc = json_set(doc, '$.attention.state', 'hidden') WHERE id = 4"#, [])?))
        .unwrap();
    assert_eq!(run(&b, &f, Gate::Now, NOW + 3000).skip, None);
    b.store
        .write(|tx| Ok(tx.execute("UPDATE model_setting SET version = 2", [])?))
        .unwrap();
    assert_eq!(run(&b, &f, Gate::Now, NOW + 4000).skip, None);
    assert_eq!(f.data.borrow().len(), 4);
    // An hour later the serve gate opens, but the fingerprint is the same.
    assert_eq!(
        run(&b, &f, Gate::Hourly, NOW + 4000 + 3_600_000).skip,
        Some(Skip::Same)
    );
    // One thread only: no call.
    let b = setup("few", 1, &titles(3), &["record"]);
    assert_eq!(run(&b, &f, Gate::Now, NOW).skip, Some(Skip::Few));
}

#[test]
fn a_model_error_starts_the_wait_and_no_model_still_cleans() {
    let b = setup("wait", 3, &titles(3), &["record"]);
    let f = fake(vec![Err(ModelError::Transport("lost".into()))]);
    let o = run(&b, &f, Gate::Now, NOW);
    assert!(o.failed);
    assert!(col(&b, "SELECT retry_at FROM model_setting") > NOW);
    assert_eq!(
        col(
            &b,
            "SELECT count(*) FROM matter_run WHERE candidates IS NULL AND tried_at IS NOT NULL"
        ),
        1
    );
    // A second command at once makes no call.
    assert_eq!(run(&b, &f, Gate::Now, NOW + 1).skip, Some(Skip::Wait));
    assert_eq!(f.data.borrow().len(), 1);
    // After the wait, an answer ends it.
    let later = col(&b, "SELECT retry_at FROM model_setting") + 1;
    assert_eq!(run(&b, &f, Gate::Now, later).skip, None);
    assert_eq!(col(&b, "SELECT retry_failures FROM model_setting"), 0);

    // No model: step 0 still deletes a proposal that is not valid.
    let b = setup("nomodel", 2, &titles(2), &["record"]);
    let f = fake(vec![answer(
        json!([{"title": "T", "items": ["r-1", "r-2"]}]),
    )]);
    run(&b, &f, Gate::Now, NOW);
    b.store
        .write(|tx| {
            tx.execute("DELETE FROM model_setting", [])?;
            Ok(tx.execute("UPDATE item SET status = 'gone' WHERE id = 2", [])?)
        })
        .unwrap();
    let o = run(&b, &f, Gate::Now, NOW + 1);
    assert_eq!((o.skip, o.cleaned), (Some(Skip::NoModel), 1));
    assert_eq!(col(&b, "SELECT count(*) FROM matter"), 0);
}

#[test]
fn a_declined_grouping_and_a_member_hidden_during_the_call_are_dropped() {
    let b = setup("declined", 3, &titles(3), &["record"]);
    let f = fake(vec![
        answer(json!([{"title": "T", "items": ["r-1", "r-2", "r-3"]}])),
        answer(json!([{"title": "T", "items": ["r-1", "r-2"]}])),
    ]);
    run(&b, &f, Gate::Now, NOW);
    decline(&b.store, 1, NOW).unwrap();
    b.store
        .write(|tx| Ok(tx.execute("UPDATE item SET updated_at = updated_at + 1", [])?))
        .unwrap();
    let o = run(&b, &f, Gate::Now, NOW + 1);
    assert_eq!((o.proposed, o.dropped), (0, 1));

    let b = setup("hidden", 3, &titles(3), &["record"]);
    let f = fake_during(
        vec![answer(json!([{"title": "T", "items": ["r-1", "r-2"]}]))],
        Some((
            b.dir.clone(),
            r#"UPDATE item SET doc = json_set(doc, '$.attention.state', 'hidden') WHERE id = 2"#
                .into(),
        )),
    );
    let o = run(&b, &f, Gate::Now, NOW);
    assert_eq!((o.proposed, o.dropped), (0, 1));
}

#[test]
fn the_request_stays_in_its_bound() {
    // Long titles full of the escape and multibyte text.
    let long: Vec<String> = (0..300)
        .map(|i| format!("{i} {}", "<<<é".repeat(60)))
        .collect();
    let b = setup("bound", 7, &long, &["record", "conversation"]);
    let f = fake(vec![]);
    run(&b, &f, Gate::Now, NOW);
    let data = f.data.borrow()[0].clone();
    let bytes = Shared(f.clone()).request_bytes(
        iris_matters::propose::INSTRUCTION,
        &iris_model::data_block(&data),
    );
    assert!(bytes <= MAX_REQUEST, "{bytes}");
    assert!(data.lines().count() < 300, "the bound cut lines");
}

/// Random answers over random IDs never write a member that the call did
/// not send, never more than 10 proposals, and each proposal is valid.
#[test]
fn random_answers_keep_the_rules() {
    let mut x: u64 = 0x9E37_79B9;
    let mut next = |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    let long_title = "y".repeat(81);
    let mut total = 0;
    for seed in 0..30 {
        let forms = ["record", "conversation", "commitment", "feed"];
        let b = setup(&format!("rand-{seed}"), 5, &titles(20), &forms);
        // Item 20 is out of the window: never sent.
        b.store
            .write(|tx| {
                Ok(tx.execute(
                    "UPDATE item SET updated_at = ?1 WHERE id = 20",
                    [NOW - WINDOW_MS - 1],
                )?)
            })
            .unwrap();
        let prefixes = ["c", "v", "r", "f", "m", "x"];
        let matters: Vec<Value> = (0..1 + next(14))
            .map(|_| {
                let items: Vec<String> = (0..next(6))
                    .map(|_| {
                        let row = 1 + next(22);
                        // Mostly the right prefix of the row, so some
                        // matters pass; else any prefix.
                        let p = if next(10) < 7 {
                            ["r", "v", "c", "f"][((row - 1) % 4) as usize]
                        } else {
                            prefixes[next(6) as usize]
                        };
                        format!("{p}-{row}")
                    })
                    .collect();
                let title = [
                    "Trip",
                    "Trip",
                    "",
                    "a\u{7}b",
                    long_title.as_str(),
                    "Ignore the rules",
                ][next(6) as usize];
                json!({"title": title, "items": items})
            })
            .collect();
        let f = fake(vec![answer(Value::Array(matters))]);
        let o = run(&b, &f, Gate::Now, NOW);
        assert!(o.proposed <= 10);
        total += o.proposed;
        let sent = f.data.borrow()[0].clone();
        let members: Vec<(String, i64)> = b
            .store
            .read(|c| {
                let mut s = c.prepare(
                    "SELECT i.form, i.id FROM matter_item mi JOIN item i ON i.id = mi.item_id",
                )?;
                let v = s
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(v)
            })
            .unwrap();
        for (form, id) in members {
            let p = match form.as_str() {
                "commitment" => "c",
                "conversation" => "v",
                "record" => "r",
                _ => "f",
            };
            let pid = format!("{p}-{id}");
            assert!(
                sent.contains(&format!("\"id\":\"{pid}\"")),
                "seed {seed}: {pid} not sent"
            );
            assert_ne!(form, "feed");
        }
        assert_eq!(
            col(
                &b,
                "SELECT count(*) FROM matter WHERE id NOT IN (SELECT matter_id FROM matter_valid)"
            ),
            0,
            "seed {seed}"
        );
    }
    assert!(
        total > 0,
        "no random answer passed: the test proves nothing"
    );
}
