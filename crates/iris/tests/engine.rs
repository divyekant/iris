//! The engine with a fake Gmail and a fake token endpoint: no network, no
//! real mailbox and no real token.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;

use common::*;
use iris::engine::{self, Engine, STATE_NEEDS_SIGN_IN, STATE_OK};
use iris_fakegmail::FakeGmail;
use iris_store::Store;
use iris_synth::Mailbox;

fn engine(name: &str, count: usize) -> (Engine, FakeGmail, TokenServer, i64) {
    let fake = FakeGmail::start(Mailbox::new(7, count)).unwrap();
    let tokens = token_server();
    let store = Arc::new(Store::open(&data_dir(name)).unwrap());
    let account = add_account(&store, &gmail(fake.base_url()));
    let client = engine::load_client(&store)
        .unwrap()
        .with_test_token_uri(&tokens.uri)
        .unwrap();
    (
        Engine::new(store, gmail(fake.base_url()), Some(client)),
        fake,
        tokens,
        account,
    )
}

#[test]
fn the_refresh_token_and_the_client_are_stored_sealed() {
    let (e, _fake, _tokens, account) = engine("sealed", 1);
    let row: (Vec<u8>, String) = e
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT token, state FROM account WHERE id = ?1",
                [account],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(row.1, STATE_OK);
    assert!(
        !contains(&row.0, REFRESH.as_bytes()),
        "plain refresh token in iris.db"
    );
    let file = std::fs::read(e.store.dir().join("google-client")).unwrap();
    assert!(
        !contains(&file, CLIENT_SECRET.as_bytes()),
        "plain client secret on disk"
    );
    assert_eq!(e.store.unseal(&row.0).unwrap(), REFRESH.as_bytes());
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn a_tick_runs_the_first_sync_with_a_refreshed_token() {
    let (mut e, _fake, tokens, account) = engine("tick", 30);
    let results = e.tick();
    assert_eq!(results.len(), 1);
    let synced = results.into_iter().next().unwrap().1.unwrap();
    assert_eq!(synced.fetched, 30);
    assert_eq!(
        count(
            &e.store,
            "SELECT count(*) FROM message WHERE fetched_at IS NOT NULL"
        ),
        30
    );
    assert_eq!(
        count(
            &e.store,
            &format!(
                "SELECT count(*) FROM account WHERE id = {account} AND mirror_ready_at IS NOT NULL"
            )
        ),
        1
    );
    // The engine got the access token from the stored refresh token.
    let calls = tokens.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].contains("grant_type=refresh_token"));
    assert!(calls[0].contains(REFRESH));
    // The second tick uses the token in memory.
    e.tick().into_iter().next().unwrap().1.unwrap();
    assert_eq!(tokens.calls.lock().unwrap().len(), 1);
}

#[test]
fn a_401_gets_a_new_token_one_time() {
    let (mut e, fake, tokens, _) = engine("unauthorized", 5);
    e.tick().into_iter().next().unwrap().1.unwrap();
    fake.fail_next(&[401]);
    e.tick().into_iter().next().unwrap().1.unwrap();
    assert_eq!(tokens.calls.lock().unwrap().len(), 2);
}

#[test]
fn a_401_after_a_new_token_needs_sign_in() {
    let (mut e, fake, _tokens, account) = engine("unauthorized-twice", 5);
    e.tick().into_iter().next().unwrap().1.unwrap();
    fake.fail_next(&[401, 401]);
    let r = e.tick().into_iter().next().unwrap().1;
    assert!(matches!(r, Err(engine::Error::NeedsSignIn)), "{r:?}");
    assert_state(&e.store, account, STATE_NEEDS_SIGN_IN);
    assert!(
        e.tick().is_empty(),
        "an account that needs sign-in does not sync"
    );
}

#[test]
fn a_refused_refresh_token_needs_sign_in() {
    let (mut e, _fake, tokens, account) = engine("revoked", 5);
    tokens
        .answers
        .lock()
        .unwrap()
        .push_back((400, r#"{"error":"invalid_grant"}"#.into()));
    let r = e.tick().into_iter().next().unwrap().1;
    assert!(matches!(r, Err(engine::Error::NeedsSignIn)), "{r:?}");
    assert_state(&e.store, account, STATE_NEEDS_SIGN_IN);
    assert!(engine::sync_accounts(&e.store).unwrap().is_empty());
}

#[test]
fn a_token_endpoint_error_keeps_the_account() {
    let (mut e, _fake, tokens, account) = engine("token-down", 5);
    tokens
        .answers
        .lock()
        .unwrap()
        .push_back((500, r#"{"error":"internal_failure"}"#.into()));
    assert!(e.tick().into_iter().next().unwrap().1.is_err());
    assert_state(&e.store, account, STATE_OK);
    // The next tick tries again and syncs.
    e.tick().into_iter().next().unwrap().1.unwrap();
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

    let store = Store::open(&dir).unwrap();
    assert_eq!(engine::sync_accounts(&store).unwrap().len(), 1);
    assert!(engine::load_client(&store).is_ok());
}

#[test]
fn test_overrides_refuse_an_address_that_is_not_local() {
    let dir = data_dir("override");
    let client = dir.with_extension("client.json");
    std::fs::write(&client, client_json()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(["account", "add", "--paste", "--data"])
        .arg(&dir)
        .arg(&client)
        .env("IRIS_TEST_TOKEN_URI", "https://attacker.example/token")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("127.0.0.1"));
}

#[test]
fn backup_copies_the_data_folder() {
    let (mut e, _fake, _tokens, _) = engine("backup", 10);
    e.tick().into_iter().next().unwrap().1.unwrap();
    let dest = e.store.dir().with_extension("copy");
    let _ = std::fs::remove_dir_all(&dest);
    let out = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(["backup", "--data"])
        .arg(e.store.dir())
        .arg(&dest)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("10 content files"));
    let copy = Store::open(&dest).unwrap();
    assert_eq!(count(&copy, "SELECT count(*) FROM message"), 10);
    // The copy has the key: the sealed client opens.
    assert!(engine::load_client(&copy).is_ok());
}
