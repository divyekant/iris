//! The read verbs give the same data as the local API (tracker item R1-23).

mod common;

use std::process::Command;
use std::sync::Arc;

use common::*;
use iris::engine::Worker;
use iris_fakegmail::FakeGmail;
use iris_store::Store;
use iris_synth::Mailbox;
use serde_json::Value;

/// A data folder with a synced synthetic mailbox.
fn synced(name: &str) -> (Arc<Store>, FakeGmail) {
    let fake = FakeGmail::start(Mailbox::new(3, 40)).unwrap();
    let tokens = token_server();
    let store = Arc::new(Store::open(&data_dir(name)).unwrap());
    add_account(&store, &gmail(fake.base_url()));
    Worker::new(store.clone(), gmail(fake.base_url()), 1, Some(tokens.uri))
        .sync()
        .unwrap();
    (store, fake)
}

fn iris(store: &Store, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(args)
        .arg("--data")
        .arg(store.dir())
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn json(store: &Store, args: &[&str]) -> Value {
    let (ok, out, err) = iris(store, args);
    assert!(ok, "{err}");
    serde_json::from_str(&out).unwrap()
}

fn value(v: impl serde::Serialize) -> Value {
    serde_json::to_value(v).unwrap()
}

#[test]
fn status_json_is_the_api_status() {
    let (store, _fake) = synced("status");
    let api = value(iris_api::status(&store).unwrap());
    assert_eq!(json(&store, &["status", "--json"]), api);
    assert_eq!(api["messages"]["fetched"], 40);
    let (ok, out, _) = iris(&store, &["status"]);
    assert!(ok);
    assert!(out.contains("owner@example.com"), "{out}");
    assert!(out.contains("Messages: 40"), "{out}");
    assert!(out.contains("Mode: observe"), "{out}");
}

#[test]
fn search_json_is_the_api_search() {
    let (store, _fake) = synced("search");
    let api = iris_api::search(&store, "label:INBOX", 50).unwrap();
    assert!(!api.is_empty());
    // A list is JSON Lines (specification 2, section 8.3): one summary each line.
    let (ok, out, err) = iris(&store, &["search", "label:INBOX", "--json"]);
    assert!(ok, "{err}");
    let lines: Vec<Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines, api.iter().map(value).collect::<Vec<_>>());
    let (ok, out, _) = iris(&store, &["search", "label:INBOX", "--limit", "3"]);
    assert!(ok);
    assert_eq!(out.lines().count(), 3, "{out}");
    assert!(out.starts_with(&api[0].id), "{out}");
}

#[test]
fn a_bad_search_text_is_an_error() {
    let (store, _fake) = synced("bad-search");
    let (ok, _, err) = iris(&store, &["search", "after:not-a-date"]);
    assert!(!ok);
    assert!(err.starts_with("iris: "), "{err}");
}

#[test]
fn show_json_is_the_api_message() {
    let (store, _fake) = synced("show");
    let id = iris_api::search(&store, "label:INBOX", 1).unwrap()[0]
        .id
        .clone();
    let api = value(iris_api::message(&store, &id).unwrap().unwrap());
    assert_eq!(json(&store, &["show", &id, "--json"]), api);
    let (ok, out, _) = iris(&store, &["show", &id]);
    assert!(ok);
    assert!(out.starts_with(&format!("ID:      {id}")), "{out}");
}

#[test]
fn show_of_an_unknown_id_fails() {
    let (store, _fake) = synced("show-missing");
    for id in ["m-999999", "c-999999"] {
        let (ok, _, err) = iris(&store, &["show", id]);
        assert!(!ok, "{id}");
        assert!(err.contains("no object has the ID"), "{err}");
    }
}

#[test]
fn serve_answers_the_api_before_the_web_client() {
    use std::io::{BufRead, BufReader, Read, Write};
    let (store, _fake) = synced("serve-api");
    let mut child = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(["serve", "--addr", "127.0.0.1:0", "--data"])
        .arg(store.dir())
        // No token endpoint listens here: the sync fails, and no request leaves the machine.
        .env("IRIS_TEST_TOKEN_URI", "http://127.0.0.1:9/token")
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    while !line.contains("serving on") {
        line.clear();
        assert!(stderr.read_line(&mut line).unwrap() > 0, "no address line");
    }
    let addr = line.trim().rsplit("http://").next().unwrap().to_string();
    let mut line = String::new();
    stderr.read_line(&mut line).unwrap();
    let code = line.trim().rsplit("#code=").next().unwrap().to_string();
    assert!(line.contains(&format!("http://{addr}/#code=")), "{line}");
    let send = |request: String| {
        let mut stream = std::net::TcpStream::connect(&addr).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    };
    let get = |auth: &str| {
        send(format!(
            "GET /api/status HTTP/1.1\r\nHost: {addr}\r\n{auth}Connection: close\r\n\r\n"
        ))
    };
    // Without the token the API answers 401, not the page of the web client.
    let refused = get("");
    let body = format!("{{\"code\":\"{code}\"}}");
    let session = send(format!(
        "POST /api/session HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    let token = session
        .split("\"token\":\"")
        .nth(1)
        .and_then(|t| t.split('"').next())
        .unwrap_or_default()
        .to_string();
    let response = get(&format!("Authorization: Bearer {token}\r\n"));
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(refused.starts_with("HTTP/1.1 401"), "{refused}");
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "{session}\n{response}"
    );
    assert!(response.contains("owner@example.com"), "{response}");
}

#[test]
fn serve_refuses_an_address_that_is_not_loopback() {
    let dir = data_dir("serve-open");
    let out = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(["serve", "--addr", "0.0.0.0:0", "--data"])
        .arg(&dir)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not a loopback address"), "{err}");
    assert!(!dir.exists(), "the engine opened the store");
}
