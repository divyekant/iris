use std::fs;
use std::path::PathBuf;

use iris_job::{
    answer, ask, create, events, finish, format_job_id, get, list, page, parse_job_id, wait, wake,
    write_document, By, End, Error, State, Wait,
};
use iris_store::Store;
use rusqlite::params;
use serde_json::Value;

/// A new empty folder for one test, removed at the end.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-job-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const NOW: i64 = 1_763_283_422_000;
const DAY: i64 = 86_400_000;

fn refused<T: std::fmt::Debug>(r: Result<T, Error>) {
    assert!(matches!(r, Err(Error::Refused(_))), "{r:?}");
}

/// Run raw SQL on the write connection. Gives `true` if it succeeds.
fn sql(store: &Store, text: &str) -> bool {
    store.write(|tx| Ok(tx.execute_batch(text)?)).is_ok()
}

/// The state of the job row and of its newest event, and the event count.
fn agree(store: &Store, job: i64) {
    let (row, newest, n): (String, Option<String>, i64) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT state,
                   (SELECT state FROM job_event WHERE job_id = ?1 ORDER BY id DESC LIMIT 1),
                   (SELECT count(*) FROM job_event WHERE job_id = ?1)
                 FROM job WHERE id = ?1",
                [job],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert!(n >= 1, "job {job} has no event");
    assert_eq!(Some(row), newest, "job {job}");
}

#[test]
fn a_job_keeps_its_state_after_a_restart() {
    let dir = TempDir::new("restart");
    let w = Wait {
        thread: Some(7),
        until_ms: NOW + 2 * DAY,
    };
    let (job, before, events_before) = {
        let store = Store::open(&dir.0).unwrap();
        let job = create(
            &store,
            "Get the signed contract from Dana.",
            Some(NOW + 4 * DAY),
            NOW,
        )
        .unwrap();
        ask(&store, job, "Can I accept section 4?", None, NOW + 1).unwrap();
        answer(&store, job, "Yes.", NOW + 2).unwrap();
        wait(&store, job, w, Some("Dana has the contract now."), NOW + 3).unwrap();
        (job, get(&store, job).unwrap(), events(&store, job).unwrap())
    };
    let store = Store::open(&dir.0).unwrap();
    let after = get(&store, job).unwrap();
    assert_eq!(after, before);
    assert_eq!(after.state, State::Waiting);
    assert_eq!(after.wait, Some(w));
    assert_eq!(after.due_ms, Some(NOW + 4 * DAY));
    assert_eq!(after.updated_ms, NOW + 3);
    assert_eq!(events(&store, job).unwrap(), events_before);
    assert_eq!(events_before.len(), 4);
    assert_eq!(events_before[2].answer.as_deref(), Some("Yes."));
    assert_eq!(list(&store, State::Waiting).unwrap(), vec![after]);
}

/// The move table of the spec: from, to, who, and the kind of value.
#[derive(Clone, Copy, Debug)]
enum Val {
    None,
    Wait,
    Question,
    Answer,
    Complete,
    Cancel,
}

fn allowed(from: State, to: State, by: By, v: Val) -> bool {
    use State::*;
    match (from, to, v) {
        (Working, Waiting, Val::Wait) => by == By::Agent,
        (Working, NeedsYou, Val::Question) => by == By::Agent,
        (Waiting, Working, Val::None) => by == By::Engine || by == By::Person,
        (NeedsYou, Working, Val::Answer) => by == By::Person,
        (Working, Done, Val::Complete) => by == By::Agent || by == By::Person,
        (Waiting | NeedsYou, Done, Val::Complete) => by == By::Person,
        (Working | Waiting | NeedsYou, Done, Val::Cancel) => by == By::Person,
        _ => false,
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<T: Copy>(&mut self, from: &[T]) -> T {
        from[(self.next() % from.len() as u64) as usize]
    }
}

/// Property test of the move table: random moves, through raw SQL, with
/// random states, actors and values. Each move succeeds exactly when the
/// table allows it, and the job row always agrees with its newest event.
#[test]
fn only_the_moves_of_the_table_succeed() {
    let dir = TempDir::new("moves");
    let store = Store::open(&dir.0).unwrap();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let values = [
        Val::None,
        Val::Wait,
        Val::Question,
        Val::Answer,
        Val::Complete,
        Val::Cancel,
    ];
    let (mut ok, mut no) = (0, 0);
    for round in 0..60 {
        let job = create(&store, &format!("Goal {round}"), None, NOW).unwrap();
        agree(&store, job);
        let mut at = NOW;
        for _ in 0..12 {
            at += 1000;
            let from = get(&store, job).unwrap().state;
            let to = rng.pick(State::ALL);
            let by = rng.pick(By::ALL);
            let v = rng.pick(&values);
            let (thread, until, question, answer, end) = match v {
                Val::None => (None, None, None, None, None),
                Val::Wait => (Some(9), Some(at + DAY), None, None, None),
                Val::Question => (None, None, Some("Which day?"), None, None),
                Val::Answer => (None, None, None, Some("Tuesday."), None),
                Val::Complete => (None, None, None, None, Some("complete")),
                Val::Cancel => (None, None, None, None, Some("cancel")),
            };
            let done = store
                .write(|tx| {
                    Ok(tx.execute(
                        "INSERT INTO job_event (job_id, at, state, by, wait_thread, wait_until,
                           question, answer, end_kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![
                            job,
                            at,
                            to.as_str(),
                            by.as_str(),
                            thread,
                            until,
                            question,
                            answer,
                            end
                        ],
                    )?)
                })
                .is_ok();
            assert_eq!(
                done,
                allowed(from, to, by, v),
                "{from:?} -> {to:?} by {by:?} with {v:?}"
            );
            if done {
                ok += 1;
            } else {
                no += 1;
            }
            agree(&store, job);
        }
    }
    assert!(ok > 50 && no > 50, "{ok} moves, {no} refusals");
}

#[test]
fn the_api_moves_and_refusals() {
    let dir = TempDir::new("api");
    let store = Store::open(&dir.0).unwrap();
    let job = create(&store, "Book the dentist.", None, NOW).unwrap();
    // A wait must end after the move.
    let w = |until_ms| Wait {
        thread: None,
        until_ms,
    };
    refused(wait(&store, job, w(NOW), None, NOW));
    // The person does not wait or ask, and the agent does not answer.
    refused(answer(&store, job, "Yes.", NOW));
    wait(&store, job, w(NOW + DAY), None, NOW + 1).unwrap();
    // The agent does not wake or end a waiting job.
    refused(wake(&store, job, By::Agent, None, NOW + 2));
    refused(finish(&store, job, By::Agent, End::Complete, None, NOW + 2));
    // An event is not before the newest event.
    refused(wake(&store, job, By::Engine, Some("reply"), NOW));
    wake(&store, job, By::Engine, Some("reply"), NOW + 2).unwrap();
    // The agent does not cancel.
    refused(finish(&store, job, By::Agent, End::Cancel, None, NOW + 3));
    ask(&store, job, "Morning or afternoon?", None, NOW + 3).unwrap();
    finish(
        &store,
        job,
        By::Person,
        End::Cancel,
        Some("Not needed."),
        NOW + 4,
    )
    .unwrap();
    // done is final.
    for by in By::ALL {
        refused(wake(&store, job, *by, None, NOW + 5));
        refused(finish(&store, job, *by, End::Complete, None, NOW + 5));
    }
    let j = get(&store, job).unwrap();
    assert_eq!(
        (j.state, j.end, j.question),
        (State::Done, Some(End::Cancel), None)
    );
    let states: Vec<State> = events(&store, job)
        .unwrap()
        .iter()
        .map(|e| e.state)
        .collect();
    use State::*;
    assert_eq!(states, [Working, Waiting, Working, NeedsYou, Done]);
    // Unknown job.
    assert!(matches!(get(&store, 99), Err(Error::NotFound)));
    assert!(matches!(events(&store, 99), Err(Error::NotFound)));
    assert!(matches!(
        wake(&store, 99, By::Person, None, NOW),
        Err(Error::NotFound)
    ));
}

#[test]
fn the_text_limits() {
    let dir = TempDir::new("limits");
    let store = Store::open(&dir.0).unwrap();
    refused(create(&store, "", None, NOW));
    refused(create(&store, &"g".repeat(2001), None, NOW));
    // Characters, not bytes: 2,000 two-byte characters fit.
    let job = create(&store, &"é".repeat(2000), None, NOW).unwrap();
    refused(ask(&store, job, "", None, NOW));
    refused(ask(&store, job, &"q".repeat(2001), None, NOW));
    refused(ask(&store, job, "Which?", Some(""), NOW));
    refused(ask(
        &store,
        job,
        &format!("x\0{}", "y".repeat(3000)),
        None,
        NOW,
    ));
    refused(create(&store, "G", Some(-1), NOW));
    refused(create(&store, "G", None, 253_402_300_800_000));
    ask(&store, job, "Which?", None, NOW).unwrap();
    refused(answer(&store, job, &"a".repeat(10_001), NOW));
    answer(&store, job, &"a".repeat(10_000), NOW).unwrap();
    agree(&store, job);
}

#[test]
fn raw_sql_cannot_change_a_job_without_an_event() {
    let dir = TempDir::new("raw");
    let store = Store::open(&dir.0).unwrap();
    let job = create(&store, "Pay the invoice.", None, NOW).unwrap();
    for text in [
        "UPDATE job SET goal = 'Other.'",
        "UPDATE job SET due_at = 5",
        "UPDATE job SET created_at = 5",
        "UPDATE job SET id = 50",
        "UPDATE job SET state = 'done', end_kind = 'complete'",
        "UPDATE job SET updated_at = updated_at + 1",
        "DELETE FROM job",
        "UPDATE job_event SET by = 'agent'",
        "DELETE FROM job_event",
        // A second create event.
        "INSERT INTO job_event (job_id, at, state, by) VALUES (1, 1763283422000, 'working', 'person')",
        // A new job that is not in working.
        "INSERT INTO job (goal, created_at, state, question, updated_at)
           VALUES ('G', 1, 'needs_you', 'Q', 1)",
        "INSERT INTO job (goal, created_at, state, updated_at) VALUES ('G', 1, 'working', 2)",
        // An insert that replaces the job.
        "INSERT OR REPLACE INTO job (id, goal, created_at, state, updated_at)
           VALUES (1, 'G', 1, 'working', 1)",
        // An event with an ID lower than the newest event.
        "INSERT INTO job_event (id, job_id, at, state, by, question)
           VALUES (0, 1, 1763283423000, 'needs_you', 'agent', 'Q')",
        // A job ID that is not positive: j-0 is not a job ID.
        "INSERT INTO job (id, goal, created_at, state, updated_at) VALUES (0, 'G', 1, 'working', 1)",
        "INSERT INTO job (id, goal, created_at, state, updated_at) VALUES (-3, 'G', 1, 'working', 1)",
        // A value that does not fit the job document: a NUL in a text (SQLite
        // length() stops at a NUL), a blob, a time that is not an integer or
        // not in the years 1970 to 9999, a thread ID that is not positive.
        "INSERT INTO job (goal, created_at, state, updated_at)
           VALUES ('x' || char(0) || printf('%.3000c', 'y'), 1, 'working', 1)",
        "INSERT INTO job (goal, created_at, state, updated_at) VALUES (x'41', 1, 'working', 1)",
        "INSERT INTO job (goal, created_at, state, updated_at) VALUES ('G', 'soon', 'working', 'soon')",
        "INSERT INTO job (goal, created_at, state, updated_at) VALUES ('G', -1, 'working', -1)",
        "INSERT INTO job (goal, due_at, created_at, state, updated_at)
           VALUES ('G', 253402300800000, 1, 'working', 1)",
        "INSERT INTO job (goal, due_at, created_at, state, updated_at) VALUES ('G', 1.5, 1, 'working', 1)",
        "INSERT INTO job_event (job_id, at, state, by, question)
           VALUES (1, 1763283423000, 'needs_you', 'agent', 'Q' || char(0) || 'more')",
        "INSERT INTO job_event (job_id, at, state, by, wait_thread, wait_until)
           VALUES (1, 1763283423000, 'waiting', 'agent', 0, 1763283424000)",
        "INSERT INTO job_event (job_id, at, state, by, wait_until)
           VALUES (1, 1763283423000, 'waiting', 'agent', '1763283424000x')",
        "INSERT INTO job_event (job_id, at, state, by, end_kind, why)
           VALUES (1, 1763283423000, 'done', 'person', 'cancel', char(0))",
        "INSERT INTO job_event (id, job_id, at, state, by, question)
           VALUES (-1, 1, 1763283423000, 'needs_you', 'agent', 'Q')",
        // An event of a job that does not exist.
        "INSERT INTO job_event (job_id, at, state, by, end_kind)
           VALUES (9, 1763283423000, 'done', 'person', 'cancel')",
    ] {
        assert!(!sql(&store, text), "{text}");
    }
    agree(&store, job);
    assert_eq!(get(&store, job).unwrap().goal, "Pay the invoice.");
    // A raw insert of a job gets its create event from the trigger.
    assert!(sql(
        &store,
        "INSERT INTO job (goal, created_at, state, updated_at) VALUES ('G', 7, 'working', 7)"
    ));
    let e = events(&store, 2).unwrap();
    assert_eq!((e.len(), e[0].by, e[0].at_ms), (1, By::Person, 7));
    agree(&store, 2);
}

/// Two moves of one job at the same time: one succeeds, and the other gets
/// the new state and fails.
#[test]
fn two_moves_at_the_same_time() {
    let dir = TempDir::new("race");
    let store = Store::open(&dir.0).unwrap();
    for i in 0..20 {
        let job = create(&store, "Race.", None, NOW).unwrap();
        let (a, b) = std::thread::scope(|s| {
            let a = s.spawn(|| ask(&store, job, "Which?", None, NOW + 1));
            let b = s.spawn(|| finish(&store, job, By::Agent, End::Complete, None, NOW + 1));
            (a.join().unwrap(), b.join().unwrap())
        });
        assert!(a.is_ok() != b.is_ok(), "round {i}: {a:?} {b:?}");
        agree(&store, job);
    }
}

#[test]
fn the_document_of_each_state_passes_the_schema() {
    let dir = TempDir::new("doc");
    let store = Store::open(&dir.0).unwrap();
    let schema = iris_spec::validator("job");
    let check = |job: i64, state: &str| {
        let mut out = Vec::new();
        write_document(&store, job, &mut out).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        let errors: Vec<String> = schema.iter_errors(&v).map(|e| e.to_string()).collect();
        assert!(errors.is_empty(), "{state}: {errors:?}\n{v}");
        assert_eq!(v["state"], state);
        assert_eq!(v["id"], format_job_id(job));
        v
    };
    let job = create(&store, "Get the contract.", Some(NOW + DAY), NOW).unwrap();
    check(job, "working");
    let w = Wait {
        thread: Some(8841),
        until_ms: NOW + DAY,
    };
    wait(&store, job, w, Some("Dana has it."), NOW + 1).unwrap();
    let v = check(job, "waiting");
    assert_eq!(v["wait"]["thread"], "t-8841");
    wake(&store, job, By::Engine, Some("reply"), NOW + 2).unwrap();
    ask(&store, job, "Accept section 4?", None, NOW + 3).unwrap();
    check(job, "needs_you");
    answer(&store, job, "Yes.", NOW + 4).unwrap();
    finish(
        &store,
        job,
        By::Agent,
        End::Complete,
        Some("Signed."),
        NOW + 5,
    )
    .unwrap();
    let v = check(job, "done");
    assert_eq!(v["events"].as_array().unwrap().len(), 6);
    assert_eq!(v["events"][4]["answer"], "Yes.");
    let mut out = Vec::new();
    assert!(matches!(
        write_document(&store, 99, &mut out),
        Err(Error::NotFound)
    ));
}

/// Property test of the job ID parser on random text: it accepts exactly
/// `j-` and a positive decimal `i64`, and round-trips.
#[test]
fn job_id_parser() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let alphabet = b"j-0123456789+ x";
    for _ in 0..20_000 {
        let len = (rng.next() % 8) as usize;
        let text: String = (0..len).map(|_| rng.pick(alphabet) as char).collect();
        // The parser takes only the canonical form: the text that
        // format_job_id gives for the row (no zero in front, no sign).
        let expect = text
            .strip_prefix("j-")
            .and_then(|n| n.parse::<i64>().ok())
            .filter(|n| *n > 0 && format_job_id(*n) == text);
        assert_eq!(parse_job_id(&text), expect, "{text:?}");
        if let Some(n) = expect {
            assert_eq!(parse_job_id(&format_job_id(n)), Some(n));
        }
    }
    assert_eq!(parse_job_id("j-99999999999999999999"), None);
    for bad in ["j-01", "j-007", "j-0", "j-+1"] {
        assert_eq!(parse_job_id(bad), None, "{bad}");
    }
}

/// SQLite stores text that is not UTF-8 from raw SQL, and has no check of
/// it. Each reader of such a row fails closed with an error.
#[test]
fn a_reader_refuses_text_that_is_not_utf8() {
    let dir = TempDir::new("utf8");
    let store = Store::open(&dir.0).unwrap();
    assert!(sql(
        &store,
        "INSERT INTO job (goal, created_at, state, updated_at)
           VALUES (CAST(x'41ff42' AS TEXT), 1, 'working', 1)"
    ));
    assert!(sql(
        &store,
        "INSERT INTO job_event (job_id, at, state, by, question)
           VALUES (1, 2, 'needs_you', 'agent', CAST(x'80' AS TEXT))"
    ));
    assert!(matches!(get(&store, 1), Err(Error::Store(_))));
    assert!(matches!(
        list(&store, State::NeedsYou),
        Err(Error::Store(_))
    ));
    assert!(matches!(events(&store, 1), Err(Error::Store(_))));
    let mut out = Vec::new();
    assert!(matches!(
        write_document(&store, 1, &mut out),
        Err(Error::Store(_))
    ));
}

/// Issue #303: a valid job with a bad event gives an error and no byte of
/// the document, so a caller cannot send a partial document.
#[test]
fn a_bad_event_gives_no_partial_document() {
    let dir = TempDir::new("partial");
    let store = Store::open(&dir.0).unwrap();
    let job = create(&store, "Get the contract.", None, NOW).unwrap();
    ask(&store, job, "Which copy?", None, NOW + 1).unwrap();
    assert!(sql(
        &store,
        &format!(
            "INSERT INTO job_event (job_id, at, state, by, answer)
               VALUES ({job}, {}, 'working', 'person', CAST(x'80' AS TEXT))",
            NOW + 2
        )
    ));
    assert!(get(&store, job).is_ok(), "the job row stays valid");
    let mut out = Vec::new();
    assert!(matches!(
        write_document(&store, job, &mut out),
        Err(Error::Store(_))
    ));
    assert!(out.is_empty(), "{}", String::from_utf8_lossy(&out));
}

/// Generated input: for each mix of states, each limit and each start, the
/// pages hold each job of the states once, newest first, and no other job.
#[test]
fn the_pages_hold_each_job_of_the_states_once() {
    let dir = TempDir::new("page");
    let store = Store::open(&dir.0).unwrap();
    let mut all = Vec::new();
    for i in 0..12i64 {
        let job = create(&store, "A goal.", None, NOW + i).unwrap();
        match i % 4 {
            1 => ask(&store, job, "Which?", None, NOW + i).unwrap(),
            2 => wait(
                &store,
                job,
                Wait {
                    thread: None,
                    until_ms: NOW + DAY,
                },
                None,
                NOW + i,
            )
            .unwrap(),
            3 => finish(&store, job, By::Person, End::Cancel, None, NOW + i).unwrap(),
            _ => {}
        }
        all.push((job, get(&store, job).unwrap().state));
    }
    let states = [State::Working, State::NeedsYou, State::Waiting, State::Done];
    for mask in 1u32..16 {
        let pick: Vec<State> = (0..4)
            .filter(|b| mask & (1 << b) != 0)
            .map(|b| states[b])
            .collect();
        let want: Vec<i64> = all
            .iter()
            .rev()
            .filter(|(_, s)| pick.contains(s))
            .map(|(j, _)| *j)
            .collect();
        for limit in 1..=5u32 {
            let mut got = Vec::new();
            let mut before = None;
            loop {
                let jobs = page(&store, &pick, limit, before).unwrap();
                assert!(jobs.len() <= limit as usize);
                let Some(last) = jobs.last() else { break };
                before = Some(last.id);
                got.extend(jobs.iter().map(|j| j.id));
            }
            assert_eq!(got, want, "states {pick:?}, limit {limit}");
        }
    }
    assert!(page(&store, &[], 5, None).unwrap().is_empty());
}

#[test]
fn only_the_person_or_the_engine_changes_a_scope() {
    use iris_job::{add_scope, revoke_scope, scope, Ref};
    let dir = TempDir::new("scope");
    let store = Store::open(&dir.0).unwrap();
    let job = create(&store, "Get the contract.", None, NOW).unwrap();
    // No ID of a future row.
    refused(add_scope(&store, job, Ref::Thread(5), By::Person, NOW));
    refused(add_scope(&store, job, Ref::Item(9), By::Person, NOW));
    assert!(sql(
        &store,
        r#"INSERT INTO account (id, provider, address) VALUES (1, 'gmail', 'me@example.com');
           INSERT INTO thread (id, account_id, provider_id) VALUES (5, 1, 'a'), (6, 1, 'b');
           INSERT INTO item (id, form, account_id, status, doc, created_at, updated_at)
             VALUES (9, 'commitment', 1, 'open', '{}', 1, 1);"#
    ));
    refused(add_scope(&store, job, Ref::Thread(5), By::Agent, NOW));
    add_scope(&store, job, Ref::Thread(5), By::Person, NOW).unwrap();
    add_scope(&store, job, Ref::Item(9), By::Engine, NOW).unwrap();
    // The same ref again changes nothing.
    add_scope(&store, job, Ref::Thread(5), By::Engine, NOW + 1).unwrap();
    assert_eq!(scope(&store, job).unwrap(), [Ref::Thread(5), Ref::Item(9)]);
    assert!(matches!(scope(&store, 99), Err(Error::NotFound)));
    assert!(matches!(
        add_scope(&store, 99, Ref::Thread(5), By::Person, NOW),
        Err(Error::NotFound)
    ));
    for text in [
        "INSERT INTO job_scope (job_id, thread_id, by, at) VALUES (1, 6, 'agent', 1)",
        "INSERT INTO job_scope (job_id, thread_id, item_id, by, at) VALUES (1, 6, 7, 'person', 1)",
        "INSERT INTO job_scope (job_id, by, at) VALUES (1, 'person', 1)",
        "INSERT INTO job_scope (job_id, thread_id, by, at) VALUES (1, 0, 'person', 1)",
        "INSERT INTO job_scope (job_id, thread_id, by, at) VALUES (1, 5, 'person', 1)",
        "INSERT OR REPLACE INTO job_scope (job_id, thread_id, by, at) VALUES (1, 5, 'engine', 2)",
        "INSERT INTO job_scope (job_id, thread_id, by, at) VALUES (2, 6, 'person', 1)",
        "INSERT INTO job_scope (job_id, thread_id, by, at, revoked_by, revoked_at) VALUES (1, 6, 'person', 1, 'person', 1)",
        "UPDATE job_scope SET by = 'engine'",
        "UPDATE job_scope SET revoked_by = 'agent', revoked_at = 2",
        "UPDATE job_scope SET revoked_by = 'person'",
        "UPDATE job_scope SET revoked_by = 'person', revoked_at = 2, thread_id = 7 WHERE thread_id = 5",
        "DELETE FROM job_scope",
    ] {
        assert!(!sql(&store, text), "{text}");
    }

    // A revoke: only by the person or the engine, once. The row stays, and
    // the ref can be added again.
    refused(revoke_scope(&store, job, Ref::Thread(5), By::Agent, NOW));
    assert_eq!(iris_job::revoked_count(&store, job).unwrap(), 0);
    revoke_scope(&store, job, Ref::Thread(5), By::Person, NOW + 1).unwrap();
    assert_eq!(scope(&store, job).unwrap(), [Ref::Item(9)]);
    assert_eq!(iris_job::revoked_count(&store, job).unwrap(), 1);
    revoke_scope(&store, job, Ref::Thread(5), By::Engine, NOW + 2).unwrap();
    assert!(!sql(
        &store,
        "UPDATE job_scope SET revoked_by = 'engine', revoked_at = 9 WHERE thread_id = 5"
    ));
    add_scope(&store, job, Ref::Thread(5), By::Person, NOW + 3).unwrap();
    assert_eq!(scope(&store, job).unwrap(), [Ref::Item(9), Ref::Thread(5)]);
    let rows: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM job_scope", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(rows, 3);

    finish(&store, job, By::Person, End::Cancel, None, NOW + 4).unwrap();
    refused(add_scope(&store, job, Ref::Thread(6), By::Person, NOW + 5));
    revoke_scope(&store, job, Ref::Item(9), By::Person, NOW + 6).unwrap();
    assert_eq!(scope(&store, job).unwrap(), [Ref::Thread(5)]);
}
