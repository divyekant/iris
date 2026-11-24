//! The move of a job on its due date (decision D24,
//! docs/specs/2026-11-24-job-due-date.md).

use std::fs;
use std::path::PathBuf;

use iris_job::{
    answer, ask, create, due_jobs, events, finish, get, move_due, move_due_one, wait, wake, By,
    Due, End, State, Wait, DUE_QUESTION,
};
use iris_store::Store;

const NOW: i64 = 1_763_283_422_000;
const DAY: i64 = 86_400_000;

struct Fixture {
    dir: PathBuf,
    store: Store,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("iris-job-due-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    Fixture { dir, store }
}

fn sql(store: &Store, text: &str) -> bool {
    store.write(|tx| Ok(tx.execute_batch(text)?)).is_ok()
}

#[test]
fn an_open_job_moves_once_on_its_due_date() {
    let f = fixture("once");
    let s = &f.store;
    let working = create(s, "Book the dentist.", Some(NOW + DAY), NOW).unwrap();
    let waiting = create(s, "Get the contract.", Some(NOW + DAY), NOW).unwrap();
    let w = Wait {
        thread: None,
        until_ms: NOW + 5 * DAY,
    };
    wait(s, waiting, w, None, NOW + 1).unwrap();
    let no_due = create(s, "No date.", None, NOW).unwrap();
    // Before the due date: no move.
    assert!(move_due(s, NOW + DAY - 1).unwrap().moved.is_empty());
    let moved = move_due(s, NOW + DAY).unwrap().moved;
    assert_eq!(
        moved,
        vec![
            Due {
                id: working,
                goal: "Book the dentist.".into()
            },
            Due {
                id: waiting,
                goal: "Get the contract.".into()
            }
        ]
    );
    for job in [working, waiting] {
        let j = get(s, job).unwrap();
        assert_eq!(j.state, State::NeedsYou);
        assert_eq!(j.question.as_deref(), Some(DUE_QUESTION));
        assert_eq!(j.wait, None);
        let e = events(s, job).unwrap().pop().unwrap();
        assert_eq!((e.by, e.why.as_deref()), (By::Engine, Some("due")));
    }
    assert_eq!(get(s, no_due).unwrap().state, State::Working);
    // Once for each due date: not again, also after the answer.
    assert!(move_due(s, NOW + 2 * DAY).unwrap().moved.is_empty());
    answer(s, working, "Go on, book next week.", None, NOW + 2 * DAY).unwrap();
    assert!(move_due(s, NOW + 3 * DAY).unwrap().moved.is_empty());
    assert_eq!(get(s, working).unwrap().state, State::Working);
}

#[test]
fn a_job_that_the_person_or_the_agent_handled_after_the_due_date_does_not_move() {
    let f = fixture("handled");
    let s = &f.store;
    // Asked before the due date, answered after it.
    let asked = create(s, "A.", Some(NOW + DAY), NOW).unwrap();
    ask(s, asked, "Which day?", None, NOW + 1).unwrap();
    assert!(
        move_due(s, NOW + DAY).unwrap().moved.is_empty(),
        "needs_you already"
    );
    answer(s, asked, "Monday.", None, NOW + DAY + 1).unwrap();
    // Asked after the due date.
    let late_ask = create(s, "B.", Some(NOW + DAY), NOW).unwrap();
    ask(s, late_ask, "Which day?", None, NOW + DAY + 1).unwrap();
    answer(s, late_ask, "Monday.", None, NOW + DAY + 2).unwrap();
    // Woken by the person after the due date.
    let woken = create(s, "C.", Some(NOW + DAY), NOW).unwrap();
    let w = Wait {
        thread: None,
        until_ms: NOW + 5 * DAY,
    };
    wait(s, woken, w, None, NOW + 1).unwrap();
    wake(s, woken, By::Person, None, NOW + DAY + 1).unwrap();
    // Done jobs never move.
    let done = create(s, "D.", Some(NOW + DAY), NOW).unwrap();
    finish(s, done, By::Person, End::Cancel, None, NOW + 1).unwrap();
    assert!(move_due(s, NOW + 2 * DAY).unwrap().moved.is_empty());
}

#[test]
fn a_move_after_the_read_wins() {
    let f = fixture("race");
    let s = &f.store;
    let job = create(s, "A.", Some(NOW + DAY), NOW).unwrap();
    assert_eq!(due_jobs(s, NOW + DAY).unwrap(), vec![job]);
    // The person ends the job between the read and the write.
    finish(s, job, By::Person, End::Complete, None, NOW + DAY).unwrap();
    assert_eq!(move_due_one(s, job, NOW + DAY).unwrap(), None);
    assert_eq!(get(s, job).unwrap().state, State::Done);
}

#[test]
fn a_clock_that_goes_back_does_not_fail_the_move() {
    let f = fixture("clock");
    let s = &f.store;
    let job = create(s, "A.", Some(NOW + DAY), NOW).unwrap();
    let w = Wait {
        thread: None,
        until_ms: NOW + 5 * DAY,
    };
    wait(s, job, w, None, NOW + 2 * DAY).unwrap();
    assert!(move_due_one(s, job, NOW + DAY).unwrap().is_some());
    assert_eq!(get(s, job).unwrap().updated_ms, NOW + 2 * DAY);
}

#[test]
fn the_move_table_refuses_each_other_due_move() {
    let f = fixture("table");
    let s = &f.store;
    let job = create(s, "A.", Some(NOW + DAY), NOW).unwrap();
    let none = create(s, "B.", None, NOW).unwrap();
    let insert = |job: i64, at: i64, by: &str, why: &str| {
        sql(
            s,
            &format!(
                "INSERT INTO job_event (job_id, at, state, by, question, why)
                 VALUES ({job}, {at}, 'needs_you', '{by}', 'Q?', {why})"
            ),
        )
    };
    // Before the due date, with no due date, with another reason or no
    // reason, and by the person.
    assert!(!insert(job, NOW + DAY - 1, "engine", "'due'"));
    assert!(!insert(none, NOW + DAY, "engine", "'due'"));
    assert!(!insert(job, NOW + DAY, "engine", "'time'"));
    assert!(!insert(job, NOW + DAY, "engine", "NULL"));
    assert!(!insert(job, NOW + DAY, "person", "'due'"));
    assert_eq!(get(s, job).unwrap().state, State::Working);
    assert!(insert(job, NOW + DAY, "engine", "'due'"));
    // From needs_you: no second due move.
    assert!(!insert(job, NOW + DAY + 1, "engine", "'due'"));
}

#[test]
fn a_job_that_starts_after_its_due_date_moves_once() {
    let f = fixture("past");
    let s = &f.store;
    // The start event is by the person, after the due date: it is no move.
    let job = create(s, "Pay the late bill.", Some(NOW - DAY), NOW).unwrap();
    assert_eq!(due_jobs(s, NOW).unwrap(), vec![job]);
    assert_eq!(move_due(s, NOW).unwrap().moved.len(), 1);
    assert_eq!(get(s, job).unwrap().state, State::NeedsYou);
    // The person answers: a move by the person after the due date.
    assert!(move_due(s, NOW + DAY).unwrap().moved.is_empty());
    answer(s, job, "Go on.", None, NOW + 1).unwrap();
    assert!(move_due(s, NOW + 2 * DAY).unwrap().moved.is_empty());
}

#[test]
fn a_job_whose_goal_is_not_utf8_does_not_stop_the_others() {
    let f = fixture("utf8");
    let s = &f.store;
    let first = create(s, "First.", Some(NOW), NOW - 1).unwrap();
    // Raw SQL stores a goal that is not UTF-8 (job.rs, a_reader_refuses_text_that_is_not_utf8).
    assert!(sql(
        s,
        &format!(
            "INSERT INTO job (goal, due_at, created_at, state, updated_at)
               VALUES (CAST(x'41ff42' AS TEXT), {NOW}, {}, 'working', {})",
            NOW - 1,
            NOW - 1
        )
    ));
    let third = create(s, "Third.", Some(NOW), NOW - 1).unwrap();
    let turn = move_due(s, NOW).unwrap();
    let ids: Vec<i64> = turn.moved.iter().map(|d| d.id).collect();
    assert_eq!(ids, vec![first, third], "{turn:?}");
    // The bad job fails closed: no move, and it stays in `working`.
    assert_eq!(turn.failed.len(), 1, "{turn:?}");
    let bad = turn.failed[0].0;
    let state: String = s
        .read(|c| Ok(c.query_row("SELECT state FROM job WHERE id = ?1", [bad], |r| r.get(0))?))
        .unwrap();
    assert_eq!(state, "working");
    let again = move_due(s, NOW + 1).unwrap();
    assert!(again.moved.is_empty() && again.failed.len() == 1, "{again:?}");
}
