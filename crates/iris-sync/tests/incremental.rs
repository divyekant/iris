//! The incremental sync, the full resync and the failed message, against
//! the fake Gmail server (specification 1, sections 4.3, 4.4 and 4.7, and
//! the test plan of section 10). No network.

use iris_fakegmail::FakeGmail;
use iris_store::Store;
use iris_sync::{sync, Error, KIND_RESYNC, RESYNC_GONE};
use iris_synth::Mailbox;

mod common;
use common::*;

fn synced(name: &str, seed: u64, n: usize) -> (FakeGmail, Store, i64) {
    let fake = FakeGmail::start(Mailbox::new(seed, n)).unwrap();
    let store = Store::open(&data_dir(name)).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();
    sync(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    (fake, store, account)
}

fn latest(fake: &FakeGmail) -> u64 {
    client(fake.base_url()).profile(TOKEN).unwrap().history_id
}

fn gone_at(store: &Store, id: &str) -> Option<i64> {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT gone_at FROM message WHERE provider_id = ?1",
                [id],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

fn log_kinds(store: &Store) -> Vec<String> {
    store
        .read(|c| {
            let mut st = c.prepare("SELECT kind FROM sync_log ORDER BY id")?;
            let rows = st.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap()
}

/// An item with these source messages. `status` `gone` saves `open` as the
/// status before it.
fn add_item(store: &Store, account: i64, sources: &[&str], status: &str) -> i64 {
    store
        .write(|tx| {
            let before = (status == "gone").then_some("open");
            tx.execute(
                "INSERT INTO item(form, account_id, status, status_before_gone, doc, created_at, updated_at)
                 VALUES ('commitment', ?1, ?2, ?3, '{}', 0, 0)",
                rusqlite::params![account, status, before],
            )?;
            let item = tx.last_insert_rowid();
            for s in sources {
                tx.execute(
                    "INSERT INTO item_source(item_id, message_id, role)
                     SELECT ?1, id, 'origin' FROM message WHERE provider_id = ?2",
                    rusqlite::params![item, s],
                )?;
            }
            Ok(item)
        })
        .unwrap()
}

fn item_status(store: &Store, item: i64) -> (String, Option<String>) {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT status, status_before_gone FROM item WHERE id = ?1",
                [item],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

#[test]
fn an_item_is_gone_when_each_source_is_gone() {
    let (fake, store, account) = synced("item-gone", 16, 50);
    let item = add_item(&store, account, &[&FakeGmail::id_of(7)], "open");
    assert!(fake.delete_message(&FakeGmail::id_of(7)));
    sync(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert_eq!(
        item_status(&store, item),
        ("gone".into(), Some("open".into()))
    );
}

#[test]
fn the_first_sync_ends_with_a_ready_mirror() {
    let (fake, store, _) = synced("ready", 3, 150);
    let (kind, start, step, history) = sync_state(&store);
    assert_eq!((kind, start, step), (None, None, None));
    assert_eq!(history, Some(latest(&fake).to_string()));
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM account WHERE mirror_ready_at IS NOT NULL"
        ),
        1
    );
    assert_eq!(mirror_state(&store), fake_state(&fake));
    assert_eq!(log_kinds(&store), vec!["first"]);
}

#[test]
fn new_mail_label_changes_and_deletes_reach_the_mirror() {
    let (fake, store, account) = synced("changes", 5, 200);
    let gmail = client(fake.base_url());
    let new = fake.add_message(
        b"From: a@example.org\r\nSubject: new\r\n\r\nhello\r\n",
        &["INBOX", "UNREAD"],
    );
    assert!(fake.modify_labels(&FakeGmail::id_of(5), &["STARRED", "Label_1"], &["INBOX"]));
    assert!(fake.delete_message(&FakeGmail::id_of(9)));

    let s = sync(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!((s.fetched, s.gone, s.failed, s.resync), (1, 1, 0, false));
    assert_eq!(s.history_id, latest(&fake));
    assert_eq!(sync_state(&store).3, Some(s.history_id.to_string()));
    assert!(gone_at(&store, &FakeGmail::id_of(9)).is_some());
    assert!(gone_at(&store, &new).is_none());
    assert_eq!(mirror_state(&store), fake_state(&fake));

    // A sync with no change fetches nothing.
    let before = raw_requests(&fake);
    let s = sync(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!((s.fetched, s.gone), (0, 0));
    assert_eq!(raw_requests(&fake), before);
}

#[test]
fn a_message_added_and_deleted_before_a_sync() {
    let (fake, store, account) = synced("add-delete", 6, 50);
    let id = fake.add_message(b"Subject: short\r\n\r\nx\r\n", &["INBOX"]);
    assert!(fake.delete_message(&id));

    let s = sync(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert_eq!(s.history_id, latest(&fake));
    assert_eq!(sync_state(&store).3, Some(s.history_id.to_string()));
    let rows = count(
        &store,
        &format!("SELECT count(*) FROM message WHERE provider_id = '{id}'"),
    );
    assert_eq!(rows, 0, "a message that Gmail deleted is not saved");
    assert_eq!(mirror_state(&store), fake_state(&fake));
}

#[test]
fn a_message_added_record_of_a_known_message_sets_its_labels_without_a_fetch() {
    let (fake, store, account) = synced("known-added", 8, 60);
    let victim = FakeGmail::id_of(3);
    store
        .write(|tx| {
            tx.execute(
                "DELETE FROM message_label WHERE message_id =
                   (SELECT id FROM message WHERE provider_id = ?1)",
                [&victim],
            )?;
            // From the first history ID: a messageAdded record for each message.
            tx.execute("UPDATE account SET history_id = '100000'", [])?;
            Ok(())
        })
        .unwrap();
    assert_ne!(mirror_state(&store), fake_state(&fake));

    let before = raw_requests(&fake);
    let s = sync(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert_eq!(s.fetched, 0);
    assert_eq!(raw_requests(&fake), before, "no message is fetched again");
    assert_eq!(mirror_state(&store), fake_state(&fake));
}

#[test]
fn a_label_rename_and_delete_reach_the_label_rows() {
    let (fake, store, account) = synced("labels", 9, 200);
    let with_label_2 = "SELECT count(*) FROM message_label ml JOIN label l ON l.id = ml.label_id
                        WHERE l.provider_id = 'Label_2'";
    assert!(count(&store, with_label_2) > 0);
    assert!(fake.rename_label("Label_1", "Bills"));
    assert!(fake.delete_label("Label_2"));

    sync(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    let name: String = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT name FROM label WHERE provider_id = 'Label_1'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(name, "Bills");
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM label WHERE provider_id = 'Label_2'"
        ),
        0
    );
    assert_eq!(count(&store, with_label_2), 0);
    assert_eq!(mirror_state(&store), fake_state(&fake));
}

#[test]
fn an_expired_history_starts_a_full_resync() {
    let (fake, store, account) = synced("resync", 11, 200);
    let back = FakeGmail::id_of(20);
    let refetch = FakeGmail::id_of(21);
    // A sync defect: two messages that Gmail has are gone in the mirror. One
    // still has its content file, and one does not.
    let sha: String = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT content_sha256 FROM message WHERE provider_id = ?1",
                [&refetch],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    std::fs::remove_file(store.content_path(&sha).unwrap()).unwrap();
    store
        .write(|tx| {
            tx.execute(
                "UPDATE message SET gone_at = 1, understood_at = 2 WHERE provider_id IN (?1, ?2)",
                [&back, &refetch],
            )?;
            Ok(())
        })
        .unwrap();
    let item = add_item(&store, account, &[&back], "gone");
    let other = add_item(
        &store,
        account,
        &[&FakeGmail::id_of(30), &FakeGmail::id_of(32)],
        "open",
    );
    fake.add_message(b"Subject: during\r\n\r\nx\r\n", &["INBOX"]);
    assert!(fake.delete_message(&FakeGmail::id_of(30)));
    assert!(fake.modify_labels(&FakeGmail::id_of(31), &["Label_2"], &[]));
    fake.expire_history_before(latest(&fake));

    let s = sync(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert!(s.resync);
    assert_eq!(s.history_id, latest(&fake));
    assert_eq!(sync_state(&store).0, None);
    assert_eq!(mirror_state(&store), fake_state(&fake));
    for id in [&back, &refetch] {
        let (gone, understood): (Option<i64>, Option<i64>) = store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT gone_at, understood_at FROM message WHERE provider_id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!((gone, understood), (None, None), "{id} comes back");
    }
    assert!(store.content_path(&sha).unwrap().is_file(), "fetched again");
    assert!(gone_at(&store, &FakeGmail::id_of(30)).is_some());
    assert_eq!(
        item_status(&store, item),
        ("open".into(), None),
        "the item comes back"
    );
    assert_eq!(
        item_status(&store, other),
        ("open".into(), None),
        "one source is left"
    );
    assert_eq!(
        log_kinds(&store).last().map(String::as_str),
        Some(KIND_RESYNC)
    );
    assert_eq!(count(&store, "SELECT count(*) FROM sync_seen"), 0);
}

#[test]
fn a_delete_during_the_label_lists_of_a_resync() {
    let (fake, store, account) = synced("resync-delete", 12, 200);
    assert!(fake.modify_labels(&FakeGmail::id_of(1), &["STARRED"], &[]));
    fake.expire_history_before(latest(&fake));
    let victim = FakeGmail::id_of(4);
    let at_fake = fake.clone();
    let v = victim.clone();
    let gmail = client(hook_proxy(fake.addr(), "labelIds=", move || {
        assert!(at_fake.delete_message(&v));
    }));

    let s = sync(&store, &gmail, account, TOKEN).unwrap();
    assert!(s.resync);
    assert!(
        gone_at(&store, &victim).is_some(),
        "gone after the last step"
    );
    assert_eq!(mirror_state(&store), fake_state(&fake));
}

#[test]
fn a_stop_during_the_gone_check_of_a_resync_continues_from_h1() {
    let (fake, store, account) = synced("resync-stop", 14, 200);
    let victim = FakeGmail::id_of(6);
    assert!(fake.delete_message(&victim));
    fake.expire_history_before(latest(&fake));
    let at_fake = fake.clone();
    let cut = client(hook_proxy(fake.addr(), "format=minimal", move || {
        at_fake.fail_next(&[401]);
    }));
    match sync(&store, &cut, account, TOKEN) {
        Err(Error::Gmail(iris_gmail::Error::Unauthorized)) => {}
        other => panic!("the hook did not stop the resync: {other:?}"),
    }
    let (kind, h1, step, _) = sync_state(&store);
    assert_eq!(
        (kind.as_deref(), step),
        (Some(KIND_RESYNC), Some(RESYNC_GONE))
    );
    let h1 = h1.unwrap();

    let from = fake.requests().len();
    let s = sync(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert!(s.resync);
    let after = &fake.requests()[from..];
    let full_list = |r: &&String| r.contains("/messages?") && !r.contains("labelIds=");
    assert!(
        !after.iter().any(|r| full_list(&r)),
        "the list does not run again"
    );
    let first_minimal = after
        .iter()
        .position(|r| r.contains("format=minimal"))
        .unwrap();
    let histories: Vec<(usize, &String)> = after
        .iter()
        .enumerate()
        .filter(|(_, r)| r.contains("/history?"))
        .collect();
    assert!(!histories.is_empty());
    for (i, r) in histories {
        assert!(
            i > first_minimal,
            "no incremental sync before the resync ends"
        );
        assert!(r.contains(&format!("startHistoryId={h1}")), "{r}");
    }
    assert!(gone_at(&store, &victim).is_some());
    assert_eq!(mirror_state(&store), fake_state(&fake));
}

#[test]
fn a_message_that_fails_does_not_stop_the_first_sync() {
    let fake = FakeGmail::start(Mailbox::new(15, 100)).unwrap();
    let store = Store::open(&data_dir("failed")).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();
    let at_fake = fake.clone();
    // Gmail refuses one fetch with HTTP 400: the client does not try again.
    let gmail = client(hook_proxy(fake.addr(), "format=raw", move || {
        at_fake.fail_next(&[400]);
    }));

    let s = sync(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!((s.fetched, s.failed), (99, 1));
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM account WHERE mirror_ready_at IS NOT NULL"
        ),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM message WHERE fetched_at IS NULL"
        ),
        1,
        "the failed message stays known, not fetched"
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM sync_log WHERE kind = 'fetch' AND result LIKE 'failed %'"
        ),
        1
    );
}
