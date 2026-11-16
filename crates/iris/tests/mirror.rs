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
