//! The engine with a fake Gmail and a fake token endpoint: no network, no
//! real mailbox and no real token.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use iris::engine::{self, Engine, Worker, STATE_NEEDS_SIGN_IN, STATE_OK};
use iris_fakegmail::FakeGmail;
use iris_store::Store;
use iris_synth::Mailbox;

struct Setup {
    store: Arc<Store>,
    fake: FakeGmail,
    tokens: TokenServer,
    account: i64,
}

impl Setup {
    fn worker(&self, account: i64) -> Worker {
        Worker::new(
            self.store.clone(),
            gmail(self.fake.base_url()),
            account,
            Some(self.tokens.uri.clone()),
        )
    }
}

fn setup(name: &str, count: usize) -> Setup {
    let fake = FakeGmail::start(Mailbox::new(7, count)).unwrap();
    let tokens = token_server();
    let store = Arc::new(Store::open(&data_dir(name)).unwrap());
    let account = add_account(&store, &gmail(fake.base_url()));
    Setup {
        store,
        fake,
        tokens,
        account,
    }
}

fn token_of(store: &Store, account: i64) -> Vec<u8> {
    store
        .read(|c| {
            Ok(
                c.query_row("SELECT token FROM account WHERE id = ?1", [account], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn the_refresh_token_and_the_client_are_stored_sealed() {
    let s = setup("sealed", 1);
    let sealed = token_of(&s.store, s.account);
    assert!(
        !contains(&sealed, REFRESH.as_bytes()),
        "plain refresh token in iris.db"
    );
    assert!(
        !contains(&sealed, CLIENT_SECRET.as_bytes()),
        "plain client secret in iris.db"
    );
    let plain = String::from_utf8(s.store.unseal(&sealed).unwrap()).unwrap();
    assert!(plain.contains(REFRESH) && plain.contains(CLIENT_SECRET));
    assert_state(&s.store, s.account, STATE_OK);
    // No file outside iris.db holds the client.
    for f in std::fs::read_dir(s.store.dir()).unwrap() {
        let path = f.unwrap().path();
        if path.is_file() {
            let bytes = std::fs::read(&path).unwrap();
            assert!(!contains(&bytes, CLIENT_SECRET.as_bytes()), "{path:?}");
        }
    }
}

#[test]
fn a_sync_gets_a_token_from_the_stored_sign_in() {
    let s = setup("first", 30);
    let mut w = s.worker(s.account);
    assert_eq!(w.sync().unwrap().fetched, 30);
    assert_eq!(
        count(
            &s.store,
            "SELECT count(*) FROM account WHERE mirror_ready_at IS NOT NULL"
        ),
        1
    );
    let calls = s.tokens.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].contains("grant_type=refresh_token"));
    assert!(calls[0].contains(REFRESH));
    // The second sync uses the token in memory.
    w.sync().unwrap();
    assert_eq!(s.tokens.calls.lock().unwrap().len(), 1);
}

#[test]
fn a_401_gets_a_new_token_one_time() {
    let s = setup("unauthorized", 5);
    let mut w = s.worker(s.account);
    w.sync().unwrap();
    s.fake.fail_next(&[401]);
    w.sync().unwrap();
    assert_eq!(s.tokens.calls.lock().unwrap().len(), 2);
}

#[test]
fn a_401_after_a_new_token_needs_sign_in() {
    let s = setup("unauthorized-twice", 5);
    let mut w = s.worker(s.account);
    w.sync().unwrap();
    s.fake.fail_next(&[401, 401]);
    let r = w.sync();
    assert!(matches!(r, Err(engine::Error::NeedsSignIn)), "{r:?}");
    assert_state(&s.store, s.account, STATE_NEEDS_SIGN_IN);
    assert!(engine::sync_accounts(&s.store).unwrap().is_empty());
}

#[test]
fn a_refused_refresh_token_needs_sign_in() {
    let s = setup("revoked", 5);
    s.tokens
        .answers
        .lock()
        .unwrap()
        .push_back((400, r#"{"error":"invalid_grant"}"#.into()));
    let r = s.worker(s.account).sync();
    assert!(matches!(r, Err(engine::Error::NeedsSignIn)), "{r:?}");
    assert_state(&s.store, s.account, STATE_NEEDS_SIGN_IN);
    assert!(engine::sync_accounts(&s.store).unwrap().is_empty());
}

#[test]
fn a_token_endpoint_error_keeps_the_account() {
    let s = setup("token-down", 5);
    s.tokens
        .answers
        .lock()
        .unwrap()
        .push_back((500, r#"{"error":"internal_failure"}"#.into()));
    let mut w = s.worker(s.account);
    assert!(w.sync().is_err());
    assert_state(&s.store, s.account, STATE_OK);
    // The next sync tries again.
    w.sync().unwrap();
}

#[test]
fn a_failed_old_sign_in_does_not_undo_a_new_sign_in() {
    let s = setup("stale", 1);
    let old = token_of(&s.store, s.account);
    // `iris account add` again, while a refresh with the old sign-in runs.
    add_account(&s.store, &gmail(s.fake.base_url()));
    assert!(!engine::mark_needs_sign_in(&s.store, s.account, &old).unwrap());
    assert_state(&s.store, s.account, STATE_OK);
    let new = token_of(&s.store, s.account);
    assert!(engine::mark_needs_sign_in(&s.store, s.account, &new).unwrap());
    assert_state(&s.store, s.account, STATE_NEEDS_SIGN_IN);
}

#[test]
fn each_account_refreshes_with_its_own_client() {
    let s = setup("two-clients", 1);
    rename_account(&s.store, s.account, "first@example.com");
    let other = client_json().replace("fake-id", "other-id");
    let second = sign_in(
        &s.store,
        &gmail(s.fake.base_url()),
        &other,
        "refresh-of-other",
    );
    let _ = s.worker(s.account).sync(); // WrongMailbox: the fake has one address.
    s.worker(second).sync().unwrap();
    let calls = s.tokens.calls.lock().unwrap().clone();
    assert!(
        calls[0].contains("client_id=fake-id") && calls[0].contains(REFRESH),
        "{calls:?}"
    );
    assert!(
        calls[1].contains("client_id=other-id") && calls[1].contains("refresh-of-other"),
        "{calls:?}"
    );
}

#[test]
fn a_stalled_account_does_not_stop_another_account() {
    let s = setup("stalled", 20);
    rename_account(&s.store, s.account, "stalled@example.com");
    *s.tokens.stall.lock().unwrap() = Some(REFRESH.to_string());
    let second = sign_in(
        &s.store,
        &gmail(s.fake.base_url()),
        &client_json(),
        "refresh-two",
    );
    let mut e = Engine::new(
        s.store.clone(),
        config(s.fake.base_url()),
        Some(s.tokens.uri.clone()),
    );
    assert_eq!(e.tick().unwrap(), vec![s.account, second]);
    wait_for(
        &s.store,
        &format!(
            "SELECT count(*) FROM account WHERE id = {second} AND mirror_ready_at IS NOT NULL"
        ),
        Duration::from_secs(30),
    );
    // A thread runs for each account: the next tick starts none.
    assert!(e.tick().unwrap().is_empty());
}

#[test]
fn an_account_added_while_the_engine_runs_starts_to_sync() {
    let fake = FakeGmail::start(Mailbox::new(7, 3)).unwrap();
    let tokens = token_server();
    let store = Arc::new(Store::open(&data_dir("late-add")).unwrap());
    let mut e = Engine::new(
        store.clone(),
        config(fake.base_url()),
        Some(tokens.uri.clone()),
    );
    assert!(e.tick().unwrap().is_empty(), "no account yet");
    let account = add_account(&store, &gmail(fake.base_url()));
    assert_eq!(e.tick().unwrap(), vec![account]);
    wait_for(
        &store,
        "SELECT count(*) FROM account WHERE mirror_ready_at IS NOT NULL",
        Duration::from_secs(30),
    );
}

fn assert_state(store: &Store, account: i64, state: &str) {
    let s: String = store
        .read(|c| {
            Ok(
                c.query_row("SELECT state FROM account WHERE id = ?1", [account], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(s, state);
}

#[test]
fn account_add_signs_in_with_a_pasted_address() {
    let fake = FakeGmail::start(Mailbox::new(7, 1)).unwrap();
    let tokens = token_server();
    let dir = data_dir("add");
    let client = dir.with_extension("client.json");
    std::fs::write(&client, client_json()).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(["account", "add", "--paste", "--data"])
        .arg(&dir)
        .arg(&client)
        .env("IRIS_TEST_GMAIL_URL", fake.base_url())
        .env("IRIS_TEST_TOKEN_URI", &tokens.uri)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let url = loop {
        let mut line = String::new();
        assert!(
            stderr.read_line(&mut line).unwrap() > 0,
            "no sign-in address"
        );
        if line.starts_with("https://accounts.google.com/") {
            break line;
        }
    };
    let state = url
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("state="))
        .unwrap()
        .trim();
    writeln!(
        child.stdin.take().unwrap(),
        "http://127.0.0.1:1/oauth/callback?state={state}&code=fake-code&scope={}",
        iris_auth::SCOPE
    )
    .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("added owner@example.com"));
    assert!(tokens.calls.lock().unwrap()[0].contains("grant_type=authorization_code"));

    let store = Arc::new(Store::open(&dir).unwrap());
    assert_eq!(engine::sync_accounts(&store).unwrap(), vec![1]);
    Worker::new(store, gmail(fake.base_url()), 1, Some(tokens.uri.clone()))
        .sync()
        .unwrap();
}

#[test]
fn test_overrides_refuse_an_address_that_is_not_local() {
    let dir = data_dir("override");
    let client = dir.with_extension("client.json");
    std::fs::write(&client, client_json()).unwrap();
    for (var, value) in [
        ("IRIS_TEST_TOKEN_URI", "https://attacker.example/token"),
        (
            "IRIS_TEST_GMAIL_URL",
            "http://127.0.0.1:80@attacker.example/gmail/v1",
        ),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_iris"))
            .args(["account", "add", "--paste", "--data"])
            .arg(&dir)
            .arg(&client)
            .env(var, value)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!out.status.success(), "{var}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("127.0.0.1"),
            "{var}: {out:?}"
        );
    }
}

#[test]
fn backup_copies_the_data_folder_with_the_sign_in() {
    let s = setup("backup", 10);
    s.worker(s.account).sync().unwrap();
    let dest = s.store.dir().with_extension("copy");
    let _ = std::fs::remove_dir_all(&dest);
    let out = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(["backup", "--data"])
        .arg(s.store.dir())
        .arg(&dest)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("10 content files"));
    let copy = Arc::new(Store::open(&dest).unwrap());
    assert_eq!(count(&copy, "SELECT count(*) FROM message"), 10);
    // The copy has the key and the sign-in: it syncs.
    Worker::new(
        copy,
        gmail(s.fake.base_url()),
        s.account,
        Some(s.tokens.uri.clone()),
    )
    .sync()
    .unwrap();
}
