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
        let page = get(
            &gmail,
            &format!("messages?maxResults=50&includeSpamTrash=true{token}"),
        );
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
fn search_is_refused() {
    let gmail = FakeGmail::start(Mailbox::new(4, 10)).unwrap();
    assert_eq!(call(&gmail, "GET", "messages?q=from:a", None, true).0, 400);
    assert_eq!(
        call(&gmail, "GET", "messages?q=rfc822msgid:", None, true).0,
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

#[test]
fn send_to_an_old_thread_keeps_the_thread_count_and_big_tokens_are_safe() {
    let gmail = FakeGmail::start(Mailbox::new(4, 10)).unwrap();
    let threads = get(&gmail, "profile")["threadsTotal"].clone();
    let thread = get(&gmail, "messages?maxResults=1")["messages"][0]["threadId"].clone();
    let raw = base64::encode_url(b"Subject: Re\r\n\r\nOK\r\n");
    let (status, _) = call(
        &gmail,
        "POST",
        "messages/send",
        Some(&json!({ "raw": raw, "threadId": thread })),
        true,
    );
    assert_eq!(status, 200);
    assert_eq!(get(&gmail, "profile")["threadsTotal"], threads);

    let max = usize::MAX;
    let page = get(&gmail, &format!("messages?pageToken={max}"));
    assert!(page.get("messages").is_none() && page.get("nextPageToken").is_none());
    let start = get(&gmail, "profile")["historyId"]
        .as_str()
        .unwrap()
        .to_string();
    let history = get(
        &gmail,
        &format!("history?startHistoryId={start}&pageToken={max}"),
    );
    assert!(history.get("nextPageToken").is_none());
}

#[test]
fn query_values_are_url_decoded_and_bad_padding_is_refused() {
    let gmail = FakeGmail::start(Mailbox::new(4, 5)).unwrap();
    let id = get(&gmail, "messages?maxResults=1")["messages"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(get(&gmail, &format!("messages/{id}?format=%72aw"))["raw"].is_string());
    let meta = get(
        &gmail,
        &format!("messages/{id}?format=metadata&metadataHeaders=Message%2DID"),
    );
    assert_eq!(meta["payload"]["headers"][0]["name"], "Message-ID");
    let (status, _) = call(
        &gmail,
        "POST",
        "messages/send",
        Some(&json!({ "raw": "AA=A" })),
        true,
    );
    assert_eq!(status, 400);
}

#[test]
fn a_small_mailbox_has_an_unread_message() {
    let gmail = FakeGmail::start(Mailbox::new(4, 40)).unwrap();
    let ids = get(&gmail, "messages?maxResults=40");
    let unread = ids["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| {
            let full = get(
                &gmail,
                &format!("messages/{}?format=minimal", m["id"].as_str().unwrap()),
            );
            full["labelIds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l == "UNREAD")
        })
        .count();
    assert_eq!(unread, 1);
}

/// Percent-encode each byte that is not unreserved, as a client does.
fn encode_query(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The `q` parser of the fake: for any text, the fake either refuses the
/// search (HTTP 400) or accepts exactly `rfc822msgid:<id>` with a
/// non-empty ID and no white space, and then gives only the drafts and
/// messages of that Message-ID.
#[test]
fn a_search_gives_only_its_message_id_or_is_refused() {
    let gmail = FakeGmail::start(Mailbox::new(3, 5)).unwrap();
    let raw = |id: &str| {
        base64::encode_url(
            format!("From: owner@example.com\r\nMessage-ID: <{id}>\r\nSubject: s\r\n\r\nb\r\n")
                .as_bytes(),
        )
    };
    for id in ["iris.1@x.org", "iris.2@x.org"] {
        let (status, _) = call(
            &gmail,
            "POST",
            "drafts",
            Some(&json!({"message": {"raw": raw(id)}})),
            true,
        );
        assert_eq!(status, 200);
    }
    const POOL: &[&str] = &[
        "rfc822msgid:",
        "rfc822msgid",
        "from:",
        "iris.1@x.org",
        "<iris.1@x.org>",
        "iris.2@x.org",
        "IRIS.2@X.ORG",
        " ",
        "\t",
        "<",
        ">",
        "&",
        "=",
        "%",
        "+",
        "x",
        "in:sent",
        "\u{e9}",
    ];
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..400 {
        let n = 1 + (next() % 4) as usize;
        let q: String = (0..n)
            .map(|_| POOL[(next() % POOL.len() as u64) as usize])
            .collect();
        let want = q
            .trim()
            .strip_prefix("rfc822msgid:")
            .filter(|id| !id.trim().is_empty() && !id.contains(char::is_whitespace));
        for path in ["drafts", "messages"] {
            let (status, body) = call(
                &gmail,
                "GET",
                &format!("{path}?q={}", encode_query(&q)),
                None,
                true,
            );
            let Some(id) = want else {
                assert_eq!(status, 400, "{q:?} {body}");
                continue;
            };
            assert_eq!(status, 200, "{q:?} {body}");
            let bare = id.trim_matches(['<', '>']).to_ascii_lowercase();
            let key = if path == "drafts" {
                "drafts"
            } else {
                "messages"
            };
            let found = body[key].as_array().map_or(0, Vec::len);
            let expected = usize::from(bare == "iris.1@x.org" || bare == "iris.2@x.org");
            assert_eq!(found, expected, "{q:?} {body}");
        }
    }
}
