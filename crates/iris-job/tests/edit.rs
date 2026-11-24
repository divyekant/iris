//! The edit of a job and the start from a typed goal (decision D25,
//! docs/specs/2026-11-24-job-goal-and-scope.md).

use std::fs;
use std::path::PathBuf;

use iris_job::{
    create, create_for_request_in, edit, events, finish, get, transaction, By, End, Error,
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
    let dir = std::env::temp_dir().join(format!("iris-job-edit-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    Fixture { dir, store }
}

fn sql(store: &Store, text: &str) -> bool {
    store.write(|tx| Ok(tx.execute_batch(text)?)).is_ok()
}

fn refused<T: std::fmt::Debug>(r: Result<T, Error>) {
    assert!(matches!(r, Err(Error::Refused(_))), "{r:?}");
}

/// Each edit as (goal before, due before, goal, due).
fn edits(store: &Store, job: i64) -> Vec<(String, Option<i64>, String, Option<i64>)> {
    store
        .read(|c| {
            let mut q = c.prepare(
                "SELECT goal_before, due_before, goal, due_at FROM job_edit WHERE job_id = ?1 ORDER BY id",
            )?;
            let v = q
                .query_map([job], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(v)
        })
        .unwrap()
}

#[test]
fn an_edit_changes_the_goal_and_the_due_date_and_keeps_both_values() {
    let f = fixture("edit");
    let s = &f.store;
    let job = create(s, "Book the dentist.", Some(NOW + DAY), NOW).unwrap();
    let before = get(s, job).unwrap();
    edit(s, job, "Book the dentist for Monday.", None, NOW + 5).unwrap();
    edit(
        s,
        job,
        "Book the dentist for Monday.",
        Some(NOW + 2 * DAY),
        NOW + 6,
    )
    .unwrap();
    let j = get(s, job).unwrap();
    assert_eq!(j.goal, "Book the dentist for Monday.");
    assert_eq!(j.due_ms, Some(NOW + 2 * DAY));
    // Not a move: the state, the time of the newest event and the events
    // stay.
    assert_eq!((j.state, j.updated_ms), (before.state, before.updated_ms));
    assert_eq!(events(s, job).unwrap().len(), 1);
    assert_eq!(
        edits(s, job),
        vec![
            (
                "Book the dentist.".into(),
                Some(NOW + DAY),
                "Book the dentist for Monday.".into(),
                None
            ),
            (
                "Book the dentist for Monday.".into(),
                None,
                "Book the dentist for Monday.".into(),
                Some(NOW + 2 * DAY)
            ),
        ]
    );
}

#[test]
fn an_edit_that_changes_nothing_or_of_a_done_job_is_refused() {
    let f = fixture("refused");
    let s = &f.store;
    let job = create(s, "A.", None, NOW).unwrap();
    refused(edit(s, job, "A.", None, NOW + 1));
    finish(s, job, By::Person, End::Complete, None, NOW + 2).unwrap();
    refused(edit(s, job, "B.", None, NOW + 3));
    assert!(matches!(
        edit(s, 999, "B.", None, NOW),
        Err(Error::NotFound)
    ));
    assert!(edits(s, job).is_empty());
}

#[test]
fn only_an_edit_changes_the_goal_or_the_due_date() {
    let f = fixture("sql");
    let s = &f.store;
    let job = create(s, "A.", None, NOW).unwrap();
    assert!(!sql(
        s,
        &format!("UPDATE job SET goal = 'B.' WHERE id = {job}")
    ));
    assert!(!sql(
        s,
        &format!("UPDATE job SET due_at = 5 WHERE id = {job}")
    ));
    // An edit row with the wrong values before, or out of order, is refused.
    assert!(!sql(
        s,
        &format!(
            "INSERT INTO job_edit (job_id, at, goal_before, due_before, goal, due_at)
             VALUES ({job}, {NOW}, 'Z.', NULL, 'B.', NULL)"
        )
    ));
    edit(s, job, "B.", None, NOW + 10).unwrap();
    assert!(!sql(
        s,
        &format!(
            "INSERT INTO job_edit (job_id, at, goal_before, due_before, goal, due_at)
             VALUES ({job}, {NOW}, 'B.', NULL, 'C.', NULL)"
        )
    ));
    // An edit row never changes and is never deleted.
    assert!(!sql(s, "UPDATE job_edit SET goal = 'X.'"));
    assert!(!sql(s, "DELETE FROM job_edit"));
    // A clock that goes back does not fail the edit of the person.
    edit(s, job, "C.", None, NOW).unwrap();
    assert_eq!(get(s, job).unwrap().goal, "C.");
}

#[test]
fn a_typed_start_with_the_same_request_makes_one_job() {
    let f = fixture("request");
    let s = &f.store;
    let start = |request: &str| {
        transaction(s, |c| {
            create_for_request_in(c, "Book the dentist.", None, request, NOW)
        })
        .unwrap()
    };
    let (job, made) = start("r-1");
    assert!(made);
    assert_eq!(start("r-1"), (job, false));
    let (other, made) = start("r-2");
    assert!(made && other != job);
}

/// Decision D25 with D24: an edit to a past due date after an answer is a
/// new due date. The job moves on the next step, once.
#[test]
fn an_edit_to_a_past_due_date_after_an_answer_moves_the_job_again() {
    let f = fixture("past-due");
    let s = &f.store;
    let job = iris_job::create(s, "Book the dentist.", Some(NOW + 100), NOW).unwrap();
    assert_eq!(iris_job::move_due(s, NOW + 100).unwrap().moved.len(), 1);
    iris_job::answer(s, job, "Go on.", None, NOW + 110).unwrap();
    assert!(iris_job::move_due(s, NOW + 115).unwrap().moved.is_empty());
    iris_job::edit(s, job, "Book the dentist.", Some(NOW + 105), NOW + 120).unwrap();
    assert_eq!(iris_job::move_due(s, NOW + 121).unwrap().moved.len(), 1);
    assert_eq!(
        iris_job::get(s, job).unwrap().state,
        iris_job::State::NeedsYou
    );
    // An edit of the goal only is not a new due date.
    iris_job::answer(s, job, "Go on.", None, NOW + 130).unwrap();
    iris_job::edit(s, job, "Book the dentist soon.", Some(NOW + 105), NOW + 140).unwrap();
    assert!(iris_job::move_due(s, NOW + 141).unwrap().moved.is_empty());
}
