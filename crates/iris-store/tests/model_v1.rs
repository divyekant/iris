//! Migration 31: version 1 of the open model
//! (docs/specs/2026-11-17-open-model-v1.md). Each stored item gets
//! `model` `iris/1.0`, keeps each other field, and no item goes to the
//! attention queue.

use std::fs;
use std::path::PathBuf;

use iris_store::Store;
use rusqlite::Connection;
use serde_json::{json, Value};

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const MIGRATIONS: [&str; 30] = [
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
];

#[test]
fn each_stored_item_gets_model_1_0_and_keeps_its_attention() {
    let dir = std::env::temp_dir().join(format!("iris-model-v1-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let dir = TempDir(dir);
    drop(Store::open(&dir.0).unwrap());
    let db = dir.0.join("iris.db");
    fs::remove_file(&db).unwrap();
    let old = json!({"model": "iris/0", "id": "c-1", "form": "commitment", "title": "Pay",
                     "attention": {"state": "hidden", "rule": 3, "reason": "r"}});
    {
        let c = Connection::open(&db).unwrap();
        for sql in MIGRATIONS {
            c.execute_batch(sql).unwrap();
        }
        c.pragma_update(None, "user_version", 30).unwrap();
        c.execute_batch(
            "INSERT INTO account (id, provider, address) VALUES (1, 'gmail', 'me@example.com');
             INSERT INTO thread (id, account_id, provider_id) VALUES (1, 1, 't');
             INSERT INTO message (id, account_id, thread_id, provider_id, internal_date)
               VALUES (1, 1, 1, 'a', 1);",
        )
        .unwrap();
        for (id, doc) in [
            (1, old.to_string()),
            (2, r#"{"model":"iris/1.0","id":"c-2"}"#.into()),
        ] {
            c.execute(
                "INSERT INTO item (id, form, account_id, status, doc, created_at, updated_at)
                 VALUES (?1, 'commitment', 1, 'open', ?2, 1, 1)",
                rusqlite::params![id, doc],
            )
            .unwrap();
            c.execute(
                "INSERT INTO item_source (item_id, message_id, role) VALUES (?1, 1, 'origin')",
                [id],
            )
            .unwrap();
        }
        c.execute("DELETE FROM attention_due", []).unwrap();
    }
    let s = Store::open(&dir.0).unwrap();
    s.write(|tx| {
        assert_eq!(iris_store::schema_version(tx)?, 31);
        let docs: Vec<Value> = tx
            .prepare("SELECT doc FROM item ORDER BY id")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|d| serde_json::from_str(&d.unwrap()).unwrap())
            .collect();
        let mut want = old.clone();
        want["model"] = json!(iris_store::MODEL);
        assert_eq!(docs, vec![want, json!({"model": "iris/1.0", "id": "c-2"})]);
        let due: i64 = tx.query_row("SELECT count(*) FROM attention_due", [], |r| r.get(0))?;
        assert_eq!(due, 0, "the migration must not queue the attention");
        Ok(())
    })
    .unwrap();
}
