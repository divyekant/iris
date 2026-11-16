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

    // The caller gets the 401, so that it can get a new token.
    let e = verify(&store, &gmail, account, TOKEN).unwrap_err();
    assert!(
        matches!(e, iris_sync::Error::Gmail(iris_gmail::Error::Unauthorized)),
        "{e}"
    );
    // The log and the resync are saved before the error.
    let logged: String = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT result FROM sync_log WHERE kind = 'verify' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(logged.starts_with("2 differences"), "{logged}");
    assert!(logged.contains(&bad), "{logged}");
    assert_eq!(sync_state(&store).0.as_deref(), Some(KIND_RESYNC));
    // With a new token, the next verify runs the resync, which repairs the
    // labels, and repairs the content file.
    let r = verify(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert_eq!(r.differences, vec![Difference::Content(bad)]);
    assert!(!r.resync);
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
fn a_message_left_off_the_list_by_a_page_shift_is_still_compared() {
    let (fake, store, account) = synced("v-page-shift", 36, 700);
    // The delete of message 690 (page one) moves message 199 from the start
    // of page two to the end of page one, after Gmail sent page one.
    let omitted = FakeGmail::id_of(199);
    store
        .write(|tx| {
            tx.execute(
                "DELETE FROM message_label WHERE message_id =
                   (SELECT id FROM message WHERE provider_id = ?1)",
                [&omitted],
            )?;
            Ok(())
        })
        .unwrap();
    let at_fake = fake.clone();
    let gmail = client(hook_proxy(fake.addr(), "pageToken=", move || {
        assert!(at_fake.delete_message(&FakeGmail::id_of(690)));
    }));
    let r = verify(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(r.differences, vec![Difference::Labels(omitted)]);
    assert!(r.resync);
}

#[test]
fn a_delete_between_two_pages_of_a_label_list_gives_no_difference() {
    let (fake, store, account) = synced("v-label-shift", 37, 1_200);
    let inbox: Vec<String> = fake_state(&fake)
        .into_iter()
        .filter(|(_, l)| l.iter().any(|l| l == "INBOX"))
        .map(|(id, _)| id)
        .collect();
    assert!(inbox.len() > 500, "INBOX needs two list pages");
    // `fake_state` sorts by ID, and a higher ID is newer: the last INBOX
    // message is on the first page of the newest-first INBOX list.
    let deleted = inbox.last().unwrap().clone();
    eprintln!(
        "INBOX n={} first={} at500={} at499={}",
        inbox.len(),
        inbox[0],
        inbox[500],
        inbox[499]
    );
    let at_fake = fake.clone();
    let gmail = client(hook_proxy(
        fake.addr(),
        "labelIds=INBOX&includeSpamTrash=true&pageToken=",
        move || assert!(at_fake.delete_message(&deleted)),
    ));
    let r = verify(&store, &gmail, account, TOKEN).unwrap();
    for q in fake.requests().iter().filter(|r| r.contains("INBOX")) {
        eprintln!("REQ {q}");
    }
    eprintln!("DBG {:?} skipped {}", r.differences, r.skipped);
    assert_eq!(r.differences, vec![]);
    assert!(!r.resync);
}

#[test]
fn a_compare_error_keeps_the_difference_found_before_it() {
    let (fake, store, account) = synced("v-error-keeps", 38, 30);
    let missing = FakeGmail::id_of(9);
    store
        .write(|tx| {
            tx.execute(
                "UPDATE message SET gone_at = 1 WHERE provider_id = ?1",
                [&missing],
            )?;
            // A mirror row that Gmail does not list.
            tx.execute(
                "INSERT INTO message(account_id, thread_id, provider_id, fetched_at)
                 SELECT account_id, thread_id, 'ffffffffffffffff', 1 FROM message LIMIT 1",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let at_fake = fake.clone();
    let gmail = client(hook_proxy(fake.addr(), "format=raw", move || {
        at_fake.fail_next(&[400])
    }));
    assert!(verify(&store, &gmail, account, TOKEN).is_err());
    let logged: String = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT result FROM sync_log WHERE kind = 'verify' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(logged.contains(&missing), "{logged}");
    assert_eq!(sync_state(&store).0.as_deref(), Some(KIND_RESYNC));
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

#[test]
fn a_400_of_the_extra_message_check_is_an_error_not_a_difference() {
    let (fake, store, account) = synced("v-extra-400", 34, 30);
    // A mirror row that Gmail does not list.
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO message(account_id, thread_id, provider_id, fetched_at)
                 SELECT account_id, thread_id, 'ffffffffffffffff', 1 FROM message LIMIT 1",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let at_fake = fake.clone();
    let gmail = client(hook_proxy(fake.addr(), "format=raw", move || {
        at_fake.fail_next(&[400])
    }));
    match verify(&store, &gmail, account, TOKEN) {
        Err(iris_sync::Error::Gmail(iris_gmail::Error::Api { status: 400, .. })) => {}
        other => panic!("not the 400: {other:?}"),
    }
    assert_eq!(sync_state(&store).0, None, "no resync");
    // Without the failure, Gmail gives 404: a real difference.
    let r = verify(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert_eq!(
        r.differences[0],
        Difference::ExtraMessage("ffffffffffffffff".into())
    );
    assert!(r.resync);
}

#[test]
fn many_thread_differences_are_counted_with_flat_memory() {
    let n = iris_sync::LISTED + 50;
    let (fake, store, account) = synced("v-threads", 35, n);
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO thread(account_id, provider_id) SELECT id, 'wrong' FROM account",
                [],
            )?;
            tx.execute(
                "UPDATE message SET thread_id = (SELECT id FROM thread WHERE provider_id = 'wrong')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let r = verify(&store, &client(fake.base_url()), account, TOKEN).unwrap();
    assert_eq!(r.total, n as u64);
    assert_eq!(r.differences.len(), iris_sync::LISTED);
    assert!(r.resync);
}

#[test]
fn a_changed_message_past_many_thread_differences_is_left_out() {
    let n = iris_sync::LISTED + 50;
    let (fake, store, account) = synced("v-threads-changed", 36, n);
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO thread(account_id, provider_id) SELECT id, 'wrong' FROM account",
                [],
            )?;
            tx.execute(
                "UPDATE message SET thread_id = (SELECT id FROM thread WHERE provider_id = 'wrong')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    // Message 0 is the oldest, so the newest-first list names it last.
    let at_fake = fake.clone();
    let gmail = client(hook_proxy(fake.addr(), "labelIds=", move || {
        assert!(at_fake.modify_labels(&FakeGmail::id_of(0), &["STARRED"], &[]));
    }));
    let r = verify(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(r.total, n as u64 - 1);
    assert!(!r
        .differences
        .contains(&Difference::Thread(FakeGmail::id_of(0))));
}
