//! Migration 18: the verbs `draft` and `send`, and the tables `draft`,
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
            include_str!("../src/schema_17.sql"),
        ] {
            c.execute_batch(sql).unwrap();
        }
        c.pragma_update(None, "user_version", 17).unwrap();
        add_action(&c, "archive").unwrap();
        assert!(add_action(&c, "send").is_err());
    }
    let s = Store::open(&dir.0).unwrap();
    s.write(|tx| {
        assert_eq!(iris_store::schema_version(tx)?, 46);
        let n: i64 = tx.query_row(
            "SELECT count(*) FROM changeset_action WHERE verb = 'archive'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(n, 1);
        add_action(tx, "draft")?;
        add_action(tx, "send")?;
        // Migration 26 (issue #226): the verb of the undo of a state entry.
        add_action(tx, "restore")?;
        // Migration 29 (R5-02) widens the same CHECK again.
        for verb in ["skip", "mute", "dismiss"] {
            add_action(tx, verb)?;
        }
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
    add_action(&c, "dismiss").unwrap();
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

/// The draft of a submitted action does not change: no new binding, no
/// change and no delete, also for two drafts with the same hash.
#[test]
fn the_draft_of_a_submitted_send_is_fixed() {
    let dir = TempDir::new("fixed");
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    s.write(|tx| {
        let mut drafts = Vec::new();
        for n in 1..=2 {
            add_action(tx, "draft")?;
            let action = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                                   action_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, '{}', 'same', x'00', ?4, 0, 0)",
                rusqlite::params![a, format!("r{n}"), format!("iris.{n}@example.com"), action],
            )?;
            drafts.push(tx.last_insert_rowid());
        }
        // Before submit, the binding can change.
        add_action(tx, "send")?;
        let open = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO action_draft VALUES (?1, ?2)",
            [open, drafts[0]],
        )?;
        tx.execute("DELETE FROM action_draft WHERE action_id = ?1", [open])?;

        let send = approved(tx, "send")?;
        assert!(tx
            .execute(
                "INSERT INTO action_draft VALUES (?1, ?2)",
                [send, drafts[0]]
            )
            .is_err());
        Ok(())
    })
    .unwrap();
    // A row that submit wrote before the state stays fixed after it.
    s.write(|tx| {
        add_action(tx, "send")?;
        let send = tx.last_insert_rowid();
        let d1: i64 = tx.query_row("SELECT min(id) FROM draft", [], |r| r.get(0))?;
        tx.execute("INSERT INTO action_draft VALUES (?1, ?2)", [send, d1])?;
        tx.execute_batch(&format!(
            "UPDATE changeset_action SET hash = 'h' WHERE id = {send};
             UPDATE changeset SET why = 'w', submitted_at = 0
               WHERE id = (SELECT changeset_id FROM changeset_action WHERE id = {send});
             UPDATE changeset_action SET state = 'approved', decided_by = 'person', decided_at = 1
               WHERE id = {send};"
        ))?;
        assert!(tx
            .execute(
                "UPDATE action_draft SET draft_id = draft_id + 1 WHERE action_id = ?1",
                [send]
            )
            .is_err());
        assert!(tx
            .execute("DELETE FROM action_draft WHERE action_id = ?1", [send])
            .is_err());
        Ok(())
    })
    .unwrap();
}
