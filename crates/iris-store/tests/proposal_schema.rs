//! Migration 30: `rule_proposal` holds the `hide` proposals of R5-02
//! (docs/specs/2026-11-17-preference-rules.md).

use std::fs;
use std::path::PathBuf;

use iris_store::Store;
use rusqlite::Connection;

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const MIGRATIONS: [&str; 29] = [
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
];

fn row(c: &Connection, verb: &str, value: &str, form: Option<&str>) -> rusqlite::Result<i64> {
    match form {
        None => c.execute(
            "INSERT INTO rule_proposal(verb, cond, value, approvals, created_at, updated_at)
             VALUES (?1, 'sender', ?2, 10, 0, 0)",
            rusqlite::params![verb, value],
        )?,
        Some(f) => c.execute(
            "INSERT INTO rule_proposal(verb, cond, value, form, approvals, created_at, updated_at)
             VALUES (?1, 'sender', ?2, ?3, 10, 0, 0)",
            rusqlite::params![verb, value, f],
        )?,
    };
    Ok(c.last_insert_rowid())
}

/// The migration keeps each row with its ID and state, and keeps the
/// highest ID ever given: with IDs 1 and 2 kept and ID 3 deleted, the next
/// proposal gets ID 4. The new key holds for `allow` rows (the form is
/// `''`, never NULL), and the CHECK refuses a `hide` row with no form.
#[test]
fn an_old_store_keeps_each_proposal_and_no_id_comes_again() {
    let dir = std::env::temp_dir().join(format!("iris-proposal-{}", std::process::id()));
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
        c.pragma_update(None, "user_version", 29).unwrap();
        for v in ["a@x.example", "b@x.example", "c@x.example"] {
            row(&c, "archive", v, None).unwrap();
        }
        c.execute(
            "UPDATE rule_proposal SET state = 'dismissed' WHERE id = 2",
            [],
        )
        .unwrap();
        c.execute("DELETE FROM rule_proposal WHERE id = 3", [])
            .unwrap();
    }
    let s = Store::open(&dir.0).unwrap();
    s.write(|tx| {
        assert_eq!(iris_store::schema_version(tx)?, 67);
        let rows: Vec<(i64, String, String)> = tx
            .prepare("SELECT id, state, form FROM rule_proposal ORDER BY id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(
            rows,
            vec![
                (1, "open".into(), String::new()),
                (2, "dismissed".into(), String::new())
            ]
        );
        assert_eq!(row(tx, "hide", "n@x.example", Some("feed"))?, 4);
        // The same allow rule again: the key holds.
        assert!(row(tx, "archive", "a@x.example", None).is_err());
        assert!(row(tx, "hide", "n@x.example", Some("feed")).is_err());
        assert!(
            row(tx, "hide", "m@x.example", None).is_err(),
            "a hide row needs a form"
        );
        assert!(row(tx, "hide", "m@x.example", Some("commitment")).is_err());
        assert!(row(tx, "archive", "m@x.example", Some("feed")).is_err());
        // The triggers of migration 15 came back.
        assert!(tx
            .execute("UPDATE rule_proposal SET state = 'open' WHERE id = 2", [])
            .is_err());
        assert!(tx
            .execute("DELETE FROM rule_proposal WHERE id = 2", [])
            .is_err());
        let ok: String = tx.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        assert_eq!(ok, "ok");
        Ok(())
    })
    .unwrap();
}
