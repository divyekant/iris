//! Job notices (`docs/specs/2026-11-25-job-notices.md`): a row for each move
//! of a job to `needs_you`, open while that move is the newest event, and a
//! system notice only for a move by the engine (the due date).

use iris_policy::notice;
use iris_store::Store;
use std::path::PathBuf;

const NOW: i64 = 1_800_000_000_000;

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn store(name: &str) -> (Dir, Store) {
    let dir = std::env::temp_dir().join(format!("iris-job-notice-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    (Dir(dir), store)
}

fn open(s: &Store) -> Vec<(Option<i64>, String)> {
    s.read(notice::list)
        .unwrap()
        .into_iter()
        .map(|n| (n.job_id, n.rule_name))
        .collect()
}

fn ids(v: Vec<notice::Notice>) -> Vec<i64> {
    v.into_iter().map(|n| n.id).collect()
}

/// A question of the agent gives an open row in the list and the count,
/// but no system notice (`after`, `jobs_after`). The due date gives a row
/// that interrupts. An answer closes the row with no write to it.
#[test]
fn a_question_waits_quietly_and_the_due_date_interrupts() {
    let (_d, s) = store("kinds");
    let goal = "Book the\nflight.";
    let asked = iris_job::create(&s, goal, None, NOW).unwrap();
    iris_job::ask(&s, asked, "Which day?", None, NOW + 1).unwrap();
    let due = iris_job::create(&s, "Renew the passport.", Some(NOW + 2), NOW).unwrap();
    let moved = iris_job::move_due(&s, NOW + 3).unwrap();
    assert_eq!(moved.moved.len(), 1, "{:?}", moved.failed);

    let list = s.read(notice::list).unwrap();
    assert_eq!(list.len(), 2);
    let q = list.iter().find(|n| n.job_id == Some(asked)).unwrap();
    assert_eq!(q.rule_name, "Job question");
    assert_eq!((q.rule, q.message_id), (None, None));
    assert_eq!(q.title, format!("j-{asked}"));
    // The goal is the words of the person, one line.
    assert_eq!(q.text, "Book the flight.");
    let d = list.iter().find(|n| n.job_id == Some(due)).unwrap();
    assert_eq!(d.rule_name, "Job due");
    assert_eq!(s.read(notice::counts).unwrap().0, 2);

    assert_eq!(ids(s.read(|c| notice::after(c, 0, 50)).unwrap()), [d.id]);
    assert_eq!(ids(s.read(|c| notice::jobs_after(c, 0)).unwrap()), [d.id]);
    assert!(s.read(|c| notice::is_open(c, q.id)).unwrap());

    // The answer adds a newer event: the row closes.
    iris_job::answer(&s, asked, "Monday.", None, NOW + 4).unwrap();
    assert!(!s.read(|c| notice::is_open(c, q.id)).unwrap());
    assert_eq!(open(&s), vec![(Some(due), "Job due".to_string())]);

    // A second question later: a second row; the first stays closed.
    iris_job::ask(&s, asked, "Which seat?", None, NOW + 5).unwrap();
    let rows: Vec<_> = s
        .read(notice::list)
        .unwrap()
        .into_iter()
        .filter(|n| n.job_id == Some(asked))
        .collect();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].id > q.id);
}

/// A move that rolls back leaves no row: the trigger runs in the
/// transaction of the move.
#[test]
fn a_move_that_rolls_back_leaves_no_row() {
    let (_d, s) = store("rollback");
    let job = iris_job::create(&s, "Goal.", None, NOW).unwrap();
    let r: iris_store::Result<()> = s.write(|tx| {
        iris_job::ask_in(tx, job, "Why?", None, NOW + 1)
            .map_err(|e| iris_store::Error::Io(std::io::Error::other(e.to_string())))?;
        Err(iris_store::Error::Io(std::io::Error::other("stop")))
    });
    assert!(r.is_err());
    assert!(open(&s).is_empty());
    let all: i64 = s
        .read(|c| Ok(c.query_row("SELECT count(*) FROM notice", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(all, 0);
}

/// The interrupt term reads who moved the job, not the name: a rule row
/// whose name is `Job question` still interrupts.
#[test]
fn a_rule_named_like_a_job_notice_still_interrupts() {
    let (_d, s) = store("name");
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO account(id, provider, address) VALUES (1, 'gmail', 'me@x.example');
             INSERT INTO thread(account_id, provider_id) VALUES (1, 't');
             INSERT INTO message(id, account_id, thread_id, provider_id) VALUES (1, 1, 1, 'm');
             INSERT INTO policy_version(version, text, sha256, confirmed_at) VALUES (1, '', '', 0);
             INSERT INTO notice(message_id, rule, rule_name, policy_version, title, text, created_at)
               VALUES (1, 1, 'Job question', 1, 'Ann', 'Hi', 0);",
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(s.read(|c| notice::after(c, 0, 50)).unwrap().len(), 1);
}
