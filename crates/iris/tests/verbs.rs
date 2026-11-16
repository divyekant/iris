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
}

#[test]
fn search_json_is_the_api_search() {
    let (store, _fake) = synced("search");
    let api = iris_api::search(&store, "label:INBOX", 50).unwrap();
    assert!(!api.is_empty());
    let verb = json(&store, &["search", "label:INBOX", "--json"]);
    assert_eq!(verb, serde_json::json!({ "messages": value(&api) }));
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
    let sign_in = line
        .trim()
        .rsplit(addr.as_str())
        .next()
        .unwrap()
        .to_string();
    assert!(sign_in.starts_with("/api/session?code="), "{line}");
    let get = |path: &str, cookie: &str| {
        let mut stream = std::net::TcpStream::connect(&addr).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {addr}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    };
    // Without the cookie the API answers 401, not the page of the web client.
    let refused = get("/api/status", "");
    let signed = get(&sign_in, "");
    let cookie = signed
        .lines()
        .find_map(|l| {
            l.strip_prefix("set-cookie: ")
                .or_else(|| l.strip_prefix("Set-Cookie: "))
        })
        .map(|c| c.split(';').next().unwrap().to_string())
        .unwrap_or_default();
    let response = get("/api/status", &cookie);
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(refused.starts_with("HTTP/1.1 401"), "{refused}");
    assert!(response.starts_with("HTTP/1.1 200"), "{signed}\n{response}");
    assert!(response.contains("owner@example.com"), "{response}");
}
