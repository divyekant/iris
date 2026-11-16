//! Tests of the backup, its lock and the content cleanup (specification 1,
//! sections 4.5 and 5.1, and the test plan of section 10).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iris_store::{Body, Cleaned, Error, NewMessage, Store, GONE_KEEP_MS};
use rusqlite::Connection;

const NOW: i64 = 1_800_000_000_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-backup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A message with its content file written first.
fn msg(store: &Store, id: &str, raw: &[u8], text: &str) -> NewMessage {
    NewMessage {
        provider_id: id.into(),
        thread_provider_id: format!("t-{id}"),
        content_sha256: Some(store.put_content(raw).unwrap()),
        body: Some(Body {
            subject: "s".into(),
            sender: "a@example.com".into(),
            text: text.into(),
        }),
        ..Default::default()
    }
}

fn set_gone(store: &Store, id: i64, at: i64) {
    store
        .write(|tx| Ok(tx.execute("UPDATE message SET gone_at = ?1 WHERE id = ?2", [at, id])?))
        .unwrap();
}

fn cite(store: &Store, acc: i64, message: i64) {
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES ('record', ?1, 'open', '{}', 0, 0)",
                [acc],
            )?;
            tx.execute(
                "INSERT INTO item_source(item_id, message_id, role) VALUES (last_insert_rowid(), ?1, 'origin')",
                [message],
            )?;
            Ok(())
        })
        .unwrap();
}

fn count(store: &Store, sql: &str, id: i64) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [id], |r| r.get(0))?))
        .unwrap()
}

/// Each content file that the database in `dir` refers to is in `dir`.
fn missing_files(dir: &Path) -> Vec<String> {
    let c = Connection::open(dir.join("iris.db")).unwrap();
    let mut st = c
        .prepare("SELECT DISTINCT content_sha256 FROM message WHERE content_sha256 IS NOT NULL")
        .unwrap();
    let shas: Vec<String> = st
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    shas.into_iter()
        .filter(|s| {
            !dir.join("content")
                .join(&s[..2])
                .join(format!("{s}.eml"))
                .exists()
        })
        .collect()
}

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn the_cleanup_removes_old_gone_content_and_keeps_the_rest() {
    let dir = TempDir::new("clean");
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    let ids = store
        .put_messages(
            acc,
            &[
                msg(&store, "old", b"only old", "apple"),
                msg(&store, "cited", b"only cited", "pear"),
                msg(&store, "shared-gone", b"shared", "plum"),
                msg(&store, "shared-live", b"shared", "plum"),
                msg(&store, "young", b"only young", "fig"),
            ],
        )
        .unwrap();
    let [old, cited, shared_gone, shared_live, young] = ids[..] else {
        panic!()
    };
    let expired = NOW - GONE_KEEP_MS;
    for id in [old, cited, shared_gone] {
        set_gone(&store, id, expired);
    }
    set_gone(&store, young, expired + 1);
    cite(&store, acc, cited);

    let done = store.clean_gone(NOW).unwrap().unwrap();
    assert_eq!(
        done,
        Cleaned {
            messages: 3,
            files: 2,
            tombstones: 2
        }
    );

    // No message row is left for the old message and for the shared one.
    assert_eq!(
        count(&store, "SELECT count(*) FROM message WHERE id = ?1", old),
        0
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM message WHERE id = ?1",
            shared_gone
        ),
        0
    );
    // The cited message stays as a tombstone with no content, header or text.
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM message WHERE id = ?1 AND content_sha256 IS NULL AND gone_at IS NOT NULL",
            cited
        ),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM header WHERE message_id = ?1",
            cited
        ),
        0
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM body WHERE message_id = ?1",
            cited
        ),
        0
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM message_fts WHERE message_fts MATCH 'pear' AND rowid = ?1",
            cited
        ),
        0
    );
    // A file that a live message refers to stays. A message gone for less
    // than 30 days keeps its content.
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM body WHERE message_id = ?1",
            shared_live
        ),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM body WHERE message_id = ?1",
            young
        ),
        1
    );
    assert!(missing_files(&dir.0).is_empty());

    // A second run finds nothing more.
    assert_eq!(store.clean_gone(NOW).unwrap().unwrap(), Cleaned::default());
}

#[test]
fn the_cleanup_deletes_the_file_of_old_gone_mail() {
    let dir = TempDir::new("cleanfile");
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    let m = msg(&store, "a", b"raw a", "apple");
    let file = store
        .content_path(m.content_sha256.as_ref().unwrap())
        .unwrap();
    let id = store.put_messages(acc, &[m]).unwrap()[0];
    set_gone(&store, id, NOW - GONE_KEEP_MS);
    assert!(file.exists());
    store.clean_gone(NOW).unwrap().unwrap();
    assert!(!file.exists());
}

#[test]
fn the_cleanup_does_not_start_during_a_backup() {
    let dir = TempDir::new("cleanlocked");
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    let id = store
        .put_messages(acc, &[msg(&store, "a", b"raw a", "apple")])
        .unwrap()[0];
    set_gone(&store, id, 0);

    let held = store.backup_lock().unwrap();
    assert_eq!(store.clean_gone(NOW).unwrap(), None);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM body WHERE message_id = ?1",
            id
        ),
        1
    );
    drop(held);
    assert!(store.clean_gone(NOW).unwrap().is_some());
}

#[test]
fn a_cleanup_during_a_live_backup_loses_no_content_file() {
    let dir = TempDir::new("live");
    let store = Arc::new(Store::open(&dir.0).unwrap());
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    let batch: Vec<NewMessage> = (0..400)
        .map(|i| {
            msg(
                &store,
                &format!("m{i}"),
                format!("raw {i}").as_bytes(),
                "apple",
            )
        })
        .collect();
    let ids = store.put_messages(acc, &batch).unwrap();
    for id in ids.iter().step_by(2) {
        set_gone(&store, *id, 0);
    }

    for round in 0..4 {
        let dest = dir.0.with_file_name(format!(
            "iris-backup-live-copy{round}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dest);
        let s = Arc::clone(&store);
        let d = dest.clone();
        let backup = std::thread::spawn(move || s.backup(&d));
        let cleaner = {
            let s = Arc::clone(&store);
            std::thread::spawn(move || {
                for _ in 0..50 {
                    s.clean_gone(NOW).unwrap();
                }
            })
        };
        backup.join().unwrap().unwrap();
        cleaner.join().unwrap();
        assert_eq!(missing_files(&dest), Vec::<String>::new());
        fs::remove_dir_all(&dest).unwrap();
        // Make more gone mail for the next round.
        store
            .write(
                |tx| Ok(tx.execute("UPDATE message SET gone_at = 0 WHERE id % 7 = ?1", [round])?),
            )
            .unwrap();
    }
}

#[test]
fn a_backup_restores_to_a_new_folder_with_secrets_and_policy() {
    let dir = TempDir::new("restore");
    let dest = TempDir::new("restore-copy");
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    let token = store.seal(b"refresh token").unwrap();
    store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE account SET token = ?1 WHERE id = ?2",
                rusqlite::params![token, acc],
            )?)
        })
        .unwrap();
    store.write_policy(b"[rules]\nsend = \"hold\"\n").unwrap();
    let id = store
        .put_messages(acc, &[msg(&store, "a", b"raw a", "apple")])
        .unwrap()[0];

    assert_eq!(store.backup(&dest.0).unwrap(), 1);
    drop(store);
    // The restore is the copy at a new path: the engine opens it as it is.
    let restored = Store::open(&dest.0).unwrap();
    let sealed: Vec<u8> = restored
        .read(|c| {
            Ok(
                c.query_row("SELECT token FROM account WHERE id = ?1", [acc], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(restored.unseal(&sealed).unwrap(), b"refresh token");
    assert_eq!(
        fs::read_to_string(dest.0.join("policy.toml")).unwrap(),
        "[rules]\nsend = \"hold\"\n"
    );
    assert_eq!(restored.search("apple", 10).unwrap(), [id]);
    assert!(missing_files(&dest.0).is_empty());
    #[cfg(unix)]
    {
        assert_eq!(mode(&dest.0), 0o700);
        assert_eq!(mode(&dest.0.join("key")), 0o600);
        assert_eq!(mode(&dest.0.join("policy.toml")), 0o600);
    }
}

#[test]
fn a_backup_refuses_a_folder_that_is_not_empty() {
    let dir = TempDir::new("full");
    let dest = TempDir::new("full-copy");
    let store = Store::open(&dir.0).unwrap();
    fs::create_dir_all(&dest.0).unwrap();
    fs::write(dest.0.join("other"), b"x").unwrap();
    assert!(matches!(store.backup(&dest.0), Err(Error::Io(_))));
    assert_eq!(fs::read(dest.0.join("other")).unwrap(), b"x");
}

#[test]
fn the_policy_write_waits_for_a_backup() {
    let dir = TempDir::new("policy");
    let store = Arc::new(Store::open(&dir.0).unwrap());
    let held = store.backup_lock().unwrap();
    let s = Arc::clone(&store);
    let writer = std::thread::spawn(move || s.write_policy(b"v2").unwrap());
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        !dir.0.join("policy.toml").exists(),
        "no write during a backup"
    );
    drop(held);
    writer.join().unwrap();
    assert_eq!(fs::read(dir.0.join("policy.toml")).unwrap(), b"v2");
}

#[test]
fn a_message_with_a_missing_content_file_writes_nothing() {
    let dir = TempDir::new("missing");
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    let good = msg(&store, "a", b"raw a", "apple");
    let mut bad = msg(&store, "b", b"raw b", "pear");
    fs::remove_file(
        store
            .content_path(bad.content_sha256.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        store.put_messages(acc, &[good, bad.clone()]),
        Err(Error::MissingContent(_))
    ));
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM message WHERE account_id = ?1",
            acc
        ),
        0
    );
    bad.content_sha256 = Some("not a sha".into());
    assert!(matches!(
        store.put_messages(acc, &[bad]),
        Err(Error::MissingContent(_))
    ));
}
