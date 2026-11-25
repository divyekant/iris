//! Migration 63: job rows in `notice` (issues #844 and #845,
//! docs/specs/2026-11-25-job-notices.md).

use std::fs;
use std::path::PathBuf;

use iris_store::Store;
use rusqlite::{params, Connection};

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const MIGRATIONS: [&str; 62] = [
    include_str!("../src/schema_1.sql"),
    include_str!("../src/schema_2.sql"),
    include_str!("../src/schema_3.sql"),
    include_str!("../src/schema_4.sql"),
    include_str!("../src/schema_5.sql"),
    include_str!("../src/schema_6.sql"),
    include_str!("../src/schema_7.sql"),
    include_str!("../src/schema_8.sql"),
    include_str!("../src/schema_9.sql"),
    include_str!("../src/schema_10.sql"),
    include_str!("../src/schema_11.sql"),
    include_str!("../src/schema_12.sql"),
    include_str!("../src/schema_13.sql"),
    include_str!("../src/schema_14.sql"),
    include_str!("../src/schema_15.sql"),
    include_str!("../src/schema_16.sql"),
    include_str!("../src/schema_17.sql"),
    include_str!("../src/schema_18.sql"),
    include_str!("../src/schema_19.sql"),
    include_str!("../src/schema_20.sql"),
    include_str!("../src/schema_21.sql"),
    include_str!("../src/schema_22.sql"),
    include_str!("../src/schema_23.sql"),
    include_str!("../src/schema_24.sql"),
    include_str!("../src/schema_25.sql"),
    include_str!("../src/schema_26.sql"),
    include_str!("../src/schema_27.sql"),
    include_str!("../src/schema_28.sql"),
    include_str!("../src/schema_29.sql"),
    include_str!("../src/schema_30.sql"),
    include_str!("../src/schema_31.sql"),
    include_str!("../src/schema_32.sql"),
    include_str!("../src/schema_33.sql"),
    include_str!("../src/schema_34.sql"),
    include_str!("../src/schema_35.sql"),
    include_str!("../src/schema_36.sql"),
    include_str!("../src/schema_37.sql"),
    include_str!("../src/schema_38.sql"),
    include_str!("../src/schema_39.sql"),
    include_str!("../src/schema_40.sql"),
    include_str!("../src/schema_41.sql"),
    include_str!("../src/schema_42.sql"),
    include_str!("../src/schema_43.sql"),
    include_str!("../src/schema_44.sql"),
    include_str!("../src/schema_45.sql"),
    include_str!("../src/schema_46.sql"),
    include_str!("../src/schema_47.sql"),
    include_str!("../src/schema_48.sql"),
    include_str!("../src/schema_49.sql"),
    include_str!("../src/schema_50.sql"),
    include_str!("../src/schema_51.sql"),
    include_str!("../src/schema_52.sql"),
    include_str!("../src/schema_53.sql"),
    include_str!("../src/schema_54.sql"),
    include_str!("../src/schema_55.sql"),
    include_str!("../src/schema_56.sql"),
    include_str!("../src/schema_57.sql"),
    include_str!("../src/schema_58.sql"),
    include_str!("../src/schema_59.sql"),
    include_str!("../src/schema_60.sql"),
    include_str!("../src/schema_61.sql"),
    include_str!("../src/schema_62.sql"),
];

fn rule_row(c: &Connection, message: i64) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO notice(message_id, rule, rule_name, policy_version, title, text, created_at)
         VALUES (?1, 1, 'Boss', 1, 'Ann', 'Plan', 5)",
        [message],
    )?;
    Ok(c.last_insert_rowid())
}

/// An old store keeps each rule notice with its ID and state, and keeps the
/// highest ID ever given. Each move of a job to `needs_you` adds one job
/// row in the same statement; the CHECKs refuse a mixed row.
#[test]
fn an_old_store_keeps_its_notices_and_a_job_move_adds_a_row() {
    let dir = std::env::temp_dir().join(format!("iris-notice-schema-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let dir = TempDir(dir);
    drop(Store::open(&dir.0).unwrap());
    let db = dir.0.join("iris.db");
    fs::remove_file(&db).unwrap();
    {
        let c = Connection::open(&db).unwrap();
        for sql in MIGRATIONS {
            c.execute_batch(sql).unwrap();
        }
        c.pragma_update(None, "user_version", 62).unwrap();
        for m in [10, 11, 12] {
            rule_row(&c, m).unwrap();
        }
        c.execute_batch(
            "UPDATE notice SET done_at = 7 WHERE id = 2; DELETE FROM notice WHERE id = 3;",
        )
        .unwrap();
    }
    let s = Store::open(&dir.0).unwrap();
    s.write(|tx| {
        assert_eq!(iris_store::schema_version(tx)?, 67);
        let rows: Vec<(i64, i64, Option<i64>)> = tx
            .prepare("SELECT id, message_id, done_at FROM notice ORDER BY id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(rows, vec![(1, 10, None), (2, 11, Some(7))]);
        // ID 3 never comes again.
        assert_eq!(rule_row(tx, 13)?, 4);

        // A job, its question, an answer, and the due date.
        tx.execute(
            "INSERT INTO job (goal, created_at, state, updated_at, due_at)
             VALUES ('Get the contract.', 100, 'working', 100, 350)",
            [],
        )?;
        let job = tx.last_insert_rowid();
        let event = |at: i64, state: &str, by: &str, q: Option<&str>, a: Option<&str>| {
            let why = (by == "engine").then_some("due");
            tx.execute(
                "INSERT INTO job_event (job_id, at, state, by, question, answer, why)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![job, at, state, by, q, a, why],
            )
        };
        event(200, "needs_you", "agent", Some("Which day?"), None)?;
        event(300, "working", "person", None, Some("Tuesday."))?;
        event(400, "needs_you", "engine", Some("The due date passed."), None)?;
        let jobs: Vec<(i64, i64, String, String, String, i64)> = tx
            .prepare(
                "SELECT id, job_id, rule_name, title, text, created_at FROM notice
                 WHERE job_event_id IS NOT NULL ORDER BY id",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let title = format!("j-{job}");
        assert_eq!(
            jobs,
            vec![
                (5, job, "Job question".into(), title.clone(), "Get the contract.".into(), 200),
                (6, job, "Job due".into(), title, "Get the contract.".into(), 400),
            ]
        );
        // A refused move adds no row.
        assert!(event(500, "waiting", "person", None, None).is_err());
        let n: i64 = tx.query_row("SELECT count(*) FROM notice", [], |r| r.get(0))?;
        assert_eq!(n, 5);
        // A row with a mail and a job, or with neither, or a job row with a rule.
        for bad in [
            "INSERT INTO notice(message_id, job_id, job_event_id, rule, rule_name, policy_version, title, text, created_at)
             VALUES (20, 1, 99, 1, 'x', 1, 't', 't', 1)",
            "INSERT INTO notice(rule_name, title, text, created_at) VALUES ('x', 't', 't', 1)",
            "INSERT INTO notice(job_id, job_event_id, rule, rule_name, title, text, created_at)
             VALUES (1, 98, 1, 'x', 't', 't', 1)",
            "INSERT INTO notice(job_event_id, rule_name, title, text, created_at) VALUES (97, 'x', 't', 't', 1)",
        ] {
            assert!(tx.execute(bad, []).is_err(), "{bad}");
        }
        let ok: String = tx.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        assert_eq!(ok, "ok");
        Ok(())
    })
    .unwrap();
}
