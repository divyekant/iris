//! Migration 17: the verbs `draft` and `send`, and the tables `draft`,
//! `action_draft` and `ledger_send` (docs/specs/2026-11-16-send.md).

use std::fs;
use std::path::PathBuf;

use iris_store::Store;
use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-send-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn add_action(c: &Connection, verb: &str) -> rusqlite::Result<usize> {
    c.execute(
        "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0)",
        [],
    )?;
    let cs = c.last_insert_rowid();
    c.execute(
        "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
         VALUES (?1, 1, 1, ?2, '{}', '{}')",
        rusqlite::params![cs, verb],
    )
}

/// An approved action of a new changeset, as the claim of a ledger entry
/// needs it. Gives its row ID.
fn approved(c: &Connection, verb: &str) -> rusqlite::Result<i64> {
    add_action(c, verb)?;
    let id = c.last_insert_rowid();
    c.execute_batch(&format!(
        "UPDATE changeset_action SET hash = 'h' WHERE id = {id};
         UPDATE changeset SET why = 'w', submitted_at = 0
           WHERE id = (SELECT changeset_id FROM changeset_action WHERE id = {id});
         UPDATE changeset_action SET state = 'approved', decided_by = 'person', decided_at = 1
           WHERE id = {id};"
    ))?;
    Ok(id)
}

/// The migration of an old store keeps its rows, accepts the new verbs on
/// the connection that migrated and on a new one, and still refuses an
/// unknown verb.
#[test]
fn an_old_store_gets_the_new_verbs() {
    let dir = TempDir::new("old");
    drop(Store::open(&dir.0).unwrap());
    let db = dir.0.join("iris.db");
    fs::remove_file(&db).unwrap();
    {
        let c = Connection::open(&db).unwrap();
        for sql in [
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
        ] {
            c.execute_batch(sql).unwrap();
        }
        c.pragma_update(None, "user_version", 16).unwrap();
        add_action(&c, "archive").unwrap();
        assert!(add_action(&c, "send").is_err());
    }
    let s = Store::open(&dir.0).unwrap();
    s.write(|tx| {
        assert_eq!(iris_store::schema_version(tx)?, 17);
        let n: i64 = tx.query_row(
            "SELECT count(*) FROM changeset_action WHERE verb = 'archive'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(n, 1);
        add_action(tx, "draft")?;
        add_action(tx, "send")?;
        assert!(add_action(tx, "nudge").is_err());
        let ok: String = tx.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        assert_eq!(ok, "ok");
        let w: i64 = tx.query_row("PRAGMA writable_schema", [], |r| r.get(0))?;
        assert_eq!(w, 0);
        Ok(())
    })
    .unwrap();
    drop(s);
    let c = Connection::open(&db).unwrap();
    add_action(&c, "send").unwrap();
    assert!(add_action(&c, "nudge").is_err());
}

/// A new store takes the verbs at once, and `ledger_send` allows one send
/// row for each draft.
#[test]
fn one_send_row_for_each_draft() {
    let dir = TempDir::new("one");
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    s.write(|tx| {
        add_action(tx, "draft")?;
        let action = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                               action_id, created_at, updated_at)
             VALUES (?1, 'r1', 'iris.1@example.com', '{}', 'h', x'00', ?2, 0, 0)",
            rusqlite::params![a, action],
        )?;
        let draft = tx.last_insert_rowid();
        let send = approved(tx, "send")?;
        tx.execute(
            "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
             VALUES (?1, ?2, '{}', 'writing', 'none', 0)",
            rusqlite::params![send, a],
        )?;
        let first = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO ledger_send(draft_id, ledger_id) VALUES (?1, ?2)",
            [draft, first],
        )?;
        let send = approved(tx, "send")?;
        tx.execute(
            "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
             VALUES (?1, ?2, '{}', 'writing', 'none', 0)",
            rusqlite::params![send, a],
        )?;
        let second = tx.last_insert_rowid();
        assert!(tx
            .execute(
                "INSERT INTO ledger_send(draft_id, ledger_id) VALUES (?1, ?2)",
                [draft, second]
            )
            .is_err());
        Ok(())
    })
    .unwrap();
}
