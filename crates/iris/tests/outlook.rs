//! The Outlook account of the engine (docs/specs/2026-11-17-outlook-provider.md,
//! section 7 and card R6-16): `iris account add-outlook`, the sealed refresh
//! token that Microsoft replaces at each refresh, the provider of each
//! account and the sync against the fake Graph server.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use common::{count, data_dir, token_server, TokenServer};
use iris_fakegraph::FakeGraph;
use iris_store::Store;
use iris_synth::Mailbox;

const CLIENT_ID: &str = "00000000-0000-4000-8000-000000000001";

/// A Microsoft token answer that gives the refresh token `refresh`.
fn answer(refresh: &str) -> (u16, String) {
    (
        200,
        format!(
            r#"{{"access_token":"fake-access","expires_in":3600,"refresh_token":"{refresh}",
            "scope":"{}","token_type":"Bearer"}}"#,
            iris_auth::MICROSOFT_SCOPE
        ),
    )
}

fn iris(dir: &Path, fake: &FakeGraph, tokens: &TokenServer, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_iris"));
    c.args(args)
        .arg("--data")
        .arg(dir)
        .env("IRIS_TEST_GRAPH_URL", fake.base_url())
        .env("IRIS_TEST_TOKEN_URI", &tokens.uri);
    c
}

fn run(dir: &Path, fake: &FakeGraph, tokens: &TokenServer, args: &[&str]) -> (bool, String) {
    let out = iris(dir, fake, tokens, args).output().unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), text)
}

/// `iris account add-outlook --paste`, with the redirect address pasted.
fn add(dir: &Path, fake: &FakeGraph, tokens: &TokenServer) -> (bool, String) {
    let mut child = iris(
        dir,
        fake,
        tokens,
        &["account", "add-outlook", CLIENT_ID, "--paste"],
    )
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
        if line.starts_with("https://login.microsoftonline.com/common/") {
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
        "http://localhost:1/oauth/callback?state={state}&code=fake-code"
    )
    .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// The provider, the state and the unsealed token of account 1.
fn account(dir: &Path) -> (String, String, String) {
    let store = Store::open(dir).unwrap();
    let (provider, state, sealed): (String, String, Vec<u8>) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT provider, state, token FROM account WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    let plain = String::from_utf8(store.unseal(&sealed).unwrap()).unwrap();
    (provider, state, plain)
}

#[test]
fn an_outlook_account_signs_in_and_syncs_with_zero_differences() {
    let fake = FakeGraph::start(Mailbox::new(23, 5)).unwrap();
    let tokens = token_server();
    tokens.answers.lock().unwrap().extend([
        answer("refresh-1"),
        answer("refresh-2"),
        answer("refresh-3"),
    ]);
    let dir = data_dir("outlook-sync");

    let (ok, out) = add(&dir, &fake, &tokens);
    assert!(ok && out.contains("added owner@"), "{out}");
    let (provider, state, plain) = account(&dir);
    assert_eq!((provider.as_str(), state.as_str()), ("outlook", "ok"));
    assert!(
        plain.contains("refresh-1") && plain.contains(CLIENT_ID),
        "{plain}"
    );
    // The column holds the token sealed, never in plain text.
    let store = Store::open(&dir).unwrap();
    let sealed: Vec<u8> = store
        .read(|c| Ok(c.query_row("SELECT token FROM account WHERE id = 1", [], |r| r.get(0))?))
        .unwrap();
    assert!(!sealed.windows(9).any(|w| w == b"refresh-1"));
    drop(store);

    let (ok, out) = run(&dir, &fake, &tokens, &["mirror", "sync"]);
    assert!(ok, "{out}");
    // The sync refreshed with the first token and sealed the new one.
    assert!(account(&dir).2.contains("refresh-2"));
    let (ok, out) = run(&dir, &fake, &tokens, &["mirror", "verify"]);
    assert!(ok && out.contains(" 0 differences"), "{out}");
    assert!(account(&dir).2.contains("refresh-3"));

    let calls = tokens.calls.lock().unwrap().clone();
    assert!(
        calls[0].contains("grant_type=authorization_code"),
        "{calls:?}"
    );
    assert!(calls[1].contains("refresh_token=refresh-1"), "{calls:?}");
    assert!(calls[2].contains("refresh_token=refresh-2"), "{calls:?}");
    let store = Store::open(&dir).unwrap();
    assert_eq!(
        count(&store, "SELECT count(*) FROM message") as usize,
        fake.messages().len()
    );
}

#[test]
fn an_outlook_sign_in_with_a_bad_client_id_starts_no_sign_in() {
    let fake = FakeGraph::start(Mailbox::new(1, 1)).unwrap();
    let tokens = token_server();
    let dir = data_dir("outlook-bad-id");
    let (ok, out) = run(
        &dir,
        &fake,
        &tokens,
        &["account", "add-outlook", "not-a-guid", "--paste"],
    );
    assert!(!ok && out.contains("GUID"), "{out}");
    assert!(tokens.calls.lock().unwrap().is_empty());
}

#[test]
fn a_refused_outlook_refresh_asks_for_a_new_sign_in() {
    let fake = FakeGraph::start(Mailbox::new(3, 1)).unwrap();
    let tokens = token_server();
    tokens.answers.lock().unwrap().extend([
        answer("refresh-1"),
        (400, r#"{"error":"invalid_grant"}"#.to_string()),
    ]);
    let dir = data_dir("outlook-revoked");
    let (ok, out) = add(&dir, &fake, &tokens);
    assert!(ok, "{out}");
    let (ok, out) = run(&dir, &fake, &tokens, &["mirror", "sync"]);
    assert!(!ok && out.contains("iris account add-outlook"), "{out}");
    let (_, state, plain) = account(&dir);
    assert_eq!(state, "needs_sign_in");
    // The refused token stays: a new sign-in replaces it.
    assert!(plain.contains("refresh-1"));
}

/// A message answer of the fake with this ID, etag and category list.
fn message_json(id: &str, etag: &str, categories: &str) -> String {
    format!(
        r#"{{"id":"{id}","@odata.etag":{etag:?},"conversationId":"c",
        "parentFolderId":"F-inbox","categories":[{categories}],"isRead":true,
        "flag":{{"flagStatus":"notFlagged"}},"isDraft":false,
        "receivedDateTime":"2026-11-17T10:00:00Z","internetMessageId":null}}"#
    )
}

/// When Graph keeps another category list than a write asks for (A6),
/// the engine stores the switch with the account (limit #675): a new
/// provider value, as after a restart, refuses each category write before
/// any request.
#[test]
fn the_category_switch_lasts_after_a_restart() {
    use std::sync::Arc;
    let fake = FakeGraph::start(Mailbox::new(3, 60)).unwrap();
    // The only test of this file that reads the setting in this process.
    std::env::set_var("IRIS_TEST_GRAPH_URL", fake.base_url());
    let dir = data_dir("outlook-switch");
    let store = Arc::new(Store::open(&dir).unwrap());
    let refresh = iris_auth::Secret::new("refresh-1");
    let account =
        iris::engine::save_outlook(&store, "owner@example.com", CLIENT_ID, "common", &refresh)
            .unwrap();
    let gmail = iris_gmail::Config::default();
    let off_at = || -> Option<i64> {
        store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT categories_off_at FROM account WHERE id = ?1",
                    [account],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    };
    let (a, b) = (FakeGraph::id_of(0), FakeGraph::id_of(1));
    let mail = iris::engine::provider_for(&store, account, &gmail).unwrap();
    mail.labels("t").unwrap();
    let etag = fake.etag_of(&a).unwrap();
    fake.answer_next(&[
        &message_json(&a, &etag, ""),
        &message_json(&a, &etag, r#""Other""#),
    ]);
    // The store refuses the first write of the switch, as a full disk does
    // (issue #717).
    let trigger = |sql: &str| store.write(|tx| Ok(tx.execute_batch(sql)?)).unwrap();
    trigger(
        "CREATE TRIGGER no_switch BEFORE UPDATE OF categories_off_at ON account
         BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
    );
    let e = mail
        .modify("t", &[&a], &["category:Blue"], &[])
        .unwrap_err();
    assert!(e.write_result_unknown(), "{e}");
    assert!(off_at().is_none(), "the store refused the switch");
    // Each later category write is refused, and stores the switch again.
    let n = fake.requests().len();
    assert!(mail.modify("t", &[&b], &["category:Blue"], &[]).is_err());
    assert!(off_at().is_none());
    trigger("DROP TRIGGER no_switch");
    let e = mail
        .modify("t", &[&b], &["category:Blue"], &[])
        .unwrap_err();
    assert!(!e.write_result_unknown(), "{e}");
    assert_eq!(fake.requests().len(), n, "a refusal makes no request");
    assert!(off_at().is_some(), "the switch is stored");
    drop(mail);

    let mail = iris::engine::provider_for(&store, account, &gmail).unwrap();
    let n = fake.requests().len();
    let e = mail
        .modify("t", &[&b], &["category:Blue"], &[])
        .unwrap_err();
    assert!(!e.write_result_unknown(), "{e}");
    assert_eq!(fake.requests().len(), n, "no request after a restart");
    mail.modify("t", &[&b], &["STARRED"], &[]).unwrap();
}
