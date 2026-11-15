//! A small HTTP client lists and gets every message of a small mailbox from
//! the fake server, on 127.0.0.1 only.

use std::io::{Read, Write};
use std::net::TcpStream;

use iris_fakegmail::FakeGmail;
use iris_synth::{base64, Mailbox};
use serde_json::{json, Value};

fn call(
    gmail: &FakeGmail,
    method: &str,
    path: &str,
    body: Option<&Value>,
    token: bool,
) -> (u16, Value) {
    let mut stream = TcpStream::connect(gmail.addr()).unwrap();
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let auth = if token {
        "Authorization: Bearer test-token\r\n"
    } else {
        ""
    };
    write!(
        stream,
        "{method} /gmail/v1/users/me/{path} HTTP/1.1\r\nHost: fake\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let status = String::from_utf8_lossy(&response[9..12]).parse().unwrap();
    (
        status,
        serde_json::from_slice(&response[split + 4..]).unwrap(),
    )
}

fn get(gmail: &FakeGmail, path: &str) -> Value {
    let (status, body) = call(gmail, "GET", path, None, true);
    assert_eq!(status, 200, "{path}: {body}");
    body
}

#[test]
fn client_lists_and_gets_every_message() {
    let mailbox = Mailbox::new(9, 230);
    let expected: Vec<Vec<u8>> = (0..mailbox.len()).map(|i| mailbox.raw(i)).collect();
    let gmail = FakeGmail::start(mailbox).unwrap();

    let profile = get(&gmail, "profile");
    assert_eq!(profile["emailAddress"], "owner@example.com");
    assert_eq!(profile["messagesTotal"], 230);

    let mut ids = Vec::new();
    let mut token = String::new();
    loop {
        let page = get(&gmail, &format!("messages?maxResults=50{token}"));
        for m in page["messages"].as_array().unwrap() {
            ids.push(m["id"].as_str().unwrap().to_string());
        }
        match page["nextPageToken"].as_str() {
            Some(t) => token = format!("&pageToken={t}"),
            None => break,
        }
    }
    assert_eq!(ids.len(), 230);

    let mut got: Vec<Vec<u8>> = ids
        .iter()
        .map(|id| {
            let m = get(&gmail, &format!("messages/{id}?format=raw"));
            assert_eq!(m["id"], id.as_str());
            assert!(m["threadId"].is_string() && m["labelIds"].is_array());
            base64::decode(m["raw"].as_str().unwrap()).unwrap()
        })
        .collect();
    // The list is newest first; the mailbox is oldest first.
    got.reverse();
    assert_eq!(got, expected);
}

#[test]
fn formats_labels_history_send_and_errors() {
    let gmail = FakeGmail::start(Mailbox::new(4, 40)).unwrap();
    let id = get(&gmail, "messages?maxResults=1")["messages"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let meta = get(
        &gmail,
        &format!("messages/{id}?format=metadata&metadataHeaders=Subject&metadataHeaders=From"),
    );
    let names: Vec<&str> = meta["payload"]["headers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["From", "Subject"]);
    assert!(meta.get("raw").is_none());
    let minimal = get(&gmail, &format!("messages/{id}?format=minimal"));
    assert!(minimal.get("payload").is_none() && minimal.get("raw").is_none());
    assert_eq!(
        call(&gmail, "GET", &format!("messages/{id}"), None, true).0,
        400
    );
    assert_eq!(
        call(
            &gmail,
            "GET",
            "messages/ffffffffffffffff?format=raw",
            None,
            true
        )
        .0,
        404
    );
    assert_eq!(call(&gmail, "GET", "profile", None, false).0, 401);

    let labels = get(&gmail, "labels");
    assert!(labels["labels"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["id"] == "INBOX"));

    let start: u64 = get(&gmail, "profile")["historyId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let none = get(&gmail, &format!("history?startHistoryId={start}"));
    assert!(none.get("history").is_none());

    let raw = b"From: owner@example.com\r\nTo: a@example.org\r\nSubject: Hi\r\n\r\nHello\r\n";
    let (status, sent) = call(
        &gmail,
        "POST",
        "messages/send",
        Some(&json!({ "raw": base64::encode_url(raw) })),
        true,
    );
    assert_eq!(status, 200, "{sent}");
    assert_eq!(sent["labelIds"], json!(["SENT"]));

    let changes = get(&gmail, &format!("history?startHistoryId={start}"));
    let added = &changes["history"][0]["messagesAdded"][0]["message"];
    assert_eq!(added["id"], sent["id"]);
    assert_eq!(
        changes["historyId"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        start + 1
    );
    let back = get(
        &gmail,
        &format!("messages/{}?format=raw", sent["id"].as_str().unwrap()),
    );
    assert_eq!(base64::decode(back["raw"].as_str().unwrap()).unwrap(), raw);
    assert_eq!(get(&gmail, "profile")["messagesTotal"], 41);

    gmail.expire_history_before(start);
    assert_eq!(
        call(
            &gmail,
            "GET",
            &format!("history?startHistoryId={}", start - 1),
            None,
            true
        )
        .0,
        404
    );
    assert_eq!(
        call(
            &gmail,
            "POST",
            "messages/send",
            Some(&json!({ "raw": "***" })),
            true
        )
        .0,
        400
    );
}

#[test]
fn search_and_label_filters_are_refused() {
    let gmail = FakeGmail::start(Mailbox::new(4, 10)).unwrap();
    assert_eq!(call(&gmail, "GET", "messages?q=from:a", None, true).0, 400);
    assert_eq!(
        call(&gmail, "GET", "messages?labelIds=INBOX", None, true).0,
        400
    );
}

#[test]
fn history_sync_starts_from_the_profile_id_of_an_empty_mailbox() {
    let gmail = FakeGmail::start(Mailbox::new(4, 0)).unwrap();
    let start = get(&gmail, "profile")["historyId"]
        .as_str()
        .unwrap()
        .to_string();
    let changes = get(&gmail, &format!("history?startHistoryId={start}"));
    assert!(changes.get("history").is_none());
}
