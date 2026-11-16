//! Verify (specification 1, section 4.8) against the fake Gmail server,
//! with the lines of the test plan (section 10) that name verify. No network.

use iris_fakegmail::FakeGmail;
use iris_store::Store;
use iris_sync::{sync, verify, Difference, KIND_RESYNC};
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

fn sha_of(store: &Store, id: &str) -> Option<String> {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT content_sha256 FROM message WHERE provider_id = ?1",
                [id],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

#[test]
fn a_mirror_in_sync_has_no_difference() {
    let (fake, store, account) = synced("v-clean", 21, 200);
    let r = verify(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert_eq!(r.differences, vec![]);
    assert_eq!(sync_state(&store).0, None, "no resync");
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM sync_log WHERE kind = 'verify' AND result LIKE '0 differences%'"
        ),
        1
    );
}

#[test]
fn label_changes_between_two_syncs_give_no_difference() {
    let (fake, store, account) = synced("v-labels", 22, 200);
    let gmail = client(fake.base_url());
    assert!(fake.modify_labels(&FakeGmail::id_of(3), &["STARRED"], &["INBOX"]));
    sync(&store, &gmail, account, TOKEN).unwrap();
    assert!(fake.modify_labels(&FakeGmail::id_of(4), &["Label_1"], &[]));
    let r = verify(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(r.differences, vec![]);
}

#[test]
fn a_label_rename_and_delete_give_no_difference() {
    let (fake, store, account) = synced("v-rename", 23, 200);
    assert!(fake.rename_label("Label_1", "Bills"));
    assert!(fake.delete_label("Label_2"));
    let gmail = client(fake.base_url());
    sync(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(
        verify(&store, &gmail, account, TOKEN).unwrap().differences,
        vec![]
    );
}

#[test]
fn a_label_change_while_verify_lists_the_labels_gives_no_difference() {
    let (fake, store, account) = synced("v-during", 24, 200);
    let at_fake = fake.clone();
    let gmail = client(hook_proxy(fake.addr(), "labelIds=", move || {
        assert!(at_fake.modify_labels(&FakeGmail::id_of(5), &["STARRED", "Label_2"], &["INBOX"]));
        at_fake.rename_label("Label_1", "Bills");
    }));
    let r = verify(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(r.differences, vec![]);
    assert_eq!(r.skipped, 1);
}

#[test]
fn one_changed_byte_is_one_difference_and_a_new_fetch() {
    let (fake, store, account) = synced("v-byte", 25, 100);
    let id = FakeGmail::id_of(7);
    let path = store.content_path(&sha_of(&store, &id).unwrap()).unwrap();
    let good = std::fs::read(&path).unwrap();
    let mut bad = good.clone();
    bad[10] ^= 1;
    std::fs::write(&path, &bad).unwrap();

    let r = verify(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert_eq!(r.differences, vec![Difference::Content(id.clone())]);
    assert_eq!(std::fs::read(&path).unwrap(), good, "repaired");
    assert_eq!(
        sync_state(&store).0,
        None,
        "a local defect starts no resync"
    );
}

#[test]
fn a_failed_message_is_fetched_by_verify() {
    let fake = FakeGmail::start(Mailbox::new(26, 50)).unwrap();
    let store = Store::open(&data_dir("v-failed")).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();
    let at_fake = fake.clone();
    let cut = client(hook_proxy(fake.addr(), "format=raw", move || {
        at_fake.fail_next(&[400])
    }));
    assert_eq!(sync(&store, &cut, account, TOKEN).unwrap().failed, 1);

    let r = verify(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert_eq!(r.differences.len(), 1);
    assert!(matches!(r.differences[0], Difference::Content(_)));
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM message WHERE fetched_at IS NULL"
        ),
        0
    );
    assert_eq!(mirror_state(&store), fake_state(&fake));
}

#[test]
fn a_sync_defect_is_reported_and_starts_a_full_resync() {
    let (fake, store, account) = synced("v-defect", 27, 200);
    let gmail = client(fake.base_url());
    store
        .write(|tx| {
            tx.execute(
                "DELETE FROM message_label WHERE message_id =
                   (SELECT id FROM message WHERE provider_id = ?1)",
                [FakeGmail::id_of(8)],
            )?;
            tx.execute(
                "UPDATE message SET gone_at = 1 WHERE provider_id = ?1",
                [FakeGmail::id_of(9)],
            )?;
            Ok(())
        })
        .unwrap();
    let r = verify(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(
        r.differences,
        vec![
            Difference::MissingMessage(FakeGmail::id_of(9)),
            Difference::Labels(FakeGmail::id_of(8)),
        ]
    );
    assert_eq!(sync_state(&store).0.as_deref(), Some(KIND_RESYNC));

    assert!(sync(&store, &gmail, account, TOKEN).unwrap().resync);
    assert_eq!(
        verify(&store, &gmail, account, TOKEN).unwrap().differences,
        vec![]
    );
}

#[test]
fn a_repair_fetch_that_fails_is_reported_as_not_repaired() {
    let (fake, store, account) = synced("v-unrepaired", 28, 50);
    let id = FakeGmail::id_of(3);
    let path = store.content_path(&sha_of(&store, &id).unwrap()).unwrap();
    std::fs::write(&path, b"bad").unwrap();
    let at_fake = fake.clone();
    let gmail = client(hook_proxy(fake.addr(), "format=raw", move || {
        at_fake.fail_next(&[400])
    }));

    let r = verify(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(r.differences, vec![Difference::Unrepaired(id)]);
    assert_eq!(std::fs::read(&path).unwrap(), b"bad");
}

#[test]
fn a_repair_that_stops_keeps_each_difference_and_the_resync() {
    let (fake, store, account) = synced("v-repair-stop", 29, 50);
    let bad = FakeGmail::id_of(3);
    let path = store.content_path(&sha_of(&store, &bad).unwrap()).unwrap();
    std::fs::write(&path, b"bad").unwrap();
    store
        .write(|tx| {
            tx.execute(
                "DELETE FROM message_label WHERE message_id =
                   (SELECT id FROM message WHERE provider_id = ?1)",
                [FakeGmail::id_of(8)],
            )?;
            Ok(())
        })
        .unwrap();
    let at_fake = fake.clone();
    // The repair fetch gets HTTP 401: an error that stops each fetch.
    let gmail = client(hook_proxy(fake.addr(), "format=raw", move || {
        at_fake.fail_next(&[401])
    }));

    let r = verify(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(
        r.differences,
        vec![
            Difference::Labels(FakeGmail::id_of(8)),
            Difference::Unrepaired(bad)
        ]
    );
    assert_eq!(sync_state(&store).0.as_deref(), Some(KIND_RESYNC));
}

#[test]
fn a_delete_between_two_list_pages_gives_no_difference() {
    // More than one list page (500 messages for each page).
    let (fake, store, account) = synced("v-page", 30, 700);
    let at_fake = fake.clone();
    // Message 690 is on the first page (newest first).
    let gmail = client(hook_proxy(fake.addr(), "pageToken=", move || {
        assert!(at_fake.delete_message(&FakeGmail::id_of(690)));
    }));
    let r = verify(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(r.differences, vec![]);
    assert!(!r.resync);
}

#[test]
fn a_message_deleted_during_verify_is_not_a_content_difference() {
    let (fake, store, account) = synced("v-content-gone", 31, 50);
    let id = FakeGmail::id_of(4);
    let path = store.content_path(&sha_of(&store, &id).unwrap()).unwrap();
    std::fs::write(&path, b"bad").unwrap();
    let (at_fake, victim) = (fake.clone(), id.clone());
    let gmail = client(hook_proxy(fake.addr(), "labelIds=", move || {
        assert!(at_fake.delete_message(&victim));
    }));
    let r = verify(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(r.differences, vec![]);
}

#[test]
fn a_shared_content_file_is_repaired_by_the_other_message() {
    let fake = FakeGmail::start(Mailbox::new(32, 10)).unwrap();
    let raw = b"Subject: same\r\n\r\nsame bytes\r\n";
    let first = fake.add_message(raw, &["INBOX"]);
    let second = fake.add_message(raw, &["INBOX"]);
    let store = Store::open(&data_dir("v-shared")).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();
    sync(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    let sha = sha_of(&store, &first).unwrap();
    assert_eq!(sha_of(&store, &second), Some(sha.clone()));
    let path = store.content_path(&sha).unwrap();
    std::fs::write(&path, b"bad").unwrap();
    let at_fake = fake.clone();
    // The fetch of the first message gets HTTP 400; the second repairs the file.
    let gmail = client(hook_proxy(fake.addr(), "format=raw", move || {
        at_fake.fail_next(&[400])
    }));

    let r = verify(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(r.differences, vec![Difference::Content(first)]);
    assert_eq!(std::fs::read(&path).unwrap(), raw);
}

#[test]
fn many_bad_files_are_counted_and_the_list_is_bounded() {
    let n = iris_sync::LISTED + 100;
    let (fake, store, account) = synced("v-many", 33, n);
    for e in std::fs::read_dir(store.dir().join("content")).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            std::fs::remove_dir_all(e.path()).unwrap();
        }
    }
    let r = verify(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert_eq!(r.total, n as u64);
    assert_eq!(r.differences.len(), iris_sync::LISTED);
    assert!(!r.resync);
    let again = verify(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert_eq!(again.total, 0, "each file is repaired");
}
