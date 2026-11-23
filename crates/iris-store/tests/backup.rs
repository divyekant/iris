//! Tests of the backup, its lock and the content cleanup (specification 1,
//! sections 4.5 and 5.1, and the test plan of section 10).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iris_store::{Body, Cleaned, Error, NewMessage, PolicyExpect, Store, GONE_KEEP_MS};
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
    let lock = store.backup_lock().unwrap();
    store
        .write_policy(
            &lock,
            PolicyExpect::Any,
            Some(b"[rules]\nsend = \"hold\"\n"),
        )
        .unwrap();
    drop(lock);
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
    let writer = std::thread::spawn(move || {
        let lock = s.backup_lock().unwrap();
        s.write_policy(&lock, PolicyExpect::Any, Some(b"v2"))
            .unwrap()
    });
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

/// The files of the data folder whose names start with `policy.toml`.
fn policy_files(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("policy.toml")
        })
        .map(|p| {
            (
                p.file_name().unwrap().to_str().unwrap().to_string(),
                fs::read(&p).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a_policy_write_never_loses_a_file_text() {
    // Invariant: each text that was at policy.toml is the expected text, is
    // back at policy.toml, or stays as a saved copy.
    let dir = TempDir::new("policy-keep");
    let store = Store::open(&dir.0).unwrap();
    let lock = store.backup_lock().unwrap();
    let path = dir.0.join("policy.toml");
    let write = |expect, text: Option<&[u8]>| store.write_policy(&lock, expect, text).unwrap();

    // No file is expected, and there is none.
    assert!(write(PolicyExpect::Missing, Some(b"v1")).done);
    assert_eq!(
        policy_files(&dir.0),
        [("policy.toml".into(), b"v1".to_vec())]
    );
    // The expected text: replaced, and the old file stays as a copy.
    let w = write(PolicyExpect::Text(b"v1"), Some(b"v2"));
    assert!(w.done);
    assert_eq!(fs::read(w.saved.as_ref().unwrap()).unwrap(), b"v1");
    fs::remove_file(w.saved.unwrap()).unwrap();
    assert_eq!(
        policy_files(&dir.0),
        [("policy.toml".into(), b"v2".to_vec())]
    );
    // Another text than the expected one: it goes back.
    for expect in [PolicyExpect::Text(b"v1"), PolicyExpect::Missing] {
        let got = write(expect, Some(b"v3"));
        assert!(!got.done && got.saved.is_none());
        assert_eq!(
            policy_files(&dir.0),
            [("policy.toml".into(), b"v2".to_vec())]
        );
    }
    // A file was expected and there is none: nothing is written.
    fs::remove_file(&path).unwrap();
    assert!(!write(PolicyExpect::Text(b"v2"), Some(b"v3")).done);
    assert!(policy_files(&dir.0).is_empty());

    // Each discard keeps its own copy.
    fs::write(&path, b"edit a").unwrap();
    let a = write(PolicyExpect::Any, Some(b"v2"));
    fs::write(&path, b"edit b").unwrap();
    let b = write(PolicyExpect::Any, None);
    assert!(a.done && b.done);
    assert_ne!(a.saved, b.saved);
    assert_eq!(fs::read(a.saved.unwrap()).unwrap(), b"edit a");
    assert_eq!(fs::read(b.saved.unwrap()).unwrap(), b"edit b");
    assert!(!path.exists());
    assert!(!dir.0.join("policy.toml.tmp").exists());
}

#[test]
fn a_save_through_an_open_file_is_never_lost() {
    // Invariant: the write never removes the last name of the file that was
    // at policy.toml. An editor can hold that file open and save into it
    // after the check, so its content must stay reachable by a name.
    use std::io::{Seek, Write};
    let cases: [(PolicyExpect, Option<&[u8]>); 6] = [
        (PolicyExpect::Text(b"old"), Some(b"new")),
        (PolicyExpect::Text(b"other"), Some(b"new")),
        (PolicyExpect::Missing, Some(b"new")),
        (PolicyExpect::Any, Some(b"new")),
        (PolicyExpect::Any, None),
        (PolicyExpect::Text(b"old"), None),
    ];
    for (i, (expect, text)) in cases.into_iter().enumerate() {
        let dir = TempDir::new(&format!("policy-open-{i}"));
        let store = Store::open(&dir.0).unwrap();
        let lock = store.backup_lock().unwrap();
        let path = dir.0.join("policy.toml");
        fs::write(&path, b"old").unwrap();
        let mut editor = fs::OpenOptions::new().write(true).open(&path).unwrap();
        store.write_policy(&lock, expect, text).unwrap();
        editor.rewind().unwrap();
        editor.write_all(b"late edit").unwrap();
        editor.sync_all().unwrap();
        let files = policy_files(&dir.0);
        assert!(
            files.iter().any(|(_, t)| t == b"late edit"),
            "case {i}: {files:?}"
        );
    }
}

#[test]
fn a_write_after_a_crash_never_truncates_the_policy_file() {
    // Invariant: the policy write never opens a file that exists for
    // writing. After a crash between the link and the cleanup, the
    // temporary name and policy.toml are the same file, which the person
    // can edit.
    let dir = TempDir::new("policy-crash");
    let store = Store::open(&dir.0).unwrap();
    let lock = store.backup_lock().unwrap();
    let path = dir.0.join("policy.toml");
    fs::write(&path, b"v1").unwrap();
    fs::hard_link(&path, dir.0.join("policy.toml.tmp")).unwrap();
    fs::write(&path, b"edit").unwrap();
    for (expect, text) in [
        (PolicyExpect::Text(b"v1"), Some(&b"v2"[..])),
        (PolicyExpect::Any, Some(&b"v2"[..])),
    ] {
        store.write_policy(&lock, expect, text).unwrap();
        let files = policy_files(&dir.0);
        assert!(files.iter().any(|(_, t)| t == b"edit"), "{files:?}");
    }
}

// Invariant (#63): a backup that stops with an error leaves no file at the
// destination and no partial copy next to it, so a new backup to the same
// path can start.
#[test]
fn a_failed_backup_leaves_no_partial_copy() {
    let dir = TempDir::new("partial");
    let dest = TempDir::new("partial-copy");
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .put_messages(acc, &[msg(&store, "a", b"raw a", "apple")])
        .unwrap();
    // A content file that is not there stops the copy after iris.db.
    let content = dir.0.join("content");
    let moved = dir.0.join("content-aside");
    fs::rename(&content, &moved).unwrap();
    fs::create_dir(&content).unwrap();
    assert!(store.backup(&dest.0).is_err());
    assert!(!dest.0.exists(), "the destination has a partial copy");
    let parent = dest.0.parent().unwrap();
    let name = dest.0.file_name().unwrap().to_string_lossy().to_string();
    assert!(!fs::read_dir(parent).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(&format!(".{name}.partial"))));
    // The retry to the same path works.
    fs::remove_dir(&content).unwrap();
    fs::rename(&moved, &content).unwrap();
    assert_eq!(store.backup(&dest.0).unwrap(), 1);
    assert!(missing_files(&dest.0).is_empty());
}
