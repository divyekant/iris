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
    // A sync error of the worker can come between the two lines.
    let mut line = String::new();
    while !line.contains("sign-in address") {
        line.clear();
        assert!(stderr.read_line(&mut line).unwrap() > 0, "no sign-in line");
    }
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

#[test]
fn a_watch_gives_one_notice_for_a_new_synced_mail() {
    let (store, fake) = synced("watch");
    let tokens = token_server();
    // The fake gives its new messages a date in 2026-01.
    let t = 1_767_225_600_000 - 60_000;
    let id = iris_api::add_watch(&store, "\"lantern festival\" from:ana", t).unwrap();
    let (ok, out, err) = iris(&store, &["watch", "add", "visa from:embassy"]);
    assert!(ok, "{err}");
    let other = out.split(':').next().unwrap().to_string();
    assert!(other.starts_with("w-") && other != id, "{out}");
    let (ok, _, err) = iris(&store, &["watch", "add", "form:email"]);
    assert!(!ok && err.contains("form:"), "{err}");

    let (_, out, _) = iris(&store, &["watch", "list"]);
    assert_eq!(out.matches("waits").count(), 2, "{out}");

    fake.add_message(
        b"From: Ana Lee <ana@example.com>\r\nTo: owner@example.com\r\n\
          Subject: Lantern festival tickets\r\nDate: Thu, 01 Jan 2026 00:00:00 +0000\r\n\
          Message-ID: <watch-1@example.com>\r\n\r\nThe lantern festival is on.\r\n",
        &["INBOX"],
    );
    Worker::new(store.clone(), gmail(fake.base_url()), 1, Some(tokens.uri))
        .sync()
        .unwrap();
    // No model is set: the free steps write the clean text.
    iris::cost::after_sync(&store, t).unwrap();
    let found = store.check_watches(t).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(store.check_watches(t).unwrap().is_empty(), "one notice");

    let lines: Vec<Value> = iris(&store, &["watch", "list", "--json"])
        .1
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        value(iris_api::watches(&store).unwrap()),
        Value::from(lines.clone())
    );
    let arrived = lines.iter().find(|w| w["id"] == id.as_str()).unwrap();
    assert_eq!(arrived["state"], "arrived");
    assert_eq!(arrived["message"]["subject"], "Lantern festival tickets");
    let (_, out, _) = iris(&store, &["watch", "list"]);
    assert!(
        out.contains("arrived") && out.contains("Lantern festival tickets"),
        "{out}"
    );

    assert!(iris(&store, &["watch", "remove", &id]).0);
    let (ok, _, err) = iris(&store, &["watch", "remove", &id]);
    assert!(!ok && err.contains("no watch"), "{err}");
    let (ok, _, err) = iris(&store, &["watch", "remove", "m-1"]);
    assert!(!ok && err.contains("not a watch ID"), "{err}");
}

/// `iris matters` and `iris matters show` print the JSON of the routes, and
/// `iris matters propose` with no model makes no call.
#[test]
fn matters_verbs_are_the_api_matters() {
    use rusqlite::params;
    let store = Store::open(&data_dir("matters")).unwrap();
    let a = store.add_account("gmail", "owner@example.com").unwrap();
    let msgs: Vec<iris_store::NewMessage> = (0..2)
        .map(|t| iris_store::NewMessage {
            provider_id: format!("g{t}"),
            thread_provider_id: format!("gt{t}"),
            ..Default::default()
        })
        .collect();
    let ids = store.put_messages(a, &msgs).unwrap();
    store
        .write(|tx| {
            for (i, form) in ["record", "commitment"].iter().enumerate() {
                tx.execute(
                    "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                     VALUES (?1, ?2, 'open', '{}', 0, 0)",
                    params![form, a],
                )?;
                tx.execute(
                    "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
                    params![tx.last_insert_rowid(), ids[i]],
                )?;
            }
            Ok(())
        })
        .unwrap();
    iris_matters::write(&store, |tx| {
        iris_matters::insert_proposal(tx, "Trip", "fake", None, &[1, 2], 1_000)
    })
    .unwrap();
    assert_eq!(
        json(&store, &["matters"]),
        iris_api::matters(&store).unwrap()
    );
    assert_eq!(
        json(&store, &["matters", "show", "g-1"]),
        iris_api::matter(&store, "g-1").unwrap().unwrap()
    );
    let (ok, _, err) = iris(&store, &["matters", "show", "g-9"]);
    assert!(!ok && err.contains("no matter g-9"), "{err}");
    let (ok, out, err) = iris(&store, &["matters", "propose"]);
    assert!(ok, "{err}");
    assert!(out.contains("no model is set"), "{out}");
}

/// Card R5-05: `iris job answer` answers the question of a job once.
#[test]
fn job_answer_answers_the_question_once() {
    let dir = std::env::temp_dir().join(format!("iris-verbs-answer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let job = iris_job::create(&store, "Pay the invoice.", None, 1).unwrap();
    let id = iris_job::format_job_id(job);
    let (ok, _, err) = iris(&store, &["job", "answer", &id, "The blue card."]);
    assert!(!ok && err.contains("does not ask this question"), "{err}");
    iris_job::ask(&store, job, "Which card?", None, 2).unwrap();
    let (ok, _, err) = iris(&store, &["job", "answer", "j-999", "Yes."]);
    assert!(!ok && err.contains("no job"), "{err}");
    let (ok, out, err) = iris(&store, &["job", "answer", &id, "The blue card."]);
    assert!(ok, "{err}");
    assert!(out.contains("answered"), "{out}");
    let last = iris_job::events(&store, job).unwrap().pop().unwrap();
    assert_eq!(last.answer.as_deref(), Some("The blue card."));
    let _ = std::fs::remove_dir_all(&dir);
}
