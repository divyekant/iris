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

/// Issue #853, pre-review: the content before a draft change stays live
/// while its draft is not sent, also while the undo is hidden for a time
/// (here, a later change in `writing` that Gmail then rejects). When the
/// draft is sent, the old file goes.
#[test]
fn a_draft_change_keeps_the_file_of_its_undo() {
    let dir = TempDir::new("undo");
    let s = Store::open(&dir.0).unwrap();
    let account = s.add_account("gmail", "owner@example.com").unwrap();
    let before = file(&s, b"before", OLD);
    let after = file(&s, b"after", OLD);
    let entry = |tx: &Connection, draft: i64, pre: &str| -> rusqlite::Result<i64> {
        let a = action(tx, "{}", Some("approved"))?;
        tx.execute(
            "INSERT INTO ledger(action_id, account_id, ops, pre, post, state, undo, at)
             VALUES (?1, ?2, ?3, ?4, '{}', 'writing', 'inverse', 0)",
            params![
                a,
                account,
                serde_json::json!({"call": "drafts.update", "draft": draft}).to_string(),
                pre
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "UPDATE ledger SET state = 'applied', call = 1 WHERE id = ?1",
            [id],
        )?;
        Ok(id)
    };
    let (draft, first) = s
        .write(|tx| {
            let a = action(tx, "{}", Some("denied"))?;
            tx.execute(
                "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                                   action_id, created_at, updated_at)
                 VALUES (?1, 'r', 'iris.r@example.com', ?2, 'h', x'00', ?3, 0, 0)",
                params![account, format!("{{\"content\":{}}}", content(&after)), a],
            )?;
            let draft = tx.last_insert_rowid();
            let pre = format!(
                "{{\"draft\":{{\"content\":{}}},\"date\":null}}",
                content(&before)
            );
            Ok((draft, entry(tx, draft, &pre)?))
        })
        .unwrap();
    let can_undo = |id: i64| -> bool {
        s.read(|c| {
            Ok(c.query_row(
                "SELECT EXISTS (SELECT 1 FROM ledger_can_undo WHERE ledger_id = ?1)",
                [id],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    };
    assert!(can_undo(first));
    assert_eq!(s.clean_attachments(NOW).unwrap(), Some(0));
    assert!(exists(&s, &before), "the undo can read the old file");
    assert!(exists(&s, &after), "the draft names the new file");
    // A later change starts: the first undo is hidden for a time.
    let later = s
        .write(|tx| {
            let a = action(tx, "{}", Some("approved"))?;
            tx.execute(
                "INSERT INTO ledger(action_id, account_id, ops, pre, post, state, undo, at)
                 VALUES (?1, ?2, ?3, '{}', '{}', 'writing', 'inverse', 0)",
                params![
                    a,
                    account,
                    serde_json::json!({"call": "drafts.update", "draft": draft}).to_string()
                ],
            )?;
            Ok(tx.last_insert_rowid())
        })
        .unwrap();
    assert!(!can_undo(first));
    assert_eq!(s.clean_attachments(NOW).unwrap(), Some(0));
    assert!(exists(&s, &before), "a hidden undo keeps its file");
    // Gmail rejects the later change: the first undo is available again.
    s.write(|tx| {
        tx.execute(
            "UPDATE ledger SET state = 'failed', error = 'rejected' WHERE id = ?1",
            [later],
        )?;
        Ok(())
    })
    .unwrap();
    assert!(can_undo(first));
    assert!(exists(&s, &before));
    // The draft is sent: no undo can come back, and the old file goes.
    s.write(|tx| {
        tx.execute(
            "UPDATE draft SET sent_at = 1, sent_message = 'm' WHERE id = ?1",
            [draft],
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(s.clean_attachments(NOW).unwrap(), Some(2));
    assert!(!exists(&s, &before));
}

/// A small deterministic generator of JSON values (a linear congruential
/// generator), for the property test below.
struct Gen(u64);

impl Gen {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }

    fn value(&mut self, depth: u32, sha: &str) -> serde_json::Value {
        use serde_json::json;
        match self.next(if depth == 0 { 6 } else { 9 }) {
            0 => json!(null),
            1 => json!(self.next(2) == 1),
            2 => json!(self.next(1000) as i64 - 500),
            3 => json!("foo"),
            4 => json!(sha),
            5 => json!(1.5),
            6 => (0..self.next(3))
                .map(|_| self.value(depth - 1, sha))
                .collect(),
            7 => json!({"sha256": self.value(depth - 1, sha)}),
            _ => json!({"name": "f", "attachments": self.value(depth - 1, sha)}),
        }
    }
}

/// Property: for any args of an open action, the cleanup does not fail.
/// A file that a valid entry (an object with a text `sha256`) of the
/// attachments array names stays; any other value names no file. The
/// args come from the generator.
#[test]
fn any_draft_args_leave_the_cleanup_working() {
    let dir = TempDir::new("prop");
    let s = Store::open(&dir.0).unwrap();
    let kept = file(&s, b"kept", OLD);
    s.write(|tx| {
        action(tx, &content(&kept), None)?;
        Ok(())
    })
    .unwrap();
    let mut g = Gen(853);
    for case in 0..300 {
        let other = file(&s, format!("other {case}").as_bytes(), OLD);
        let attachments = g.value(3, &other);
        let mut args = serde_json::json!({"attachments": attachments}).to_string();
        match case % 10 {
            // The args are the generated value itself, not an object. (The
            // store refuses args that are not JSON.)
            0 => args = attachments.to_string(),
            // A valid entry next to the generated values.
            1 => {
                let mut list = match attachments {
                    serde_json::Value::Array(a) => a,
                    v => vec![v],
                };
                list.push(serde_json::json!({"sha256": kept}));
                args = serde_json::json!({"attachments": list}).to_string();
            }
            _ => {}
        }
        s.write(|tx| {
            action(tx, &args, None)?;
            Ok(())
        })
        .unwrap();
        let named = serde_json::from_str::<serde_json::Value>(&args)
            .ok()
            .and_then(|v| v["attachments"].as_array().cloned())
            .is_some_and(|a| a.iter().any(|e| e["sha256"].as_str() == Some(&*other)));
        let got = s.clean_attachments(NOW);
        assert!(got.is_ok(), "case {case}: {args}: {got:?}");
        assert_eq!(exists(&s, &other), named, "case {case}: {args}");
        assert!(exists(&s, &kept), "case {case}");
    }
}

/// Issue #853, pre-review: a draft of an account in `removing` or
/// `removed` keeps no file. No new action can name the draft, and the
/// account never signs in again.
#[test]
fn a_removed_account_keeps_no_draft_file() {
    let dir = TempDir::new("removed");
    let s = Store::open(&dir.0).unwrap();
    let mut files = Vec::new();
    for (n, state) in [None, Some("removing"), Some("removed")]
        .into_iter()
        .enumerate()
    {
        let account = s
            .add_account("gmail", &format!("owner{n}@example.com"))
            .unwrap();
        let sha = file(&s, format!("draft {n}").as_bytes(), OLD);
        s.write(|tx| {
            let a = action(tx, "{}", Some("denied"))?;
            tx.execute(
                "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                                   action_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'h', x'00', ?5, 0, 0)",
                params![
                    account,
                    format!("r{n}"),
                    format!("iris.{n}@example.com"),
                    format!("{{\"content\":{}}}", content(&sha)),
                    a
                ],
            )?;
            tx.execute(
                "UPDATE account SET state = ?2 WHERE id = ?1",
                params![account, state],
            )?;
            Ok(())
        })
        .unwrap();
        files.push((state, sha));
    }
    assert_eq!(s.clean_attachments(NOW).unwrap(), Some(2));
    for (state, sha) in &files {
        assert_eq!(exists(&s, sha), state.is_none(), "{state:?}");
    }
}

/// Issue #853, pre-review: an upload waits for the write lock that the
/// cleanup holds from its read of the references to its last delete. So
/// an upload of the same bytes never lands between the age check and the
/// delete. The test holds the lock as the cleanup does, and checks that
/// the upload does not make the file new before the lock is free.
#[test]
fn an_upload_waits_for_the_cleanup_lock() {
    let dir = TempDir::new("overlap");
    let s = std::sync::Arc::new(Store::open(&dir.0).unwrap());
    let sha = file(&s, b"same", OLD);
    let modified = |s: &Store| {
        fs::metadata(s.attachment_path(&sha).unwrap())
            .unwrap()
            .modified()
            .unwrap()
    };
    let old = modified(&s);
    let (held, wait) = std::sync::mpsc::channel();
    let (free, go) = std::sync::mpsc::channel::<()>();
    let holder = {
        let s = s.clone();
        std::thread::spawn(move || {
            s.write(|_| {
                held.send(()).unwrap();
                go.recv().unwrap();
                Ok(())
            })
            .unwrap()
        })
    };
    wait.recv().unwrap();
    let upload = {
        let s = s.clone();
        std::thread::spawn(move || s.put_attachment(b"same").unwrap())
    };
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(modified(&s), old, "the upload waits for the lock");
    assert!(!upload.is_finished());
    free.send(()).unwrap();
    holder.join().unwrap();
    assert_eq!(upload.join().unwrap(), sha);
    assert_ne!(modified(&s), old, "the upload makes the file new");
    // The upload has the real time, not the test time `NOW`.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert_eq!(s.clean_attachments(now).unwrap(), Some(0));
    assert!(exists(&s, &sha));
}

/// Issue #853, pre-review: the same holds for a second Store of the same
/// data folder (another process, for example the `iris` command). Its
/// upload waits for the SQLite write lock that the cleanup holds.
#[test]
fn an_upload_of_another_store_waits_for_the_cleanup_lock() {
    let dir = TempDir::new("overlap2");
    let a = std::sync::Arc::new(Store::open(&dir.0).unwrap());
    let b = Store::open(&dir.0).unwrap();
    let sha = file(&a, b"same", OLD);
    let modified = || {
        fs::metadata(a.attachment_path(&sha).unwrap())
            .unwrap()
            .modified()
            .unwrap()
    };
    let old = modified();
    let (held, wait) = std::sync::mpsc::channel();
    let (free, go) = std::sync::mpsc::channel::<()>();
    // Store `a` holds the SQLite write lock, as its cleanup does.
    let holder = {
        let a = a.clone();
        std::thread::spawn(move || {
            a.write(|tx| {
                tx.execute(
                    "INSERT INTO changeset(author_kind, created_at) VALUES ('person', 0)",
                    [],
                )?;
                held.send(()).unwrap();
                go.recv().unwrap();
                Ok(())
            })
            .unwrap()
        })
    };
    wait.recv().unwrap();
    let upload = std::thread::spawn(move || b.put_attachment(b"same").unwrap());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(modified(), old, "the upload of store b waits for the lock");
    assert!(!upload.is_finished());
    free.send(()).unwrap();
    holder.join().unwrap();
    assert_eq!(upload.join().unwrap(), sha);
    assert_ne!(modified(), old, "the upload makes the file new");
}
