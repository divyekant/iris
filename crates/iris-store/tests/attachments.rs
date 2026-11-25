//! The attachment cleanup (spec docs/specs/2026-11-25-attachment-cleanup.md,
//! issue #853): an old file that no live reference names goes, and each
//! file that a live action, claim or draft names stays.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iris_store::{Store, GONE_KEEP_MS};
use rusqlite::{params, Connection};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-attach-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const NOW: i64 = 1_800_000_000_000;

/// The content JSON with one attachment entry.
fn content(sha: &str) -> String {
    serde_json::json!({"to": ["a@example.com"], "subject": "s", "body": "b",
        "attachments": [{"name": "f.pdf", "type": "application/pdf", "sha256": sha, "size": 1}]})
    .to_string()
}

/// A `draft` action with these args in a new changeset. With `state`, the
/// action moves to `approved` first, and then to `denied` for "denied".
fn action(c: &Connection, args: &str, state: Option<&str>) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO changeset(author_kind, created_at) VALUES ('person', 0)",
        [],
    )?;
    let cs = c.last_insert_rowid();
    c.execute(
        "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
         VALUES (?1, 1, 1, 'draft', '{}', ?2)",
        params![cs, args],
    )?;
    let id = c.last_insert_rowid();
    let Some(state) = state else { return Ok(id) };
    c.execute_batch(&format!(
        "UPDATE changeset_action SET hash = 'h' WHERE id = {id};
         UPDATE changeset SET why = 'w', submitted_at = 0 WHERE id = {cs};"
    ))?;
    if state == "denied" {
        c.execute_batch(&format!(
            "UPDATE changeset_action SET state = 'in_review', decided_by = 'engine', decided_at = 1
               WHERE id = {id};
             UPDATE changeset_action SET state = 'denied', decided_by = 'person', decided_at = 1, reason = 'no'
               WHERE id = {id};"
        ))?;
    } else {
        c.execute_batch(&format!(
            "UPDATE changeset_action SET state = 'approved', decided_by = 'person', decided_at = 1
               WHERE id = {id};"
        ))?;
    }
    Ok(id)
}

/// Write a file with these bytes and set its time `age_ms` before `NOW`.
fn file(s: &Store, bytes: &[u8], age_ms: i64) -> String {
    let sha = s.put_attachment(bytes).unwrap();
    age(s, &sha, age_ms);
    sha
}

fn age(s: &Store, sha: &str, age_ms: i64) {
    let t = UNIX_EPOCH + Duration::from_millis((NOW - age_ms) as u64);
    fs::File::options()
        .write(true)
        .open(s.attachment_path(sha).unwrap())
        .unwrap()
        .set_modified(t)
        .unwrap();
}

fn exists(s: &Store, sha: &str) -> bool {
    s.attachment_path(sha).unwrap().exists()
}

const OLD: i64 = GONE_KEEP_MS + 1;

/// Each live place keeps its file at any age. An old file that only a
/// decided action, a sent draft or a foreign draft names goes, as does an
/// old file that nothing names. A new file and each other name stay.
#[test]
fn only_old_dead_files_go() {
    let dir = TempDir::new("live");
    let s = Store::open(&dir.0).unwrap();
    let account = s.add_account("gmail", "owner@example.com").unwrap();
    let revision = file(&s, b"revision", OLD);
    let approved = file(&s, b"approved", OLD);
    let claim = file(&s, b"claim", OLD);
    let draft = file(&s, b"draft", OLD);
    let denied = file(&s, b"denied", OLD);
    let sent = file(&s, b"sent", OLD);
    let foreign = file(&s, b"foreign", OLD);
    let none = file(&s, b"none", OLD);
    let new = file(&s, b"new", GONE_KEEP_MS - 60_000);
    s.write(|tx| {
        action(tx, &content(&revision), None)?;
        action(tx, &content(&approved), Some("approved"))?;
        action(tx, &content(&denied), Some("denied"))?;
        // The claim of an approved action whose args name no file.
        let a = action(tx, "{}", Some("approved"))?;
        tx.execute(
            "INSERT INTO ledger(action_id, account_id, ops, state, undo, at, post)
             VALUES (?1, ?2, '{}', 'writing', 'none', 0, ?3)",
            params![a, account, format!("{{\"content\":{}}}", content(&claim))],
        )?;
        for (n, (sha, sent_at, foreign_at)) in [
            (&draft, None, None),
            (&sent, Some(1), None),
            (&foreign, None, Some(1)),
        ]
        .into_iter()
        .enumerate()
        {
            let a = action(tx, "{}", Some("denied"))?;
            tx.execute(
                "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                                   action_id, sent_at, sent_message, foreign_at,
                                   created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'h', x'00', ?5, ?6, ?7, ?8, 0, 0)",
                params![
                    account,
                    format!("r{n}"),
                    format!("iris.{n}@example.com"),
                    format!("{{\"content\":{}}}", content(sha)),
                    a,
                    sent_at,
                    sent_at.map(|_| "m"),
                    foreign_at
                ],
            )?;
        }
        Ok(())
    })
    .unwrap();
    // Other names in the folder: not a SHA-256, a wrong folder, a
    // temporary file.
    let root = dir.0.join("attachments");
    let odd = [
        root.join("zz").join("not-a-sha"),
        root.join("00").join(&none),
        root.join(".tmp-0011"),
    ];
    for p in &odd {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
        fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH)
            .unwrap();
    }

    assert_eq!(s.clean_attachments(NOW).unwrap(), Some(4));
    for (name, sha) in [
        ("revision", &revision),
        ("approved", &approved),
        ("claim", &claim),
        ("draft", &draft),
        ("new", &new),
    ] {
        assert!(exists(&s, sha), "{name} stays");
    }
    for (name, sha) in [
        ("denied", &denied),
        ("sent", &sent),
        ("foreign", &foreign),
        ("none", &none),
    ] {
        assert!(!exists(&s, sha), "{name} goes");
    }
    for p in &odd {
        assert!(p.exists(), "{} stays", p.display());
    }
    // A second run finds nothing more.
    assert_eq!(s.clean_attachments(NOW).unwrap(), Some(0));
}

/// An upload of the same bytes makes an old file new, so it stays.
#[test]
fn an_upload_makes_a_file_new() {
    let dir = TempDir::new("upload");
    let s = Store::open(&dir.0).unwrap();
    let sha = file(&s, b"bytes", OLD);
    assert_eq!(s.put_attachment(b"bytes").unwrap(), sha);
    // The real time of the new file is long before NOW, so NOW is the
    // time of a run soon after the upload.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert_eq!(s.clean_attachments(now).unwrap(), Some(0));
    assert!(exists(&s, &sha));
}

/// A run while a backup holds the lock deletes nothing.
#[test]
fn a_backup_stops_the_cleanup() {
    let dir = TempDir::new("backup");
    let s = Store::open(&dir.0).unwrap();
    let sha = file(&s, b"bytes", OLD);
    let held = s.backup_lock().unwrap();
    assert_eq!(s.clean_attachments(NOW).unwrap(), None);
    drop(held);
    assert!(exists(&s, &sha));
    assert_eq!(s.clean_attachments(NOW).unwrap(), Some(1));
}

/// No attachment folder: nothing to do.
#[test]
fn no_folder_is_no_work() {
    let dir = TempDir::new("empty");
    let s = Store::open(&dir.0).unwrap();
    assert_eq!(s.clean_attachments(NOW).unwrap(), Some(0));
}
