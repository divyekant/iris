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
    /// The stop of each answer.
    stop: Stop,
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
            stop: f.stop,
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
        stop: Stop::End,
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
            "SELECT count(*) FROM understand_run WHERE prompt = 'matters/0.2'"
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
    // The bound of the texts of the call itself: `ask` gets `data`, and the
    // client makes the data block.
    let bytes = Shared(f.clone()).request_bytes(iris_matters::propose::INSTRUCTION, &data);
    assert!(bytes <= MAX_REQUEST, "{bytes}");
    assert!(data.lines().count() < 300, "the bound cut lines");
    // The step stops only when one more line does not fit: the room that
    // is left is less than the bound of the longest line.
    let longest = data.lines().map(|l| l.len() + 1).max().unwrap() as u64;
    assert!(MAX_REQUEST - bytes < 6 * (longest + 8), "{bytes}");
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

/// The outcome of the call decides the writes: a call that the budget
/// refuses leaves no attempt, a bad answer starts the wait, and only a
/// complete answer makes proposals.
#[test]
fn the_outcome_of_the_call_decides_the_writes() {
    // A paid model with no approved budget: no call goes out.
    let b = setup("budget", 2, &titles(2), &["record"]);
    b.store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE model_setting SET provider = 'anthropic', price_in = 1, price_out = 1",
                [],
            )?)
        })
        .unwrap();
    let good = || answer(json!([{"title": "T", "items": ["r-1", "r-2"]}]));
    let f = fake(vec![good()]);
    let o = run(&b, &f, Gate::Hourly, NOW);
    assert_eq!((o.skip, o.failed), (Some(Skip::Budget), false));
    assert!(f.data.borrow().is_empty());
    assert_eq!(
        col(
            &b,
            "SELECT count(*) FROM matter_run WHERE tried_at IS NOT NULL"
        ),
        0
    );
    assert_eq!(col(&b, "SELECT count(*) FROM understand_run"), 0);
    // The person approves a budget: the hourly step calls at once.
    b.store
        .write(|tx| Ok(tx.execute("UPDATE model_setting SET budget_micro_usd = 1000000", [])?))
        .unwrap();
    let o = run(&b, &f, Gate::Hourly, NOW + 1);
    assert_eq!((o.skip, o.proposed), (None, 1));

    // A malformed answer of a local model starts the wait.
    let b = setup("bad", 2, &titles(2), &["record"]);
    let f = fake(vec![Err(ModelError::BadAnswer("no content".into()))]);
    assert!(run(&b, &f, Gate::Now, NOW).failed);
    assert_eq!(run(&b, &f, Gate::Now, NOW + 1).skip, Some(Skip::Wait));
    assert_eq!(f.data.borrow().len(), 1);

    // Valid JSON with a refused or cut stop makes no proposal.
    for stop in [Stop::Refused, Stop::Truncated] {
        let b = setup("stop", 2, &titles(2), &["record"]);
        let f = Rc::new(Fake {
            answers: RefCell::new(vec![good()].into()),
            data: RefCell::new(Vec::new()),
            during: None,
            stop,
        });
        let o = run(&b, &f, Gate::Now, NOW);
        assert_eq!((o.proposed, o.failed), (0, false), "{stop:?}");
        assert_eq!(col(&b, "SELECT count(*) FROM matter"), 0);
        // The same set does not repeat the call.
        assert_eq!(run(&b, &f, Gate::Now, NOW + 1).skip, Some(Skip::Same));
    }
}

/// An answer of an unknown format is a failed call: it starts the wait and
/// writes no fingerprint, so the same set is tried again after the wait.
/// The wait starts at the end of the call, by the wall clock.
#[test]
fn an_unknown_answer_waits_and_tries_again() {
    for text in ["not json", "{\"groups\": []}", "{\"matters\": 3}"] {
        let b = setup("unknown", 2, &titles(2), &["record"]);
        let f = fake(vec![
            Ok(text.to_string()),
            answer(json!([{"title": "T", "items": ["r-1", "r-2"]}])),
        ]);
        let o = run(&b, &f, Gate::Now, NOW);
        assert!(o.failed, "{text}");
        assert_eq!(
            col(
                &b,
                "SELECT count(*) FROM matter_run WHERE candidates IS NULL"
            ),
            1
        );
        assert_eq!(run(&b, &f, Gate::Now, NOW + 1).skip, Some(Skip::Wait));
        let later = col(&b, "SELECT retry_at FROM model_setting") + 1;
        assert_eq!(run(&b, &f, Gate::Now, later).proposed, 1, "{text}");
    }
    // A step whose `now` is long past (a slow call): the wait starts at the
    // end of the call, so it is not over at once.
    let b = setup("slow", 2, &titles(2), &["record"]);
    let f = fake(vec![Err(ModelError::Transport("time-out".into()))]);
    let past = 1_000_000_000_000;
    run(&b, &f, Gate::Now, past);
    let clock = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert!(col(&b, "SELECT retry_at FROM model_setting") > clock);
}

/// A failed store write of the counts of the call is no evidence against
/// the model (#156): it starts no wait.
#[test]
fn a_failed_count_write_starts_no_wait() {
    let b = setup("count-write", 3, &titles(3), &["record"]);
    b.store
        .write(|tx| {
            Ok(tx.execute_batch(
                "CREATE TRIGGER fail_counts BEFORE UPDATE OF unknown_calls ON understand_run
                 WHEN NEW.unknown_calls <> OLD.unknown_calls OR NEW.calls <> OLD.calls
                 BEGIN SELECT RAISE(ABORT, 'disk full'); END",
            )?)
        })
        .unwrap();
    let f = fake(vec![answer(json!([]))]);
    let connect = |_: &ModelRow| -> Result<std::boxed::Box<dyn Model>, String> {
        Ok(std::boxed::Box::new(Shared(f.clone())))
    };
    let _ = propose(&b.store, &connect, Gate::Now, NOW);
    assert_eq!(f.data.borrow().len(), 1, "the call went out");
    assert_eq!(
        col(
            &b,
            "SELECT coalesce(retry_at, 0) + retry_failures FROM model_setting"
        ),
        0
    );
}

/// Issue 112: no item of hostile mail goes to the model, also when the
/// item stays in the store, for each of the three states. The item is a
/// candidate again when the state clears.
#[test]
fn no_item_of_hostile_mail_goes_to_the_model() {
    let states: [(&str, &str); 3] = [
        (
            "UPDATE message SET hostile = '[\"lookalike\"]' WHERE id = 1",
            "UPDATE message SET hostile = '[]' WHERE id = 1",
        ),
        (
            "INSERT INTO message_label(message_id, label_id)
               SELECT 1, id FROM label WHERE provider_id = 'SPAM'",
            "DELETE FROM message_label WHERE message_id = 1
               AND label_id IN (SELECT id FROM label WHERE provider_id = 'SPAM')",
        ),
        (
            "UPDATE header SET from_addr = 'bad@example.com' WHERE message_id = 1;
             INSERT INTO sender(address, kind) VALUES ('bad@example.com', 'hostile')
               ON CONFLICT(address) DO UPDATE SET kind = 'hostile'",
            "UPDATE sender SET kind = 'person' WHERE address = 'bad@example.com'",
        ),
    ];
    for (n, (set, clear)) in states.into_iter().enumerate() {
        let b = setup(&format!("hostile-{n}"), 4, &titles(4), &["record"]);
        b.store
            .put_labels(1, &[("SPAM", "SPAM", "system")])
            .unwrap();
        b.store.write(|tx| Ok(tx.execute_batch(set)?)).unwrap();
        let f = fake(vec![]);
        assert_eq!(run(&b, &f, Gate::Now, NOW).skip, None, "{set}");
        {
            let data = f.data.borrow();
            assert_eq!(data.len(), 1, "{set}");
            assert!(!data[0].contains("Item 0"), "{set}: {}", data[0]);
            assert!(
                data[0].contains("Item 1") && data[0].contains("Item 3"),
                "{}",
                data[0]
            );
        }
        b.store.write(|tx| Ok(tx.execute_batch(clear)?)).unwrap();
        assert_eq!(run(&b, &f, Gate::Now, NOW + 1000).skip, None, "{clear}");
        assert!(f.data.borrow()[1].contains("Item 0"), "{clear}");
    }
}

/// Sets each signal item to `active` (the setup writes `open`).
fn activate_signals(b: &Box) {
    b.store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE item SET status = 'active' WHERE form = 'signal'",
                [],
            )?)
        })
        .unwrap();
}

fn reset_fingerprint(b: &Box) {
    b.store
        .write(|tx| Ok(tx.execute("UPDATE matter_run SET candidates = NULL", [])?))
        .unwrap();
}

/// D20 (docs/specs/2026-11-24-matters-edit.md): the call sends the
/// confirmed matters, and an addition waits for the person. A refused
/// addition does not come back. A signal is no candidate yet (#884).
#[test]
fn an_addition_waits_for_the_person() {
    let b = setup(
        "addition",
        3,
        &titles(4),
        &["record", "conversation", "commitment", "signal"],
    );
    activate_signals(&b);
    let m = iris_matters::create(
        &b.store,
        "Tokyo trip",
        &[(1, "record"), (2, "conversation")],
        NOW,
    )
    .unwrap();
    let add = json!({"matters": [], "additions": [
        {"matter": format!("g-{m}"), "items": ["c-3"]},
        {"matter": format!("g-{m}"), "items": ["s-4"]},
    ]});
    let f = fake(vec![Ok(add.to_string()), Ok(add.to_string())]);
    let o = run(&b, &f, Gate::Now, NOW);
    assert_eq!((o.proposed, o.added, o.dropped), (0, 1, 1), "{o:?}");
    let data = f.data.borrow()[0].clone();
    assert!(data.contains(&format!("\"matter\":\"g-{m}\"")), "{data}");
    assert!(data.contains("\"id\":\"c-3\""), "{data}");
    assert!(!data.contains("\"id\":\"s-4\""), "{data}");
    // The member titles go, the members are no candidates.
    assert!(!data.contains("\"id\":\"r-1\""), "{data}");
    assert_eq!(
        col(&b, "SELECT count(*) FROM matter_add_live WHERE item_id = 3"),
        1
    );
    assert_eq!(
        col(&b, "SELECT count(*) FROM matter_add WHERE item_id = 4"),
        0
    );
    // Not this: the person refuses it, so the same answer is dropped.
    let e = iris_matters::Edit {
        remove: vec![(3, "commitment")],
        ..Default::default()
    };
    iris_matters::edit(&b.store, m, &e, NOW + 1).unwrap();
    let o = run(&b, &f, Gate::Now, NOW + 2);
    assert_eq!(o.added, 0, "{o:?}");
    assert_eq!(col(&b, "SELECT count(*) FROM matter_add"), 0);
}

/// Random additions never write an addition outside the rules: the matter
/// was sent and is confirmed, the item was sent with its prefix, it has no
/// member row in the matter, and no item is in two places.
#[test]
fn random_additions_keep_the_rules() {
    let mut x: u64 = 0x5DEE_CE66;
    let mut next = |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    let forms = ["record", "conversation", "commitment", "signal", "feed"];
    let mut total = 0;
    for seed in 0..20 {
        let b = setup(&format!("radd-{seed}"), 4, &titles(20), &forms);
        activate_signals(&b);
        // g-1 confirmed with items 1 and 2; g-2 confirmed, then deleted.
        iris_matters::create(&b.store, "A", &[(1, "record"), (2, "conversation")], NOW).unwrap();
        let g2 =
            iris_matters::create(&b.store, "B", &[(3, "commitment"), (4, "signal")], NOW).unwrap();
        // A refusal row of g-1: the commitment 8.
        b.store
            .write(|tx| {
                Ok(tx.execute(
                    "INSERT INTO matter_item(matter_id, item_id, kept) VALUES (1, 8, 0)",
                    [],
                )?)
            })
            .unwrap();
        if next(2) == 0 {
            iris_matters::delete(&b.store, g2, NOW + 1).unwrap();
        }
        let prefixes = ["r", "v", "c", "s", "f", "m"];
        let adds: Vec<Value> = (0..1 + next(12))
            .map(|_| {
                let items: Vec<String> = (0..next(5))
                    .map(|_| {
                        let row = 1 + next(22);
                        let p = if next(10) < 7 {
                            prefixes[((row - 1) % 5) as usize]
                        } else {
                            prefixes[next(6) as usize]
                        };
                        format!("{p}-{row}")
                    })
                    .collect();
                json!({"matter": format!("g-{}", 1 + next(4)), "items": items})
            })
            .collect();
        let new = json!([{"title": "Trip", "items": ["r-6", "v-7"]}]);
        let body =
            json!({"matters": if next(2) == 0 { new } else { json!([]) }, "additions": adds});
        let f = fake(vec![Ok(body.to_string())]);
        let o = run(&b, &f, Gate::Now, NOW + 2);
        total += o.added;
        let sent = f.data.borrow()[0].clone();
        let rows: Vec<(i64, i64, String)> = b
            .store
            .read(|c| {
                let mut s = c.prepare(
                    "SELECT a.matter_id, a.item_id, i.form FROM matter_add a
                     JOIN item i ON i.id = a.item_id",
                )?;
                let v = s
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(v)
            })
            .unwrap();
        for (matter, item, form) in rows {
            let p = match form.as_str() {
                "conversation" => "v",
                f => &f[..1],
            };
            assert!(
                sent.contains(&format!("\"matter\":\"g-{matter}\"")),
                "seed {seed}"
            );
            assert!(
                sent.contains(&format!("\"id\":\"{p}-{item}\"")),
                "seed {seed}: {p}-{item}"
            );
            assert_ne!(form, "feed");
            assert_ne!(item, 8, "seed {seed}: a refused item came back");
            let status = col(
                &b,
                &format!("SELECT status = 'confirmed' FROM matter WHERE id = {matter}"),
            );
            assert_eq!(status, 1, "seed {seed}");
            let elsewhere = col(
                &b,
                &format!("SELECT count(*) FROM matter_item WHERE item_id = {item} AND (kept = 1 OR matter_id = {matter})"),
            );
            assert_eq!(elsewhere, 0, "seed {seed}: {item}");
        }
    }
    assert!(
        total > 0,
        "no random addition passed: the test proves nothing"
    );
}

/// D20: the fingerprint holds each sent matter and its last change, so an
/// edit of a confirmed matter makes a new call.
#[test]
fn an_edit_of_a_sent_matter_makes_a_new_call() {
    let b = setup("medit", 3, &titles(4), &["record", "conversation"]);
    let m =
        iris_matters::create(&b.store, "Trip", &[(1, "record"), (2, "conversation")], NOW).unwrap();
    let f = fake(vec![]);
    assert_eq!(run(&b, &f, Gate::Now, NOW).skip, None);
    assert_eq!(run(&b, &f, Gate::Now, NOW + 1).skip, Some(Skip::Same));
    let e = iris_matters::Edit {
        title: Some("Tokyo trip".into()),
        ..Default::default()
    };
    iris_matters::edit(&b.store, m, &e, NOW + 2).unwrap();
    assert_eq!(run(&b, &f, Gate::Now, NOW + 3).skip, None);
    assert_eq!(f.data.borrow().len(), 2);
    assert!(f.data.borrow()[1].contains("Tokyo trip"));
}

/// The request stays in its bound with the lines of the confirmed matters
/// first: long titles of matters and members, and many long items.
#[test]
fn the_request_with_matter_lines_stays_in_its_bound() {
    let long: Vec<String> = (0..300)
        .map(|i| format!("{i} {}", "<<<é".repeat(60)))
        .collect();
    let b = setup("mbound", 7, &long, &["record", "conversation"]);
    // 25 confirmed matters of 2 members each (rows 1 to 50): 20 go.
    for k in 0..25i64 {
        let title = format!("{k} {}", "é<".repeat(38));
        iris_matters::create(
            &b.store,
            &title,
            &[(2 * k + 1, "record"), (2 * k + 2, "conversation")],
            NOW + k,
        )
        .unwrap();
    }
    let f = fake(vec![]);
    run(&b, &f, Gate::Now, NOW + 100);
    let data = f.data.borrow()[0].clone();
    let bytes = Shared(f.clone()).request_bytes(iris_matters::propose::INSTRUCTION, &data);
    assert!(bytes <= MAX_REQUEST, "{bytes}");
    let sent = data.lines().filter(|l| l.contains("\"matter\"")).count();
    assert_eq!(sent, iris_matters::propose::MAX_SENT_MATTERS, "{sent}");
    assert!(data.lines().next().unwrap().contains("\"matter\""));
}

/// An addition that takes a matter over 50 members is dropped, and one
/// that fits is kept.
#[test]
fn an_addition_over_50_members_is_dropped() {
    let b = setup("m50", 3, &titles(60), &["record", "conversation"]);
    let m =
        iris_matters::create(&b.store, "Trip", &[(1, "record"), (2, "conversation")], NOW).unwrap();
    let id = |row: i64| format!("{}-{row}", if row % 2 == 1 { "r" } else { "v" });
    let over: Vec<String> = (3..=51).map(id).collect();
    let fits: Vec<String> = (3..=50).map(id).collect();
    let f = fake(vec![
        Ok(
            json!({"matters": [], "additions": [{"matter": format!("g-{m}"), "items": over}]})
                .to_string(),
        ),
        Ok(
            json!({"matters": [], "additions": [{"matter": format!("g-{m}"), "items": fits}]})
                .to_string(),
        ),
    ]);
    let o = run(&b, &f, Gate::Now, NOW);
    assert_eq!((o.added, o.dropped), (0, 1), "{o:?}");
    assert_eq!(col(&b, "SELECT count(*) FROM matter_add"), 0);
    reset_fingerprint(&b);
    let o = run(&b, &f, Gate::Now, NOW + 1);
    assert_eq!((o.added, o.dropped), (48, 0), "{o:?}");
    assert_eq!(col(&b, "SELECT count(*) FROM matter_add"), 48);
}

/// A matter that the person deletes during the call gets no addition.
#[test]
fn an_addition_to_a_matter_deleted_during_the_call_is_dropped() {
    let b = setup("mgone", 3, &titles(4), &["record", "conversation"]);
    let m =
        iris_matters::create(&b.store, "Trip", &[(1, "record"), (2, "conversation")], NOW).unwrap();
    let f = fake_during(
        vec![Ok(json!({"matters": [], "additions": [{"matter": format!("g-{m}"), "items": ["r-3"]}]})
            .to_string())],
        Some((
            b.dir.clone(),
            format!(
                "UPDATE matter SET status = 'declined', keep = NULL, decided_at = {t}, updated_at = {t} WHERE id = {m}",
                t = NOW + 5
            ),
        )),
    );
    let o = run(&b, &f, Gate::Now, NOW + 1);
    assert!(f.data.borrow()[0].contains(&format!("\"matter\":\"g-{m}\"")));
    assert_eq!((o.added, o.dropped), (0, 1), "{o:?}");
    assert_eq!(col(&b, "SELECT count(*) FROM matter_add"), 0);
}
