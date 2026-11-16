//! The commands of cost control (tracker items R1-15): `iris model` and
//! `iris understand`, with a fake local model server. No key, no network.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
use iris::engine::Worker;
use iris_fakegmail::FakeGmail;
use iris_store::Store;
use iris_synth::Mailbox;

const KEY: &str = "test-key-not-real-4242";

/// A data folder with a synced synthetic mailbox.
fn synced(name: &str) -> (Arc<Store>, FakeGmail) {
    let fake = FakeGmail::start(Mailbox::new(3, 12)).unwrap();
    let tokens = token_server();
    let store = Arc::new(Store::open(&data_dir(name)).unwrap());
    add_account(&store, &gmail(fake.base_url()));
    Worker::new(store.clone(), gmail(fake.base_url()), 1, Some(tokens.uri))
        .sync()
        .unwrap();
    (store, fake)
}

/// Runs `iris` with `stdin` as standard input (not a terminal).
fn iris_in(store: &Store, args: &[&str], stdin: &str) -> (bool, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(args)
        .arg("--data")
        .arg(store.dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The command can exit before it reads standard input (a refused
    // argument), so a broken pipe here is not an error.
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    let out = child.wait_with_output().unwrap();
    (
        out.status.success(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn iris(store: &Store, args: &[&str]) -> (bool, String, String) {
    iris_in(store, args, "")
}

/// A local OpenAI-compatible server: each request gets a valid answer with
/// no item. Gives the base URL and the count of requests.
fn model_server() -> (String, Arc<AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut len = 0;
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            let _ = r.read_exact(&mut body);
            c.fetch_add(1, Ordering::SeqCst);
            let answer = r#"{"choices":[{"index":0,"message":{"role":"assistant","content":"{\"items\": [], \"phishing\": false}"},"finish_reason":"stop"}],"usage":{"prompt_tokens":50,"completion_tokens":5}}"#;
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    (url, count)
}

fn col(store: &Store, sql: &str) -> Option<i64> {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

#[test]
fn a_local_model_runs_and_a_second_run_makes_zero_calls() {
    let (store, _fake) = synced("local-run");
    let (url, calls) = model_server();
    let (ok, out, err) = iris(&store, &["model", "set", "local", &url, "local-1"]);
    assert!(ok, "{err}");
    assert!(out.contains("version 1"), "{out}");
    let (ok, out, err) = iris(&store, &["understand"]);
    assert!(ok, "{err}");
    assert!(out.contains("Stop: done"), "{out}");
    let first = calls.load(Ordering::SeqCst);
    assert!(first > 0);
    let (ok, out, _) = iris(&store, &["understand"]);
    assert!(ok);
    assert!(out.contains(" 0 calls"), "{out}");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        first,
        "the second run is free"
    );
}

#[test]
fn a_remote_local_model_is_refused() {
    let (store, _fake) = synced("remote");
    for url in [
        "https://remote.example/v1",
        "http://127.0.0.1@remote.example/v1",
        "http://10.1.2.3:8080/v1",
    ] {
        let (ok, _, err) = iris(&store, &["model", "set", "local", url, "m"]);
        assert!(!ok, "{url}");
        assert!(err.contains("not on this machine"), "{err}");
    }
    assert_eq!(col(&store, "SELECT count(*) FROM model_setting"), Some(0));
}

#[test]
fn the_key_comes_from_stdin_and_is_never_shown() {
    let (store, _fake) = synced("key");
    let args = [
        "model",
        "set",
        "anthropic",
        "model-x",
        "--price-in",
        "3",
        "--price-out",
        "15",
    ];
    let (ok, out, err) = iris_in(&store, &args, &format!("{KEY}\n"));
    assert!(ok, "{err}");
    assert!(!out.contains(KEY) && !err.contains(KEY));
    let (ok, out, err) = iris(&store, &["model", "show"]);
    assert!(ok, "{err}");
    assert!(out.contains("Key: set"), "{out}");
    assert!(out.contains("3 USD in, 15 USD out"), "{out}");
    assert!(out.contains("Budget: none approved"), "{out}");
    assert!(!out.contains(KEY) && !err.contains(KEY));
    // The stored key is sealed.
    let sealed: Vec<u8> = store
        .read(|c| Ok(c.query_row("SELECT api_key FROM model_setting", [], |r| r.get(0))?))
        .unwrap();
    assert!(!String::from_utf8_lossy(&sealed).contains(KEY));
    assert_eq!(store.unseal(&sealed).unwrap(), KEY.as_bytes());
    // A run with no budget makes no call, and its errors hold no key.
    let (ok, out, err) = iris(&store, &["understand"]);
    assert!(ok, "{err}");
    assert!(
        out.contains(" 0 calls") && out.contains("Stop: budget"),
        "{out}"
    );
    assert!(!out.contains(KEY) && !err.contains(KEY));
}

#[test]
fn bad_prices_are_refused_before_any_write() {
    let (store, _fake) = synced("prices");
    for (p_in, p_out) in [
        ("-1", "1"),
        ("1e309", "1"),
        ("1", "1001"),
        ("1.1234567", "1"),
    ] {
        let args = [
            "model",
            "set",
            "anthropic",
            "m",
            "--price-in",
            p_in,
            "--price-out",
            p_out,
        ];
        let (ok, _, _) = iris_in(&store, &args, &format!("{KEY}\n"));
        assert!(!ok, "{p_in} {p_out}");
    }
    assert_eq!(col(&store, "SELECT count(*) FROM model_setting"), Some(0));
}

#[test]
fn the_estimate_makes_no_call_and_writes_a_receipt() {
    let (store, _fake) = synced("estimate");
    let set = [
        "model",
        "set",
        "anthropic",
        "m",
        "--price-in",
        "1",
        "--price-out",
        "5",
    ];
    assert!(iris_in(&store, &set, &format!("{KEY}\n")).0);
    let (ok, out, err) = iris(&store, &["understand", "--estimate"]);
    assert!(ok, "{err}");
    assert!(out.contains("Messages to understand: "), "{out}");
    assert!(out.contains("Cost: about "), "{out}");
    let code = out
        .split("--approve ")
        .nth(1)
        .and_then(|r| r.split_whitespace().next())
        .unwrap()
        .to_string();
    assert_eq!(code.len(), 32);
    assert_eq!(
        col(&store, "SELECT count(*) FROM understand_estimate"),
        Some(1)
    );
    assert_eq!(col(&store, "SELECT count(*) FROM understand_run"), Some(0));
    // With no terminal, the approval is refused.
    let (ok, _, err) = iris_in(
        &store,
        &["understand", "--approve", &code, "--budget", "5"],
        "5\n",
    );
    assert!(!ok);
    assert!(err.contains("needs a terminal"), "{err}");
    assert_eq!(
        col(&store, "SELECT budget_micro_usd FROM model_setting"),
        None
    );
}

#[test]
fn a_code_approves_one_time_for_its_model_version_only() {
    let (store, _fake) = synced("approve");
    let now = iris::engine::now_ms();
    let local = |url: &str| iris::cost::NewModel {
        local: false,
        base_url: url.into(),
        model: "m".into(),
        price: Some((1_000_000, 1_000_000)),
        api_key: Some(KEY.into()),
        max_tokens: 100,
        timeout_s: 10,
    };
    iris::cost::set_model(&store, &local("https://api.anthropic.com")).unwrap();
    let code = |store: &Store| {
        let out = iris::cost::estimate(store, now).unwrap();
        out.split("--approve ")
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
            .unwrap()
            .to_string()
    };
    assert!(
        iris::cost::approve(&store, "0000", 5, now).is_err(),
        "unknown code"
    );
    let c = code(&store);
    assert!(
        iris::cost::approve(&store, &c, 5, now + iris::cost::RECEIPT_MS + 1).is_err(),
        "old code"
    );
    iris::cost::approve(&store, &c, 5_000_000, now).unwrap();
    assert_eq!(
        col(&store, "SELECT budget_micro_usd FROM model_setting"),
        Some(5_000_000)
    );
    assert!(iris::cost::approve(&store, &c, 9, now).is_err(), "one time");
    // A new model version refuses the code of the old one, and clears the budget.
    let c = code(&store);
    iris::cost::set_model(&store, &local("https://api.anthropic.com")).unwrap();
    assert_eq!(
        col(&store, "SELECT budget_micro_usd FROM model_setting"),
        None
    );
    assert!(
        iris::cost::approve(&store, &c, 5, now).is_err(),
        "other version"
    );
}

#[test]
fn model_set_fails_while_a_run_holds_the_lock() {
    let (store, _fake) = synced("lock");
    let held = iris_understand::batch::lock(&store).unwrap();
    let (ok, _, err) = iris(
        &store,
        &["model", "set", "local", "http://127.0.0.1:9/v1", "m"],
    );
    assert!(!ok);
    assert!(err.contains("a run is in progress"), "{err}");
    let (ok, _, err) = iris(&store, &["understand"]);
    assert!(!ok);
    assert!(err.contains("a run is in progress"), "{err}");
    drop(held);
    assert!(
        iris(
            &store,
            &["model", "set", "local", "http://127.0.0.1:9/v1", "m"]
        )
        .0
    );
}
