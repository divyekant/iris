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
            let head = iris_understand::Head::parse(&raw);
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
