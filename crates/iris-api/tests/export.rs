//! Tests of `iris export` (specification 2, section 8.2).

use std::fs;
use std::path::{Path, PathBuf};

use iris_store::{Header, NewMessage, Store};
use serde_json::Value;

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("iris-export-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    TempDir(dir)
}

fn raw(i: usize) -> Vec<u8> {
    format!("From: a{i}@example.com\r\nSubject: s{i}\r\n\r\nbody {i}\r\n").into_bytes()
}

fn exec(store: &Store, sql: &str, args: impl rusqlite::Params) -> i64 {
    store
        .write(|tx| {
            tx.execute(sql, args)?;
            Ok(tx.last_insert_rowid())
        })
        .unwrap()
}

fn lines(p: &Path) -> Vec<Value> {
    fs::read_to_string(p)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// Every file under `dir`, with its bytes.
fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(files(&p));
        } else {
            let bytes = fs::read(&p).unwrap();
            out.push((p, bytes));
        }
    }
    out
}

#[test]
fn the_export_has_each_item_and_each_message_and_no_secret() {
    let dir = temp("all");
    let dest = temp("all-out");
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    let sealed = store.seal(b"refresh-secret").unwrap();
    store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE account SET token = ?1 WHERE id = ?2",
                rusqlite::params![sealed, acc],
            )?)
        })
        .unwrap();
    store
        .put_labels(acc, &[("INBOX", "INBOX", "system")])
        .unwrap();
    let batch: Vec<NewMessage> = (0..4)
        .map(|i| NewMessage {
            provider_id: format!("g{i}"),
            thread_provider_id: format!("gt{i}"),
            content_sha256: Some(store.put_content(&raw(i)).unwrap()),
            size: Some(raw(i).len() as i64),
            internal_date: Some(1_000 * i as i64),
            fetched_at: Some(1_000),
            labels: vec!["INBOX".into()],
            header: Header {
                subject: Some(format!("s{i}")),
                ..Default::default()
            },
            body: None,
        })
        .collect();
    store.put_messages(acc, &batch).unwrap();
    // Message 4 is gone, and the content cleanup removed its raw message.
    exec(
        &store,
        "UPDATE message SET gone_at = 1, content_sha256 = NULL WHERE id = 4",
        [],
    );
    let sha = batch[3].content_sha256.as_deref().unwrap();
    fs::remove_file(store.content_path(sha).unwrap()).unwrap();
    for (status, doc) in [
        ("open", r#"{"title":"a","status":"wrong"}"#),
        ("gone", "{}"),
    ] {
        exec(
            &store,
            "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
             VALUES ('commitment', ?1, ?2, ?3, 0, 0)",
            rusqlite::params![acc, status, doc],
        );
    }

    let n = iris_api::export(&store, &dest.0).unwrap();
    assert_eq!(
        n,
        iris_api::Exported {
            items: 2,
            messages: 4,
            files: 3
        }
    );

    // The row is the true copy of the ID and the status.
    let items = lines(&dest.0.join("items.jsonl"));
    let got: Vec<_> = items
        .iter()
        .map(|v| (v["id"].clone(), v["status"].clone()))
        .collect();
    assert_eq!(
        got,
        [("c-1".into(), "open".into()), ("c-2".into(), "gone".into())]
    );

    let messages = lines(&dest.0.join("messages.jsonl"));
    assert_eq!(messages.len(), 4);
    for (i, m) in messages.iter().enumerate() {
        assert_eq!(m["id"], format!("m-{}", i + 1));
        assert_eq!(m["subject"], format!("s{i}"));
        if i == 3 {
            assert_eq!(m["gone"], true);
            assert!(m["file"].is_null());
        } else {
            let file = m["file"].as_str().unwrap();
            assert_eq!(fs::read(dest.0.join(file)).unwrap(), raw(i));
        }
    }

    // No file of the export holds the key or a secret, sealed or plain.
    let key = fs::read(dir.0.join("key")).unwrap();
    for (p, bytes) in files(&dest.0) {
        for secret in [&b"refresh-secret"[..], &sealed, &key] {
            assert!(
                !bytes.windows(secret.len()).any(|w| w == secret),
                "{}",
                p.display()
            );
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&dest.0).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    // A second export into the same folder mixes nothing.
    assert!(iris_api::export(&store, &dest.0).is_err());
}

#[test]
fn the_export_waits_for_a_backup() {
    let dir = temp("lock");
    let dest = temp("lock-out");
    let store = std::sync::Arc::new(Store::open(&dir.0).unwrap());
    let lock = store.backup_lock().unwrap();
    let (s, d) = (store.clone(), dest.0.clone());
    let t = std::thread::spawn(move || iris_api::export(&s, &d).unwrap());
    std::thread::sleep(std::time::Duration::from_millis(200));
    // The export holds the lock for its full run, so the content cleanup
    // cannot delete a raw message that it copies.
    assert!(!dest.0.exists());
    drop(lock);
    t.join().unwrap();
    assert!(dest.0.join("items.jsonl").is_file());
}

/// A store with `n` messages in the label INBOX. Gives the content hashes.
fn mailbox(store: &Store, n: usize) -> Vec<String> {
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .put_labels(acc, &[("INBOX", "INBOX", "system")])
        .unwrap();
    let batch: Vec<NewMessage> = (0..n)
        .map(|i| NewMessage {
            provider_id: format!("g{i}"),
            thread_provider_id: format!("gt{i}"),
            content_sha256: Some(store.put_content(&raw(i)).unwrap()),
            labels: vec!["INBOX".into()],
            ..Default::default()
        })
        .collect();
    store.put_messages(acc, &batch).unwrap();
    batch
        .into_iter()
        .map(|m| m.content_sha256.unwrap())
        .collect()
}

#[test]
fn a_missing_raw_message_fails_the_export() {
    let dir = temp("missing");
    let dest = temp("missing-out");
    let store = Store::open(&dir.0).unwrap();
    let shas = mailbox(&store, 3);
    // The row still refers to the file: the export cannot be complete.
    fs::remove_file(store.content_path(&shas[1]).unwrap()).unwrap();
    let e = iris_api::export(&store, &dest.0).unwrap_err();
    assert!(
        matches!(e, iris_api::Error::Store(iris_store::Error::MissingContent(ref s)) if *s == shas[1]),
        "{e}"
    );
    // #245: no file of the stopped export stays, and nothing looks complete.
    assert_eq!(fs::read_dir(&dest.0).unwrap().count(), 0);
    let name = dest.0.file_name().unwrap().to_string_lossy();
    assert!(!dest.0.with_file_name(format!(".{name}.partial")).exists());
    // The same folder takes the next export.
    fs::write(store.content_path(&shas[1]).unwrap(), raw(1)).unwrap();
    assert_eq!(iris_api::export(&store, &dest.0).unwrap().files, 3);
}

/// #819: each message line says if the message is hostile mail, so that a
/// tool that reads the export can tell the items of hostile mail.
#[test]
fn each_message_says_if_it_is_hostile_mail() {
    let dir = temp("hostile");
    let dest = temp("hostile-out");
    let store = Store::open(&dir.0).unwrap();
    mailbox(&store, 2);
    exec(
        &store,
        "UPDATE message SET hostile = '[\"x\"]' WHERE id = 2",
        [],
    );
    iris_api::export(&store, &dest.0).unwrap();
    let got: Vec<_> = lines(&dest.0.join("messages.jsonl"))
        .iter()
        .map(|m| m["hostile_mail"].clone())
        .collect();
    assert_eq!(got, [Value::Bool(false), Value::Bool(true)]);
}

#[test]
fn each_item_that_a_message_names_is_in_the_export_during_writes() {
    let dir = temp("snapshot");
    let dest = temp("snapshot-out");
    let store = std::sync::Arc::new(Store::open(&dir.0).unwrap());
    let n = 300;
    mailbox(&store, n);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (s, halt) = (store.clone(), stop.clone());
    // The engine adds items, each with a source message, during the export.
    let writer = std::thread::spawn(move || {
        let mut i = 0;
        while !halt.load(std::sync::atomic::Ordering::Relaxed) {
            s.write(|tx| {
                tx.execute(
                    "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                     VALUES ('commitment', 1, 'open', '{}', 0, 0)",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
                    [tx.last_insert_rowid(), (i % n as i64) + 1],
                )?;
                Ok(())
            })
            .unwrap();
            i += 1;
        }
        i
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    iris_api::export(&store, &dest.0).unwrap();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(writer.join().unwrap() > 0);

    let items: std::collections::HashSet<String> = lines(&dest.0.join("items.jsonl"))
        .iter()
        .map(|v| v["id"].as_str().unwrap().to_string())
        .collect();
    for m in lines(&dest.0.join("messages.jsonl")) {
        for id in m["items"].as_array().unwrap() {
            assert!(
                items.contains(id.as_str().unwrap()),
                "{} names {id}",
                m["id"]
            );
        }
    }
}
