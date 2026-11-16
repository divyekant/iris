//! The first sync against the fake Gmail server. No network.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use iris_fakegmail::FakeGmail;
use iris_gmail::{Config, Gmail};
use iris_store::Store;
use iris_sync::{first_sync, Error, STEP_FETCH, STEP_INCREMENTAL};
use iris_synth::Mailbox;

const TOKEN: &str = "test-token";
const OWNER: &str = "owner@example.com";

fn client(base_url: String) -> Gmail {
    Gmail::new(Config {
        base_url,
        quota_per_minute: 10_000_000,
        backoff_start: Duration::from_millis(1),
        backoff_max: Duration::from_millis(4),
        ..Config::default()
    })
}

fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iris-sync-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn count(store: &Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

fn raw_requests(fake: &FakeGmail) -> usize {
    fake.requests()
        .iter()
        .filter(|r| r.contains("format=raw"))
        .count()
}

/// The fake as the mirror must see it: (message ID, sorted label IDs).
fn fake_state(fake: &FakeGmail) -> Vec<(String, Vec<String>)> {
    let gmail = client(fake.base_url());
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let p = gmail.messages(TOKEN, &[], true, page.as_deref()).unwrap();
        for m in p.messages {
            let mut labels = gmail.minimal(TOKEN, &m.id).unwrap().label_ids;
            labels.sort();
            out.push((m.id, labels));
        }
        match p.next_page_token {
            Some(t) => page = Some(t),
            None => break,
        }
    }
    out.sort();
    out
}

fn mirror_state(store: &Store) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = store
        .read(|c| {
            let mut st = c.prepare(
                "SELECT m.provider_id, coalesce(group_concat(l.provider_id, ' '), '')
                 FROM message m LEFT JOIN message_label ml ON ml.message_id = m.id
                 LEFT JOIN label l ON l.id = ml.label_id
                 WHERE m.gone_at IS NULL GROUP BY m.id",
            )?;
            let rows = st.query_map([], |r| {
                let labels: String = r.get(1)?;
                let mut labels: Vec<String> =
                    labels.split_whitespace().map(str::to_string).collect();
                labels.sort();
                Ok((r.get(0)?, labels))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    out.sort();
    out
}

/// Each fetched message has its content file, with the same bytes as the fake.
fn check_content(store: &Store, mailbox: &Mailbox) {
    let rows: Vec<(String, String, i64)> = store
        .read(|c| {
            let mut st = c.prepare(
                "SELECT provider_id, content_sha256, size FROM message WHERE gone_at IS NULL",
            )?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    for (id, sha, size) in rows {
        let index = (0..mailbox.len())
            .find(|&i| FakeGmail::id_of(i) == id)
            .unwrap();
        let bytes = std::fs::read(store.content_path(&sha).unwrap()).unwrap();
        assert_eq!(bytes, mailbox.raw(index), "content of {id}");
        assert_eq!(size as usize, bytes.len());
    }
}

fn temp_files(dir: &Path) -> usize {
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            n += temp_files(&e.path());
        } else if e.file_name().to_string_lossy().starts_with(".tmp-") {
            n += 1;
        }
    }
    n
}

fn sync_state(store: &Store) -> (Option<String>, Option<String>, Option<i64>, Option<String>) {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT sync_kind, sync_start, sync_step, history_id FROM account",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap()
}

#[test]
fn a_full_copy_has_the_messages_and_labels_of_the_fake() {
    let mailbox = Mailbox::new(7, 300);
    let fake = FakeGmail::start(Mailbox::new(7, 300)).unwrap();
    let gmail = client(fake.base_url());
    let dir = data_dir("full");
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();
    let h0 = gmail.profile(TOKEN).unwrap().history_id;

    let done = first_sync(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(done.fetched, 300);
    assert_eq!(done.gone, 0);
    assert_eq!(done.history_start, h0);

    let labels = gmail.labels(TOKEN).unwrap();
    assert_eq!(
        count(&store, "SELECT count(*) FROM label"),
        labels.len() as i64
    );
    assert_eq!(mirror_state(&store), fake_state(&fake));
    check_content(&store, &mailbox);
    // The history ID is ready for the incremental sync, and only it writes
    // account.history_id (specification 1, section 4.2).
    let (kind, start, step, history) = sync_state(&store);
    assert_eq!(kind.as_deref(), Some("first"));
    assert_eq!(start, Some(h0.to_string()));
    assert_eq!(step, Some(STEP_INCREMENTAL));
    assert_eq!(history, None);
    assert_eq!(temp_files(&dir.join("content")), 0);

    // A call again fetches nothing.
    let before = raw_requests(&fake);
    let again = first_sync(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!((again.fetched, again.gone), (0, 0));
    assert_eq!(raw_requests(&fake), before);
}

#[test]
fn the_fetch_starts_with_the_newest_thread() {
    let fake = FakeGmail::start(Mailbox::new(9, 120)).unwrap();
    let gmail = client(fake.base_url());
    let store = Store::open(&data_dir("order")).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();
    first_sync(&store, &gmail, account, TOKEN).unwrap();

    // The list is newest first, so its first message is the newest.
    let newest = gmail.messages(TOKEN, &[], true, None).unwrap().messages[0].clone();
    let fetched: Vec<String> = fake
        .requests()
        .iter()
        .filter(|r| r.contains("format=raw"))
        .map(|r| {
            r.split("messages/")
                .nth(1)
                .unwrap()
                .split('?')
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    // The fetch reads 64 messages at one time, newest thread first, so each
    // message of the newest thread is in the first 64 fetches of 120.
    let list = gmail.messages(TOKEN, &[], true, None).unwrap().messages;
    let thread: Vec<&String> = list
        .iter()
        .filter(|m| m.thread_id == newest.thread_id)
        .map(|m| &m.id)
        .collect();
    assert!(!thread.is_empty());
    for id in thread {
        assert!(fetched[..64].contains(id), "{id} not in {fetched:?}");
    }
}

#[test]
fn rate_limits_and_server_errors_wait_and_try_again() {
    let fake = FakeGmail::start(Mailbox::new(5, 40)).unwrap();
    let gmail = client(fake.base_url());
    let store = Store::open(&data_dir("retry")).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();
    fake.fail_next(&[429, 503, 403, 500]);
    let done = first_sync(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(done.fetched, 40);
    assert_eq!(mirror_state(&store), fake_state(&fake));
}

#[test]
fn a_token_of_another_mailbox_is_refused() {
    let fake = FakeGmail::start(Mailbox::new(5, 10)).unwrap();
    let gmail = client(fake.base_url());
    let store = Store::open(&data_dir("wrong")).unwrap();
    let account = store.add_account("gmail", "other@example.com").unwrap();
    assert!(matches!(
        first_sync(&store, &gmail, account, TOKEN),
        Err(Error::WrongMailbox)
    ));
    assert_eq!(count(&store, "SELECT count(*) FROM message"), 0);
    assert_eq!(sync_state(&store).0, None);
}

#[test]
fn a_401_stops_the_sync_and_a_new_call_continues() {
    let fake = FakeGmail::start(Mailbox::new(5, 30)).unwrap();
    let gmail = client(fake.base_url());
    let store = Store::open(&data_dir("unauthorized")).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();
    fake.fail_next(&[401]);
    assert!(matches!(
        first_sync(&store, &gmail, account, TOKEN),
        Err(Error::Gmail(iris_gmail::Error::Unauthorized))
    ));
    assert_eq!(
        first_sync(&store, &gmail, account, TOKEN).unwrap().fetched,
        30
    );
}

/// A TCP proxy in front of the fake that drops each connection after
/// `budget` bytes from the server, and refuses each connection after that.
fn cutting_proxy(target: std::net::SocketAddr, budget: i64) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let left = Arc::new(AtomicI64::new(budget));
    std::thread::spawn(move || {
        for client in listener.incoming() {
            let Ok(client) = client else { continue };
            if left.load(Ordering::SeqCst) <= 0 {
                let _ = client.shutdown(Shutdown::Both);
                continue;
            }
            let server = TcpStream::connect(target).unwrap();
            let (mut c_read, mut s_write) =
                (client.try_clone().unwrap(), server.try_clone().unwrap());
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut c_read, &mut s_write);
            });
            let left = left.clone();
            std::thread::spawn(move || {
                let (mut s_read, mut c_write) = (server, client);
                let mut buf = [0u8; 4096];
                loop {
                    let n = match s_read.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    if left.fetch_sub(n as i64, Ordering::SeqCst) <= n as i64 {
                        // Send a part of the answer, then drop the connection.
                        let _ = c_write.write_all(&buf[..n / 2]);
                        break;
                    }
                    if c_write.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
                let _ = c_write.shutdown(Shutdown::Both);
                let _ = s_read.shutdown(Shutdown::Both);
            });
        }
    });
    format!("http://{addr}/gmail/v1")
}

/// Run a first sync through a proxy that drops the connection during the
/// fetch step. Gives the store, with a part of the messages fetched.
fn cut_sync(fake: &FakeGmail, mailbox: &Mailbox, name: &str) -> (Store, i64) {
    let store = Store::open(&data_dir(name)).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();
    // About half of the mailbox: base64 makes each answer 4/3 of the raw size.
    let total: usize = (0..mailbox.len()).map(|i| mailbox.raw(i).len()).sum();
    let cut = client(cutting_proxy(fake.addr(), total as i64 * 2 / 3));
    match first_sync(&store, &cut, account, TOKEN) {
        Err(Error::Gmail(iris_gmail::Error::Transport(_))) => {}
        other => panic!("the proxy did not drop the sync: {other:?}"),
    }
    assert_eq!(sync_state(&store).2, Some(STEP_FETCH));
    (store, account)
}

fn unfetched(store: &Store) -> Vec<String> {
    store
        .read(|c| {
            let mut st = c.prepare("SELECT provider_id FROM message WHERE fetched_at IS NULL")?;
            let rows = st.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap()
}

#[test]
fn a_dropped_connection_resumes_and_fetches_only_the_rest() {
    let mailbox = Mailbox::new(11, 300);
    let fake = FakeGmail::start(Mailbox::new(11, 300)).unwrap();
    let (store, account) = cut_sync(&fake, &mailbox, "dropped");
    let rest = unfetched(&store).len();
    assert!(rest > 0 && rest < 300, "{rest} messages not fetched");
    assert_eq!(temp_files(&store.dir().join("content")), 0);

    let before = raw_requests(&fake);
    let gmail = client(fake.base_url());
    let done = first_sync(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(done.fetched as usize, rest);
    assert_eq!(raw_requests(&fake) - before, rest);
    assert_eq!(mirror_state(&store), fake_state(&fake));
    check_content(&store, &mailbox);
    assert_eq!(sync_state(&store).2, Some(STEP_INCREMENTAL));
}

#[test]
fn a_message_deleted_after_the_list_does_not_stop_the_sync() {
    let fake = FakeGmail::start(Mailbox::new(13, 300)).unwrap();
    let (store, account) = cut_sync(&fake, &Mailbox::new(13, 300), "deleted");
    let victim = unfetched(&store).pop().unwrap();
    assert!(fake.delete_message(&victim));

    let gmail = client(fake.base_url());
    let done = first_sync(&store, &gmail, account, TOKEN).unwrap();
    assert_eq!(done.gone, 1);
    let gone: Vec<String> = store
        .read(|c| {
            let mut st = c.prepare("SELECT provider_id FROM message WHERE gone_at IS NOT NULL")?;
            let rows = st.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    assert_eq!(gone, vec![victim]);
    assert_eq!(mirror_state(&store), fake_state(&fake));
    assert_eq!(sync_state(&store).2, Some(STEP_INCREMENTAL));
}

#[test]
fn a_resume_with_a_token_of_another_mailbox_is_refused() {
    let fake = FakeGmail::start(Mailbox::new(17, 300)).unwrap();
    let (store, account) = cut_sync(&fake, &Mailbox::new(17, 300), "wrong-resume");
    // The fake has one address, so make the account another mailbox: the
    // token now gives a mailbox that is not the account.
    store
        .write(|tx| {
            tx.execute("UPDATE account SET address = 'other@example.com'", [])?;
            Ok(())
        })
        .unwrap();
    let rest = unfetched(&store);
    let before = raw_requests(&fake);
    let gmail = client(fake.base_url());
    assert!(matches!(
        first_sync(&store, &gmail, account, TOKEN),
        Err(Error::WrongMailbox)
    ));
    assert_eq!(raw_requests(&fake), before);
    assert_eq!(unfetched(&store), rest);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM message WHERE gone_at IS NOT NULL"
        ),
        0
    );
    assert_eq!(sync_state(&store).2, Some(STEP_FETCH));
}

#[test]
fn a_second_sync_of_the_account_does_not_run_at_the_same_time() {
    let fake = FakeGmail::start(Mailbox::new(19, 30)).unwrap();
    let gmail = client(fake.base_url());
    let store = Store::open(&data_dir("lock")).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();

    // The incremental sync (R1-06) takes the same lock.
    let held = iris_sync::lock(&store, account).unwrap();
    let before = fake.requests().len();
    assert!(matches!(
        first_sync(&store, &gmail, account, TOKEN),
        Err(Error::OtherSync)
    ));
    assert_eq!(fake.requests().len(), before);
    assert_eq!(count(&store, "SELECT count(*) FROM message"), 0);
    assert!(matches!(iris_sync::lock(&store, account), Err(Error::OtherSync)));

    drop(held);
    assert_eq!(first_sync(&store, &gmail, account, TOKEN).unwrap().fetched, 30);
}
