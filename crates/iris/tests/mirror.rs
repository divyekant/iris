//! `iris mirror sync` and `iris mirror verify` against the fake Gmail.

mod common;

use std::process::Command;

use common::*;
use iris_fakegmail::FakeGmail;
use iris_store::Store;
use iris_synth::Mailbox;

fn iris(
    dir: &std::path::Path,
    fake: &FakeGmail,
    tokens: &TokenServer,
    args: &[&str],
) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(args)
        .arg("--data")
        .arg(dir)
        .env("IRIS_TEST_GMAIL_URL", fake.base_url())
        .env("IRIS_TEST_TOKEN_URI", &tokens.uri)
        .env("IRIS_GMAIL_QUOTA", "100000000")
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn mirror_sync_then_verify_finds_no_difference() {
    let fake = FakeGmail::start(Mailbox::new(5, 25)).unwrap();
    let tokens = token_server();
    let dir = data_dir("mirror");
    let store = Store::open(&dir).unwrap();
    add_account(&store, &gmail(fake.base_url()));

    let (ok, out, err) = iris(&dir, &fake, &tokens, &["mirror", "sync"]);
    assert!(ok, "{err}");
    assert!(out.contains("account 1: synced to history"), "{out}");
    assert!(out.contains("fetched 25"), "{out}");

    let (ok, out, err) = iris(&dir, &fake, &tokens, &["mirror", "verify"]);
    assert!(ok, "{err}");
    assert!(out.contains("account 1: verified at history"), "{out}");
    assert!(out.contains(": 0 differences"), "{out}");

    // A broken content file is a difference, and verify fetches it again.
    let sha: String = store
        .read(|c| {
            Ok(
                c.query_row("SELECT content_sha256 FROM message LIMIT 1", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    std::fs::write(store.content_path(&sha).unwrap(), b"broken").unwrap();
    let (ok, out, err) = iris(&dir, &fake, &tokens, &["mirror", "verify"]);
    assert!(ok, "{err}");
    assert!(out.contains(": 1 differences"), "{out}");
    assert!(out.contains("bad content file, fetched again"), "{out}");
}

#[test]
fn mirror_sync_with_no_account_fails() {
    let fake = FakeGmail::start(Mailbox::new(5, 1)).unwrap();
    let tokens = token_server();
    let (ok, _, err) = iris(
        &data_dir("mirror-empty"),
        &fake,
        &tokens,
        &["mirror", "sync"],
    );
    assert!(!ok);
    assert!(err.contains("no account"), "{err}");
}

#[test]
fn mirror_verify_reports_an_account_that_needs_sign_in() {
    let fake = FakeGmail::start(Mailbox::new(5, 3)).unwrap();
    let tokens = token_server();
    let dir = data_dir("mirror-sign-in");
    let store = Store::open(&dir).unwrap();
    let first = add_account(&store, &gmail(fake.base_url()));
    rename_account(&store, first, "old@example.com");
    store
        .write(|tx| {
            tx.execute(
                "UPDATE account SET state = 'needs_sign_in' WHERE id = ?1",
                [first],
            )?;
            Ok(())
        })
        .unwrap();
    add_account(&store, &gmail(fake.base_url()));
    let (ok, out, err) = iris(&dir, &fake, &tokens, &["mirror", "verify"]);
    assert!(!ok, "a mailbox that did not run must fail the command");
    assert!(
        err.contains("account 1: the account needs a new sign-in"),
        "{err}"
    );
    assert!(out.contains("account 2: verified at history"), "{out}");
}

#[test]
fn mirror_sync_runs_understanding_so_search_finds_words_and_senders() {
    let mailbox = Mailbox::new(5, 25);
    // A message of a known person: its `From` address and a word of its
    // subject, read from the raw bytes that the fake serves.
    let (from, word) = (0..mailbox.len())
        .filter(|&i| mailbox.truth(i).sender == iris_synth::Sender::KnownPerson)
        .find_map(|i| {
            let raw = mailbox.raw(i);
            let head =
                iris_understand::Head::parse(&raw, Some(iris_understand::head::GMAIL_AUTHSERV));
            let text = String::from_utf8_lossy(&raw).to_string();
            let subject = text.lines().find_map(|l| l.strip_prefix("Subject: "))?;
            if subject.contains("=?") {
                return None;
            }
            let word = subject
                .split(|c: char| !c.is_ascii_alphabetic())
                .find(|w| w.len() >= 5)?
                .to_lowercase();
            Some((head.from_addr?, word))
        })
        .expect("a known person with a subject word");
    let fake = FakeGmail::start(mailbox).unwrap();
    let tokens = token_server();
    let dir = data_dir("mirror-understand");
    let store = Store::open(&dir).unwrap();
    add_account(&store, &gmail(fake.base_url()));

    let (ok, out, err) = iris(&dir, &fake, &tokens, &["mirror", "sync"]);
    assert!(ok, "{err}");
    assert!(out.contains("understand: Run "), "{out}");
    for query in [word, format!("from:{from}")] {
        let (ok, out, err) = iris(&dir, &fake, &tokens, &["search", &query, "--json"]);
        assert!(ok, "{err}");
        assert!(!out.trim().is_empty(), "no result for {query}");
    }
    // A new item gets its attention at the next sync (spec 2 section 6.4).
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 SELECT 'feed', account_id, 'new', '{}', 0, 0 FROM message LIMIT 1",
                [],
            )?;
            tx.execute(
                "INSERT INTO item_source(item_id, message_id, role)
                 SELECT last_insert_rowid(), min(id), 'origin' FROM message",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let (ok, _, err) = iris(&dir, &fake, &tokens, &["mirror", "sync"]);
    assert!(ok, "{err}");
    let state: String = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT json_extract(doc, '$.attention.state') FROM item",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(state, "shown");
}

#[test]
fn mirror_sync_waits_for_a_run_that_holds_the_lock() {
    let fake = FakeGmail::start(Mailbox::new(5, 10)).unwrap();
    let tokens = token_server();
    let dir = data_dir("mirror-busy");
    let store = Store::open(&dir).unwrap();
    add_account(&store, &gmail(fake.base_url()));
    let held = iris_understand::batch::lock(&store).unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(2));
        drop(held);
    });
    let (ok, out, err) = iris(&dir, &fake, &tokens, &["mirror", "sync"]);
    release.join().unwrap();
    assert!(ok, "{err}");
    assert!(out.contains("understand: waits for another run"), "{out}");
    assert!(out.contains("understand: Run "), "{out}");
    let headers: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM header", [], |r| r.get(0))?))
        .unwrap();
    assert!(headers > 0);
}

/// A model server on 127.0.0.1 that answers each request with HTTP 500.
/// Gives the base URL and the count of requests.
fn failing_model_server() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    let count = std::sync::Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut r = BufReader::new(s.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            c.fetch_add(1, Ordering::SeqCst);
            let _ = write!(
                s,
                "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}"
            );
        }
    });
    (url, count)
}

#[test]
fn a_second_mirror_sync_after_a_model_error_makes_no_call() {
    let fake = FakeGmail::start(Mailbox::new(5, 10)).unwrap();
    let tokens = token_server();
    let dir = data_dir("mirror-retry");
    let store = Store::open(&dir).unwrap();
    add_account(&store, &gmail(fake.base_url()));
    let (url, calls) = failing_model_server();
    let (ok, _, err) = iris(&dir, &fake, &tokens, &["model", "set", "local", &url, "m"]);
    assert!(ok, "{err}");
    let (ok, out, err) = iris(&dir, &fake, &tokens, &["mirror", "sync"]);
    assert!(ok, "{err}");
    assert!(out.contains("Stop: model"), "{out}");
    let first = calls.load(std::sync::atomic::Ordering::SeqCst);
    assert!(first > 0);
    let (ok, _, err) = iris(&dir, &fake, &tokens, &["mirror", "sync"]);
    assert!(ok, "{err}");
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        first,
        "the wait after a model error holds across commands"
    );
}

/// Issue #679: while another process runs the first copy of an account,
/// verify of that account says that the first copy runs, and the command
/// does not fail for it. A held lock of an account with a full mirror is
/// still a failure.
#[test]
fn mirror_verify_during_a_first_copy_says_so_and_does_not_fail() {
    let fake = FakeGmail::start(Mailbox::new(5, 3)).unwrap();
    let tokens = token_server();
    let dir = data_dir("mirror-first-copy");
    let store = Store::open(&dir).unwrap();
    let account = add_account(&store, &gmail(fake.base_url()));
    let held = iris_sync::lock(&store, account).unwrap();
    // The token endpoint fails: the check comes before any token call.
    tokens
        .answers
        .lock()
        .unwrap()
        .extend([(500, "{}".to_string()), (500, "{}".to_string())]);
    let (ok, out, err) = iris(&dir, &fake, &tokens, &["mirror", "verify"]);
    assert!(ok, "{err}");
    assert!(
        out.contains(&format!(
            "account {account}: the first copy runs now, so no full mirror exists to verify. Run verify after it ends."
        )),
        "{out}"
    );
    assert!(tokens.calls.lock().unwrap().is_empty(), "a token call");
    tokens.answers.lock().unwrap().clear();
    drop(held);
    let (ok, _, err) = iris(&dir, &fake, &tokens, &["mirror", "sync"]);
    assert!(ok, "{err}");
    let _held = iris_sync::lock(&store, account).unwrap();
    let (ok, _, err) = iris(&dir, &fake, &tokens, &["mirror", "verify"]);
    assert!(!ok);
    assert!(err.contains("another sync of the account runs"), "{err}");
}

/// Issue #836: verify takes the sync lock before its token call. A first
/// copy that starts while the token request runs cannot take the lock, so
/// a failed token request cannot hide a first copy that runs under it.
#[test]
fn mirror_verify_holds_the_sync_lock_during_its_token_call() {
    let fake = FakeGmail::start(Mailbox::new(5, 3)).unwrap();
    let tokens = token_server();
    let dir = data_dir("mirror-verify-lock");
    let store = Store::open(&dir).unwrap();
    let account = add_account(&store, &gmail(fake.base_url()));
    // Each token request stalls.
    *tokens.stall.lock().unwrap() = Some(String::new());
    let mut child = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(["mirror", "verify", "--data"])
        .arg(&dir)
        .env("IRIS_TEST_GMAIL_URL", fake.base_url())
        .env("IRIS_TEST_TOKEN_URI", &tokens.uri)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while tokens.calls.lock().unwrap().is_empty() {
        assert!(start.elapsed().as_secs() < 60, "no token call");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let first_copy = iris_sync::lock(&store, account);
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        matches!(first_copy, Err(iris_sync::Error::OtherSync)),
        "a first copy could start during the token call of verify"
    );
}
