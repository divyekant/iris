//! Tests of the removal of an account and the delete of its mail
//! (docs/specs/2026-11-24-remove-account.md).

use std::fs;
use std::path::PathBuf;

use iris_store::{Body, Deleted, FileLock, NewMessage, Store, STATE_REMOVED, STATE_REMOVING};
use rusqlite::Connection;

const NOW: i64 = 1_800_000_000_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-remove-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn msg(store: &Store, id: &str, raw: &[u8], word: &str) -> NewMessage {
    NewMessage {
        provider_id: id.into(),
        thread_provider_id: format!("t-{id}"),
        content_sha256: Some(store.put_content(raw).unwrap()),
        labels: vec!["INBOX".into()],
        header: iris_store::Header {
            from_addr: Some(format!("{word}@example.com")),
            subject: Some(word.into()),
            ..Default::default()
        },
        body: Some(Body {
            subject: word.into(),
            sender: format!("{word}@example.com"),
            text: format!("the word {word}"),
        }),
        ..Default::default()
    }
}

/// Two accounts with mail, an item each, a sender each, and one raw file
/// that both accounts use. Gives (a, b).
fn two(store: &Store) -> (i64, i64) {
    let a = store.add_account("gmail", "a@example.org").unwrap();
    let b = store.add_account("gmail", "b@example.org").unwrap();
    for acc in [a, b] {
        store
            .put_labels(acc, &[("INBOX", "INBOX", "system")])
            .unwrap();
    }
    store
        .put_messages(
            a,
            &[
                msg(store, "1", b"only a", "apple"),
                msg(store, "2", b"shared", "pear"),
            ],
        )
        .unwrap();
    store
        .put_messages(b, &[msg(store, "1", b"shared", "plum")])
        .unwrap();
    store
        .write(|tx| {
            for addr in ["apple@example.com", "plum@example.com"] {
                tx.execute(
                    "INSERT INTO sender(address, kind, basis, decided_by, decided_at)
                     VALUES (?1, 'stranger', 'test', 'engine', 0)",
                    [addr],
                )?;
            }
            tx.execute(
                "INSERT INTO sender(address, kind, basis, decided_by, decided_at)
                 VALUES ('pear@example.com', 'hostile', 'test', 'person', 0)",
                [],
            )?;
            for acc in [a, b] {
                tx.execute(
                    "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                     VALUES ('record', ?1, 'open', '{}', 0, 0)",
                    [acc],
                )?;
                tx.execute(
                    "INSERT INTO item_source(item_id, message_id, role)
                     SELECT last_insert_rowid(), min(id), 'origin' FROM message WHERE account_id = ?1",
                    [acc],
                )?;
            }
            Ok(())
        })
        .unwrap();
    (a, b)
}

fn n(store: &Store, sql: &str, acc: i64) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [acc], |r| r.get(0))?))
        .unwrap()
}

fn state(store: &Store, acc: i64) -> Option<String> {
    store
        .read(|c| {
            Ok(
                c.query_row("SELECT state FROM account WHERE id = ?1", [acc], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
}

/// The rows of `acc` in each table with mail of an account.
fn rows(store: &Store, acc: i64) -> i64 {
    n(
        store,
        "SELECT (SELECT count(*) FROM message WHERE account_id = ?1)
              + (SELECT count(*) FROM thread WHERE account_id = ?1)
              + (SELECT count(*) FROM label WHERE account_id = ?1)
              + (SELECT count(*) FROM item WHERE account_id = ?1)
              + (SELECT count(*) FROM sync_log WHERE account_id = ?1)",
        acc,
    )
}

fn none(_: i64) -> std::io::Result<()> {
    Ok(())
}

#[test]
fn the_delete_removes_the_mail_of_one_account_and_keeps_the_other() {
    let dir = TempDir::new("one");
    let store = Store::open(&dir.0).unwrap();
    let (a, b) = two(&store);
    let only_a = store.put_content(b"only a").unwrap();
    let shared = store.put_content(b"shared").unwrap();
    let b_before = rows(&store, b);
    // A sender with no header of the removed account is not a candidate.
    store
        .write(|tx| {
            Ok(tx.execute(
                "INSERT INTO sender(address, kind, basis, decided_by, decided_at)
                 VALUES ('kiwi@example.com', 'stranger', 'test', 'engine', 0)",
                [],
            )?)
        })
        .unwrap();

    store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE account SET imap_host = 'imap.example.org', imap_port = 993,
                   imap_user = 'a-user', categories_off_at = 1, history_id = 'h', token = x'00'
                 WHERE id = ?1",
                [a],
            )?)
        })
        .unwrap();
    assert_eq!(store.remove_account("a@example.org", NOW).unwrap(), a);
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVING));
    // The brief has no item of the account at once.
    assert_eq!(
        n(&store, "SELECT count(*) FROM item WHERE account_id = ?1", a),
        0
    );

    let done = store
        .delete_removed(None, &mut none, &mut |_| Ok(()))
        .unwrap()
        .unwrap();
    assert_eq!(
        done,
        Deleted {
            accounts: 1,
            messages: 2,
            files: 1
        }
    );
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVED));
    assert_eq!(rows(&store, a), 0);
    // The tombstone keeps only its ID, provider, address and state.
    let kept: Vec<String> = store
        .read(|c| {
            let cols: Vec<String> = c
                .prepare("SELECT name FROM pragma_table_info('account')")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let mut kept = Vec::new();
            for col in cols {
                let set: bool = c.query_row(
                    &format!("SELECT \"{col}\" IS NOT NULL FROM account WHERE id = ?1"),
                    [a],
                    |r| r.get(0),
                )?;
                if set {
                    kept.push(col);
                }
            }
            Ok(kept)
        })
        .unwrap();
    assert_eq!(kept, ["id", "provider", "address", "state"]);
    // The search text of the account is gone; the other account keeps its own.
    assert_eq!(store.search("apple", 10).unwrap(), Vec::<i64>::new());
    assert_eq!(store.search("pear", 10).unwrap(), Vec::<i64>::new());
    assert_eq!(store.search("plum", 10).unwrap().len(), 1);
    assert!(!store.content_path(&only_a).unwrap().exists());
    assert!(store.content_path(&shared).unwrap().exists());
    assert_eq!(rows(&store, b), b_before);
    // A sender with no mail left goes, unless the person decided it.
    let senders: Vec<String> = store
        .read(|c| {
            let mut q = c.prepare("SELECT address FROM sender ORDER BY address")?;
            let v = q
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(v)
        })
        .unwrap();
    assert_eq!(
        senders,
        ["kiwi@example.com", "pear@example.com", "plum@example.com"]
    );
    assert_eq!(
        n(
            &store,
            "SELECT count(*) FROM removed_sender WHERE account_id = ?1",
            a
        ),
        0
    );
    // A second call has nothing to do.
    assert_eq!(
        store
            .delete_removed(None, &mut none, &mut |_| Ok(()))
            .unwrap()
            .unwrap(),
        Deleted::default()
    );
}

#[test]
fn a_removed_account_gets_no_new_mail() {
    let dir = TempDir::new("insert");
    let store = Store::open(&dir.0).unwrap();
    let (a, _) = two(&store);
    store.remove_account("a@example.org", NOW).unwrap();
    // A sync that started before the removal fails at its next write.
    assert!(store
        .put_messages(a, &[msg(&store, "3", b"late", "late")])
        .is_err());
    assert!(store.put_labels(a, &[("X", "X", "user")]).is_err());
    store
        .delete_removed(None, &mut none, &mut |_| Ok(()))
        .unwrap();
    assert!(store
        .put_messages(a, &[msg(&store, "3", b"late", "late")])
        .is_err());
    assert_eq!(rows(&store, a), 0);
}

#[test]
fn the_removal_refuses_an_unknown_or_removed_account() {
    let dir = TempDir::new("refuse");
    let store = Store::open(&dir.0).unwrap();
    two(&store);
    assert!(store.remove_account("x@example.org", NOW).is_err());
    store.remove_account("a@example.org", NOW).unwrap();
    assert!(store.remove_account("a@example.org", NOW).is_err());
    store
        .delete_removed(None, &mut none, &mut |_| Ok(()))
        .unwrap();
    assert!(store.remove_account("a@example.org", NOW).is_err());
}

/// The removal by ID removes only the account that has the ID and the
/// address: an account that a person added again with the same address,
/// after a removal of the old ID, stays.
#[test]
fn a_removal_by_a_stale_id_does_not_remove_the_account_added_again() {
    let dir = TempDir::new("stale");
    let store = Store::open(&dir.0).unwrap();
    let (a, _) = two(&store);
    store.remove_account_id(a, "a@example.org", NOW).unwrap();
    store
        .delete_removed(Some(a), &mut none, &mut |_| Ok(()))
        .unwrap();
    let again = store
        .save_account_secret("gmail", "a@example.org", None, b"sealed", "ok")
        .unwrap();
    assert_ne!(again, a);
    assert!(store.remove_account_id(a, "a@example.org", NOW).is_err());
    assert_eq!(state(&store, again).as_deref(), Some("ok"));
    assert_eq!(
        store
            .remove_account_id(again, "a@example.org", NOW)
            .unwrap(),
        again
    );
}

#[test]
fn the_delete_waits_for_the_sync_lock_and_continues_after_a_restart() {
    let dir = TempDir::new("lock");
    let (a, b) = {
        let store = Store::open(&dir.0).unwrap();
        let ids = two(&store);
        store.remove_account("a@example.org", NOW).unwrap();
        ids
    };
    // A crash after the removal: a new start finds the state removing.
    let store = Store::open(&dir.0).unwrap();
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVING));
    let held = FileLock::try_take(&store.sync_lock_path(a))
        .unwrap()
        .unwrap();
    assert_eq!(
        store
            .delete_removed(None, &mut none, &mut |_| Ok(()))
            .unwrap()
            .unwrap(),
        Deleted::default()
    );
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVING));
    drop(held);
    // A held backup lock stops the delete too.
    let backup = store.backup_lock().unwrap();
    assert_eq!(
        store
            .delete_removed(None, &mut none, &mut |_| Ok(()))
            .unwrap(),
        None
    );
    drop(backup);
    store
        .delete_removed(None, &mut none, &mut |_| Ok(()))
        .unwrap()
        .unwrap();
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVED));
    assert_eq!(rows(&store, a), 0);
    assert!(rows(&store, b) > 0);
}

#[test]
fn the_address_comes_back_only_after_the_delete() {
    let dir = TempDir::new("again");
    let store = Store::open(&dir.0).unwrap();
    let (a, _) = two(&store);
    store.remove_account("a@example.org", NOW).unwrap();
    assert!(store
        .save_account_secret("gmail", "a@example.org", None, b"sealed", "ok")
        .is_err());
    store
        .delete_removed(None, &mut none, &mut |_| Ok(()))
        .unwrap();
    // The address comes back as a new account, also of another provider.
    let id = store
        .save_account_secret("outlook", "a@example.org", None, b"sealed", "ok")
        .unwrap();
    assert_ne!(id, a);
    assert_eq!(state(&store, id).as_deref(), Some("ok"));
    store
        .put_labels(id, &[("INBOX", "INBOX", "system")])
        .unwrap();
    store
        .put_messages(id, &[msg(&store, "9", b"new", "new")])
        .unwrap();
    assert_eq!(
        n(
            &store,
            "SELECT count(*) FROM message WHERE account_id = ?1",
            id
        ),
        1
    );
    // The tombstone keeps its ID and mailbox, and never signs in again.
    let (address, provider): (String, String) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT address, provider FROM account WHERE id = ?1",
                [a],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        (address.as_str(), provider.as_str()),
        (format!("removed:{a}:a@example.org").as_str(), "gmail")
    );
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVED));
    assert!(store
        .put_messages(a, &[msg(&store, "8", b"old", "old")])
        .is_err());
}

#[test]
fn the_delete_removes_the_workspace_of_each_job_with_mail_of_the_account() {
    let dir = TempDir::new("jobs");
    let store = Store::open(&dir.0).unwrap();
    let (a, b) = two(&store);
    store
        .write(|tx| {
            for (job, acc) in [(1, a), (2, b), (3, a)] {
                tx.execute(
                    "INSERT INTO job(id, goal, created_at, state, updated_at)
                     VALUES (?1, 'g', 0, 'working', 0)",
                    [job],
                )?;
                // Job 3 has an item of the account, which the removal deletes.
                let sql = if job == 3 {
                    "INSERT INTO job_scope(job_id, item_id, by, at)
                     SELECT ?1, min(id), 'person', 0 FROM item WHERE account_id = ?2"
                } else {
                    "INSERT INTO job_scope(job_id, thread_id, by, at)
                     SELECT ?1, min(id), 'person', 0 FROM thread WHERE account_id = ?2"
                };
                tx.execute(sql, [job, acc])?;
            }
            // Job 5 had an item and a listed message that are gone now (an
            // item that changed, a message that clean_gone deleted). Its
            // workspace can still hold their mail, of any account.
            tx.execute_batch(&format!(
                "INSERT INTO job(id, goal, created_at, state, updated_at) VALUES (5, 'g', 0, 'working', 0);
                 INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                   VALUES ('record', {b}, 'open', '{{}}', 0, 0);
                 INSERT INTO job_scope(job_id, item_id, by, at)
                   VALUES (5, last_insert_rowid(), 'person', 0);
                 DELETE FROM item WHERE id = (SELECT item_id FROM job_scope WHERE job_id = 5);"
            ))?;
            Ok(())
        })
        .unwrap();
    // A running session of the job stops the removal.
    store
        .write(|tx| {
            Ok(tx.execute(
                "INSERT INTO job_session(job_id, agent, registered_at) VALUES (1, 'x', 0)",
                [],
            )?)
        })
        .unwrap();
    assert!(store.remove_account("a@example.org", NOW).is_err());
    // An ended session whose process is not reaped yet can still write to
    // the workspace: it stops the removal too.
    store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE job_session SET ended_at = 1, end_reason = 'stop'",
                [],
            )?)
        })
        .unwrap();
    assert!(store.remove_account("a@example.org", NOW).is_err());
    store
        .write(|tx| Ok(tx.execute("UPDATE job_session SET reaped_at = 2", [])?))
        .unwrap();
    store.remove_account("a@example.org", NOW).unwrap();
    // A crash here: the items and their links are gone, the list stays.
    drop(store);
    let store = Store::open(&dir.0).unwrap();
    // Job 4 stands for a job whose session started after the removal: its
    // build already left the account out, so the delete skips it.
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO job(id, goal, created_at, state, updated_at) VALUES (4, 'g', 0, 'working', 0);
                 INSERT INTO job_session(job_id, agent, registered_at) VALUES (4, 'x', 0);",
            )?;
            tx.execute("INSERT INTO removed_workspace(account_id, job_id) VALUES (?1, 4)", [a])?;
            Ok(())
        })
        .unwrap();
    let mut gone = Vec::new();
    store
        .delete_removed(
            None,
            &mut |job| {
                gone.push(job);
                Ok(())
            },
            &mut |_| Ok(()),
        )
        .unwrap();
    gone.sort();
    assert_eq!(gone, [1, 3, 5]);
    // Job 4 registered its session after the removal, but its launch did
    // not build yet: its row stays, so the account stays removing, and no
    // later call ends the delete before that purge.
    let rows = |store: &Store| {
        n(
            store,
            "SELECT count(*) FROM removed_workspace WHERE account_id = ?1",
            a,
        )
    };
    assert_eq!(rows(&store), 1);
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVING));
    store
        .delete_removed(None, &mut none, &mut |_| Ok(()))
        .unwrap();
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVING));
    // The build of the launch purges the folder and deletes the row
    // (iris-workspace); the next call ends the delete.
    store
        .write(|tx| Ok(tx.execute("DELETE FROM removed_workspace WHERE job_id = 4", [])?))
        .unwrap();
    store
        .delete_removed(None, &mut none, &mut |_| Ok(()))
        .unwrap();
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVED));
    assert_eq!(rows(&store), 0);
}

/// A removal that waits for the sync lock of its own account does not hold
/// up the removal of another account.
#[test]
fn a_wait_for_one_account_does_not_hold_up_another() {
    let dir = TempDir::new("two-waits");
    let store = std::sync::Arc::new(Store::open(&dir.0).unwrap());
    let (a, b) = two(&store);
    store.remove_account("a@example.org", NOW).unwrap();
    store.remove_account("b@example.org", NOW).unwrap();
    let held = FileLock::try_take(&store.sync_lock_path(a))
        .unwrap()
        .unwrap();
    let s2 = store.clone();
    let waiter = std::thread::spawn(move || {
        s2.delete_removed(Some(a), &mut none, &mut |_| Ok(()))
            .unwrap()
            .unwrap()
    });
    std::thread::sleep(std::time::Duration::from_millis(200));
    let (send, done) = std::sync::mpsc::channel();
    let s3 = store.clone();
    std::thread::spawn(move || {
        send.send(
            s3.delete_removed(Some(b), &mut none, &mut |_| Ok(()))
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    });
    let got = done
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("b waits for a");
    assert_eq!(got.accounts, 1);
    assert_eq!(state(&store, b).as_deref(), Some(STATE_REMOVED));
    drop(held);
    assert_eq!(waiter.join().unwrap().accounts, 1);
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVED));
}

/// Each table with a column that names a message, a thread, an item or an
/// account is in one of the two lists of the spec: the delete handles it,
/// or it is a part of the record of actions that stays. A new table fails
/// here until the spec and the delete name it.
#[test]
fn each_table_with_mail_of_an_account_is_in_the_delete_or_the_record() {
    const DELETE: &[&str] = &[
        "account",
        "attention_due",
        "body",
        "correction",
        "header",
        "item",
        "item_source",
        "label",
        "link_check",
        "model_declined",
        "fixed_due",
        "matter_add",
        "matter_item",
        "message",
        "message_label",
        "notice",
        "nudge_try",
        "reply_draft",
        "sender_evidence",
        "sync_log",
        "sync_seen",
        "talk_due",
        "thread",
        "use_open",
        "removed_workspace",
        "removed_sender",
    ];
    const RECORD: &[&str] = &[
        "action_draft",
        "action_item",
        "action_message",
        "draft",
        "job_scope",
        "job_scope_message",
        "ledger",
        "ledger_message",
        "watch",
    ];
    let dir = TempDir::new("tables");
    drop(Store::open(&dir.0).unwrap());
    let c = Connection::open(dir.0.join("iris.db")).unwrap();
    let mut q = c
        .prepare(
            "SELECT DISTINCT m.name FROM sqlite_master m, pragma_table_info(m.name) p
             WHERE m.type = 'table'
               AND p.name IN ('message_id', 'thread_id', 'item_id', 'account_id', 'based_on')",
        )
        .unwrap();
    let tables: Vec<String> = q
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for t in tables {
        assert!(
            DELETE.contains(&t.as_str()) || RECORD.contains(&t.as_str()),
            "table {t} has mail of an account: add it to the delete or to the record (spec)"
        );
    }
}

#[test]
fn the_delete_removes_a_raw_file_whose_row_insert_failed() {
    let dir = TempDir::new("orphan");
    let store = Store::open(&dir.0).unwrap();
    let (a, _) = two(&store);
    store.remove_account("a@example.org", NOW).unwrap();
    // A sync wrote the file, then the trigger refused its row.
    let late = msg(&store, "3", b"late mail", "late");
    let sha = late.content_sha256.clone().unwrap();
    assert!(store.put_messages(a, &[late]).is_err());
    assert!(store.content_path(&sha).unwrap().exists());
    std::thread::sleep(std::time::Duration::from_millis(20));
    store
        .delete_removed(None, &mut none, &mut |_| Ok(()))
        .unwrap();
    assert!(!store.content_path(&sha).unwrap().exists());
    // The files of the other account stay.
    assert!(store
        .content_path(&store.put_content(b"shared").unwrap())
        .unwrap()
        .exists());
}

#[test]
fn understanding_cannot_write_for_a_removed_account() {
    let dir = TempDir::new("understand");
    let store = Store::open(&dir.0).unwrap();
    let (a, _) = two(&store);
    let m: i64 = n(
        &store,
        "SELECT min(id) FROM message WHERE account_id = ?1",
        a,
    );
    store.remove_account("a@example.org", NOW).unwrap();
    let body = Body {
        subject: "s".into(),
        sender: "x".into(),
        text: "t".into(),
    };
    store
        .write(|tx| Ok(tx.execute("DELETE FROM body WHERE message_id = ?1", [m])?))
        .unwrap();
    assert!(store.put_body(m, &body).is_err());
    let item = store.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
             VALUES ('record', ?1, 'open', '{}', 0, 0)",
            [a],
        )?)
    });
    assert!(item.is_err());
}

#[test]
fn a_busy_checkpoint_keeps_the_state_until_a_later_call() {
    let dir = TempDir::new("busy");
    let store = Store::open(&dir.0).unwrap();
    let (a, _) = two(&store);
    store.remove_account("a@example.org", NOW).unwrap();
    // A reader with an open snapshot keeps the log busy.
    let reader = Connection::open(dir.0.join("iris.db")).unwrap();
    reader
        .execute_batch("BEGIN; SELECT count(*) FROM message;")
        .unwrap();
    store
        .delete_removed(None, &mut none, &mut |_| Ok(()))
        .unwrap();
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVING));
    assert_eq!(rows(&store, a), 0);
    reader.execute_batch("COMMIT").unwrap();
    drop(reader);
    store
        .delete_removed(None, &mut none, &mut |_| Ok(()))
        .unwrap();
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVED));
}

#[test]
fn a_waiting_delete_ends_after_the_sync_lock_goes() {
    let dir = TempDir::new("wait");
    let store = std::sync::Arc::new(Store::open(&dir.0).unwrap());
    let (a, _) = two(&store);
    store.remove_account("a@example.org", NOW).unwrap();
    let held = FileLock::try_take(&store.sync_lock_path(a))
        .unwrap()
        .unwrap();
    let (send, done) = std::sync::mpsc::channel();
    let s2 = store.clone();
    let worker = std::thread::spawn(move || {
        send.send(
            s2.delete_removed(Some(a), &mut none, &mut |_| Ok(()))
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    });
    assert!(done
        .recv_timeout(std::time::Duration::from_millis(300))
        .is_err());
    drop(held);
    assert_eq!(done.recv().unwrap().accounts, 1);
    worker.join().unwrap();
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVED));
}

#[test]
fn a_waiting_delete_tries_a_busy_checkpoint_again() {
    let dir = TempDir::new("busy-wait");
    let store = Store::open(&dir.0).unwrap();
    let (a, _) = two(&store);
    store.remove_account("a@example.org", NOW).unwrap();
    let path = dir.0.join("iris.db");
    let reader = std::thread::spawn(move || {
        let c = Connection::open(path).unwrap();
        c.execute_batch("BEGIN; SELECT count(*) FROM message;")
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(700));
        c.execute_batch("COMMIT").unwrap();
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    store
        .delete_removed(Some(a), &mut none, &mut |_| Ok(()))
        .unwrap();
    assert_eq!(state(&store, a).as_deref(), Some(STATE_REMOVED));
    reader.join().unwrap();
}

#[test]
fn a_proposal_with_a_member_of_the_account_goes_and_the_other_items_stay() {
    let dir = TempDir::new("matter");
    let store = Store::open(&dir.0).unwrap();
    let (a, b) = two(&store);
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO matter(title, status, model, prompt, created_at, updated_at)
                 VALUES ('Trip', 'proposed', 'm', 'p', 0, 0)",
                [],
            )?;
            tx.execute(
                "INSERT INTO matter_item(matter_id, item_id)
                 SELECT last_insert_rowid(), id FROM item",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    store.remove_account("a@example.org", NOW).unwrap();
    assert_eq!(n(&store, "SELECT count(*) FROM matter WHERE ?1 > 0", 1), 0);
    assert_eq!(
        n(&store, "SELECT count(*) FROM item WHERE account_id = ?1", b),
        1
    );
    assert_eq!(
        n(&store, "SELECT count(*) FROM item WHERE account_id = ?1", a),
        0
    );
}

/// The removal and the delete write zeros over each deleted row: no page of
/// the database file keeps the text of an item of the account (spec, step 6
/// and "The delete"; issue #882).
#[test]
fn no_page_of_the_database_keeps_the_text_of_a_removed_item() {
    let dir = TempDir::new("secure");
    let store = Store::open(&dir.0).unwrap();
    let (a, _) = two(&store);
    let marker = "SECRET-ITEM-TEXT-7f3a";
    store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE item SET doc = json_object('note', ?2) WHERE account_id = ?1",
                rusqlite::params![a, marker],
            )?)
        })
        .unwrap();
    let c = Connection::open(dir.0.join("iris.db")).unwrap();
    c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .unwrap();
    drop(c);
    let has = |dir: &PathBuf| {
        ["iris.db", "iris.db-wal"].iter().any(|f| {
            fs::read(dir.join(f))
                .map(|b| b.windows(marker.len()).any(|w| w == marker.as_bytes()))
                .unwrap_or(false)
        })
    };
    assert!(has(&dir.0), "the marker is in the file before the removal");
    store.remove_account("a@example.org", NOW).unwrap();
    store
        .delete_removed(Some(a), &mut none, &mut |_| Ok(()))
        .unwrap()
        .unwrap();
    assert!(!has(&dir.0), "a page keeps the text of a removed item");
}
