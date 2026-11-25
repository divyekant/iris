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

/// The migration gives `iris/1.0` to each old item, with and without
/// `attention`, changes no other field, and leaves the attention queue as
/// it was.
#[test]
fn each_stored_item_gets_model_1_0_and_the_attention_queue_stays() {
    let dir = std::env::temp_dir().join(format!("iris-model-v1-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let dir = TempDir(dir);
    drop(Store::open(&dir.0).unwrap());
    let db = dir.0.join("iris.db");
    fs::remove_file(&db).unwrap();
    let old = json!({"model": "iris/0", "id": "c-1", "form": "commitment", "title": "Pay",
                     "attention": {"state": "hidden", "rule": 3, "reason": "r"}});
    // An item that the engine did not evaluate yet: no `attention`.
    let bare = json!({"model": "iris/0", "id": "c-3", "form": "commitment"});
    let docs = [
        (1, 1, old.to_string()),
        (2, 1, r#"{"model":"iris/1.0","id":"c-2"}"#.to_string()),
        (3, 2, bare.to_string()),
    ];
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
               VALUES (1, 1, 1, 'a', 1), (2, 1, 1, 'b', 2), (3, 1, 1, 'c', 3);",
        )
        .unwrap();
        for (id, message, doc) in &docs {
            c.execute(
                "INSERT INTO item (id, form, account_id, status, doc, created_at, updated_at)
                 VALUES (?1, 'commitment', 1, 'open', ?2, 1, 1)",
                rusqlite::params![id, doc],
            )
            .unwrap();
            c.execute(
                "INSERT INTO item_source (item_id, message_id, role) VALUES (?1, ?2, 'origin')",
                [id, message],
            )
            .unwrap();
        }
        // The queue before the start: one row, of another message.
        c.execute_batch("DELETE FROM attention_due; INSERT INTO attention_due VALUES (3);")
            .unwrap();
    }
    let s = Store::open(&dir.0).unwrap();
    s.write(|tx| {
        assert_eq!(iris_store::schema_version(tx)?, 55);
        let got: Vec<Value> = tx
            .prepare("SELECT doc FROM item ORDER BY id")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|d| serde_json::from_str(&d.unwrap()).unwrap())
            .collect();
        let v1 = |mut d: Value| {
            d["model"] = json!("iris/1.0"); // migration 31 writes 1.0
            d
        };
        assert_eq!(
            got,
            vec![
                v1(old.clone()),
                json!({"model": "iris/1.0", "id": "c-2"}),
                v1(bare.clone())
            ]
        );
        let due: Vec<i64> = tx
            .prepare("SELECT message_id FROM attention_due ORDER BY message_id")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(due, vec![3], "the migration keeps the queue as it was");
        Ok(())
    })
    .unwrap();
}
