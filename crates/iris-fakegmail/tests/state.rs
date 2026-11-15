//! The state layer of the fake: label samples, changes, history records,
//! filters, the fields of each format, and injected failures.

use std::io::{Read, Write};
use std::net::TcpStream;

use iris_fakegmail::FakeGmail;
use iris_synth::Mailbox;
use serde_json::Value;

fn call(gmail: &FakeGmail, path: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(gmail.addr()).unwrap();
    write!(
        stream,
        "GET /gmail/v1/users/me/{path} HTTP/1.1\r\nHost: fake\r\nAuthorization: Bearer t\r\nConnection: close\r\n\r\n"
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
    let (status, body) = call(gmail, path);
    assert_eq!(status, 200, "{path}: {body}");
    body
}

fn ids(gmail: &FakeGmail, query: &str) -> Vec<String> {
    let page = get(gmail, &format!("messages?maxResults=500{query}"));
    assert!(page.get("nextPageToken").is_none());
    page["messages"]
        .as_array()
        .map(|a| a.iter().map(|m| m["id"].as_str().unwrap().into()).collect())
        .unwrap_or_default()
}

fn keys(v: &Value) -> Vec<&str> {
    let mut k: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    k.sort();
    k
}

fn labels_of(gmail: &FakeGmail, id: &str) -> Vec<String> {
    let m = get(gmail, &format!("messages/{id}?format=minimal"));
    m.get("labelIds")
        .map(|l| serde_json::from_value(l.clone()).unwrap())
        .unwrap_or_default()
}

#[test]
fn the_mailbox_has_spam_trash_draft_and_user_label_samples() {
    let gmail = FakeGmail::start(Mailbox::new(7, 400)).unwrap();
    let all = ids(&gmail, "&includeSpamTrash=true");
    assert_eq!(all.len(), 400);
    let visible = ids(&gmail, "");
    for label in [
        "SPAM", "TRASH", "DRAFT", "SENT", "INBOX", "Label_1", "Label_2",
    ] {
        let with = ids(&gmail, &format!("&includeSpamTrash=true&labelIds={label}"));
        assert!(!with.is_empty(), "no sample with {label}");
        for id in with.iter().take(3) {
            assert!(labels_of(&gmail, id).contains(&label.to_string()));
        }
        let hidden = label == "SPAM" || label == "TRASH";
        assert_eq!(
            with.iter().any(|id| visible.contains(id)),
            !hidden,
            "{label}"
        );
    }
    // Two labelIds: a message must have both.
    let both = ids(&gmail, "&labelIds=INBOX&labelIds=Label_2");
    let inbox = ids(&gmail, "&labelIds=INBOX");
    assert!(!both.is_empty() && both.len() < inbox.len());

    let labels = get(&gmail, "labels");
    let user: Vec<&str> = labels["labels"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["type"] == "user")
        .map(|l| l["name"].as_str().unwrap())
        .collect();
    assert_eq!(user, ["Receipts", "Work/Projects"]);
    let spam = get(&gmail, "labels/SPAM");
    let spam_count = ids(&gmail, "&includeSpamTrash=true&labelIds=SPAM").len();
    assert_eq!(spam["messagesTotal"], spam_count);
    assert_eq!(call(&gmail, "labels/Label_99").0, 404);
}

#[test]
fn each_format_has_only_the_documented_fields() {
    let gmail = FakeGmail::start(Mailbox::new(7, 20)).unwrap();
    let id = FakeGmail::id_of(5);
    assert_eq!(
        keys(&get(&gmail, &format!("messages/{id}?format=minimal"))),
        ["id", "labelIds"]
    );
    let meta = get(&gmail, &format!("messages/{id}?format=metadata"));
    assert_eq!(keys(&meta), ["id", "labelIds", "payload"]);
    assert_eq!(keys(&meta["payload"]), ["headers"]);
    assert_eq!(
        keys(&get(&gmail, &format!("messages/{id}?format=raw"))),
        [
            "historyId",
            "id",
            "internalDate",
            "labelIds",
            "raw",
            "sizeEstimate",
            "snippet",
            "threadId"
        ]
    );
}

#[test]
fn changes_make_history_records_and_the_filter_works() {
    let gmail = FakeGmail::start(Mailbox::new(7, 50)).unwrap();
    let start = get(&gmail, "profile")["historyId"]
        .as_str()
        .unwrap()
        .to_string();
    let a = FakeGmail::id_of(10);
    let b = FakeGmail::id_of(11);
    assert!(gmail.modify_labels(&a, &["STARRED", "Label_1"], &["UNREAD", "INBOX"]));
    assert!(gmail.delete_message(&b));
    assert!(!gmail.delete_message(&b));
    let c = gmail.add_message(b"Subject: New\r\n\r\nHi\r\n", &["INBOX", "UNREAD"]);

    let h = get(&gmail, &format!("history?startHistoryId={start}"));
    let records = h["history"].as_array().unwrap();
    let kinds: Vec<&str> = records
        .iter()
        .map(|r| {
            let o = r.as_object().unwrap();
            [
                "messagesAdded",
                "messagesDeleted",
                "labelsAdded",
                "labelsRemoved",
            ]
            .into_iter()
            .find(|k| o.contains_key(*k))
            .unwrap()
        })
        .collect();
    // Message 10 can have no INBOX and no UNREAD, so a labelsRemoved record is optional.
    assert_eq!(kinds.first(), Some(&"labelsAdded"));
    assert_eq!(
        kinds[kinds.len() - 2..],
        ["messagesDeleted", "messagesAdded"]
    );
    assert_eq!(
        records.last().unwrap()["messagesAdded"][0]["message"]["id"],
        c
    );
    assert_eq!(h["historyId"], records.last().unwrap()["id"]);

    let only = get(
        &gmail,
        &format!("history?startHistoryId={start}&historyTypes=messageDeleted"),
    );
    let only = only["history"].as_array().unwrap();
    assert_eq!(only.len(), 1);
    assert_eq!(only[0]["messagesDeleted"][0]["message"]["id"], b);
    assert_eq!(
        call(
            &gmail,
            &format!("history?startHistoryId={start}&historyTypes=bad")
        )
        .0,
        400
    );

    assert_eq!(call(&gmail, &format!("messages/{b}?format=minimal")).0, 404);
    assert!(!ids(&gmail, "&includeSpamTrash=true").contains(&b));
    let labels = labels_of(&gmail, &a);
    assert!(labels.contains(&"STARRED".into()) && !labels.contains(&"UNREAD".into()));
    assert_eq!(get(&gmail, "profile")["messagesTotal"], 50);
}

#[test]
fn label_rename_and_delete_change_the_list_and_the_messages() {
    let gmail = FakeGmail::start(Mailbox::new(7, 50)).unwrap();
    let start = get(&gmail, "profile")["historyId"].clone();
    let id = gmail.create_label("Later");
    assert_eq!(id, "Label_3");
    let m = FakeGmail::id_of(3);
    gmail.modify_labels(&m, &[&id], &[]);
    assert!(gmail.rename_label(&id, "Soon"));
    assert!(!gmail.rename_label("INBOX", "X"));
    assert_eq!(get(&gmail, &format!("labels/{id}"))["name"], "Soon");
    assert!(gmail.delete_label(&id));
    assert!(!gmail.delete_label(&id));
    assert!(!labels_of(&gmail, &m).contains(&id));
    let names = get(&gmail, "labels");
    assert!(!names["labels"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["id"] == id));
    // Only the label change of the message made a record.
    let start = start.as_str().unwrap();
    let h = get(&gmail, &format!("history?startHistoryId={start}"));
    assert_eq!(h["history"].as_array().unwrap().len(), 1);
}

#[test]
fn injected_failures_come_first_and_each_request_is_logged() {
    let gmail = FakeGmail::start(Mailbox::new(7, 5)).unwrap();
    gmail.fail_next(&[429, 503, 403]);
    let (s1, b1) = call(&gmail, "profile");
    assert_eq!(s1, 429);
    assert_eq!(b1["error"]["errors"][0]["reason"], "rateLimitExceeded");
    assert_eq!(call(&gmail, "profile").0, 503);
    let (s3, b3) = call(&gmail, "profile");
    assert_eq!(s3, 403);
    assert_eq!(b3["error"]["errors"][0]["reason"], "rateLimitExceeded");
    assert_eq!(call(&gmail, "profile").0, 200);
    let log = gmail.requests();
    assert_eq!(log.len(), 4);
    assert_eq!(log[0], "GET /gmail/v1/users/me/profile");
}
