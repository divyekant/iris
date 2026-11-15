//! Each endpoint of the client against the fake Gmail server. No network.

use std::time::{Duration, Instant};

use iris_fakegmail::FakeGmail;
use iris_gmail::{Change, Config, Error, Gmail, HistoryType};
use iris_synth::Mailbox;

const TOKEN: &str = "test-token";

fn client(fake: &FakeGmail) -> Gmail {
    Gmail::new(Config {
        base_url: fake.base_url(),
        backoff_start: Duration::from_millis(1),
        backoff_max: Duration::from_millis(4),
        ..Config::default()
    })
}

#[test]
fn profile_and_labels() {
    let fake = FakeGmail::start(Mailbox::new(3, 120)).unwrap();
    let gmail = client(&fake);
    let p = gmail.profile(TOKEN).unwrap();
    assert_eq!(p.email_address, "owner@example.com");
    assert_eq!(p.messages_total, 120);
    assert!(p.history_id > 0);

    let labels = gmail.labels(TOKEN).unwrap();
    assert!(labels.iter().any(|l| l.id == "SPAM" && l.kind == "system"));
    assert!(labels
        .iter()
        .any(|l| l.name == "Work/Projects" && l.kind == "user"));
    let inbox = gmail.label(TOKEN, "INBOX").unwrap();
    assert!(inbox.messages_total.unwrap() > 0);
    assert!(matches!(
        gmail.label(TOKEN, "Label_77"),
        Err(Error::NotFound)
    ));
}

#[test]
fn list_follows_pages_and_filters() {
    let fake = FakeGmail::start(Mailbox::new(3, 1_100)).unwrap();
    let gmail = client(&fake);
    let mut all = Vec::new();
    let mut token: Option<String> = None;
    let mut pages = 0;
    loop {
        let page = gmail.messages(TOKEN, &[], true, token.as_deref()).unwrap();
        pages += 1;
        all.extend(page.messages);
        match page.next_page_token {
            Some(t) => token = Some(t),
            None => break,
        }
    }
    assert_eq!((pages, all.len()), (3, 1_100));
    assert!(all.iter().all(|m| !m.thread_id.is_empty()));

    let visible = gmail.messages(TOKEN, &["INBOX"], false, None).unwrap();
    let spam = gmail.messages(TOKEN, &["SPAM"], true, None).unwrap();
    assert!(!visible.messages.is_empty() && !spam.messages.is_empty());
    assert!(spam.messages.iter().all(|m| !visible.messages.contains(m)));
    assert!(fake
        .requests()
        .iter()
        .any(|r| r.contains("labelIds=SPAM") && r.contains("includeSpamTrash=true")));
}

#[test]
fn get_in_each_format() {
    let mailbox = Mailbox::new(3, 30);
    let expected = mailbox.raw(7);
    let fake = FakeGmail::start(mailbox).unwrap();
    let gmail = client(&fake);
    let id = FakeGmail::id_of(7);

    let mut sink = Vec::new();
    let raw = gmail.raw(TOKEN, &id, &mut sink).unwrap();
    assert_eq!(sink, expected);
    assert_eq!(raw.size, expected.len() as u64);
    assert_eq!(raw.id, id);
    assert!(!raw.thread_id.is_empty() && !raw.label_ids.is_empty());
    assert!(raw.history_id > 0 && raw.internal_date > 1_700_000_000_000);

    let minimal = gmail.minimal(TOKEN, &id).unwrap();
    assert_eq!(minimal.label_ids, raw.label_ids);

    let meta = gmail.metadata(TOKEN, &id, &["Subject", "From"]).unwrap();
    let names: Vec<&str> = meta.headers.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(names, ["From", "Subject"]);
    assert!(!gmail.metadata(TOKEN, &id, &[]).unwrap().headers.is_empty());

    fake.delete_message(&id);
    assert!(matches!(
        gmail.raw(TOKEN, &id, &mut Vec::new()),
        Err(Error::NotFound)
    ));
    assert!(matches!(gmail.minimal(TOKEN, &id), Err(Error::NotFound)));
}

#[test]
fn history_changes_types_and_expiry() {
    let fake = FakeGmail::start(Mailbox::new(3, 40)).unwrap();
    let gmail = client(&fake);
    let start = gmail.profile(TOKEN).unwrap().history_id;
    let a = FakeGmail::id_of(4);
    fake.modify_labels(&a, &["STARRED"], &[]);
    fake.delete_message(&FakeGmail::id_of(5));
    let new = fake.add_message(b"Subject: x\r\n\r\ny\r\n", &["INBOX"]);

    let page = gmail
        .history(TOKEN, start, &HistoryType::ALL, None)
        .unwrap();
    let changes: Vec<&Change> = page.history.iter().flat_map(|h| &h.changes).collect();
    assert!(matches!(changes[0], Change::LabelsAdded(m, l) if m.id == a && l == &["STARRED"]));
    assert!(matches!(changes[1], Change::Deleted(_)));
    assert!(matches!(changes[2], Change::Added(m) if m.id == new && m.label_ids == ["INBOX"]));
    assert_eq!(page.history_id, page.history.last().unwrap().id);
    assert!(page.next_page_token.is_none());

    let only = gmail
        .history(TOKEN, start, &[HistoryType::MessageAdded], None)
        .unwrap();
    assert_eq!(only.history.len(), 1);

    fake.expire_history_before(start + 1);
    assert!(matches!(
        gmail.history(TOKEN, start, &HistoryType::ALL, None),
        Err(Error::HistoryExpired)
    ));
}

#[test]
fn history_pages_at_500_records() {
    let fake = FakeGmail::start(Mailbox::new(3, 700)).unwrap();
    let gmail = client(&fake);
    let first = gmail.history(TOKEN, 100_000, &[], None).unwrap();
    assert_eq!(first.history.len(), 500);
    let token = first.next_page_token.unwrap();
    let second = gmail.history(TOKEN, 100_000, &[], Some(&token)).unwrap();
    assert_eq!(second.history.len(), 200);
    assert!(second.next_page_token.is_none());
}

#[test]
fn send_and_read_back() {
    let fake = FakeGmail::start(Mailbox::new(3, 5)).unwrap();
    let gmail = client(&fake);
    let raw = b"From: owner@example.com\r\nSubject: Hi\r\n\r\nHello\r\n";
    let sent = gmail.send(TOKEN, raw, None).unwrap();
    assert_eq!(sent.label_ids, ["SENT"]);
    let reply = gmail.send(TOKEN, raw, Some(&sent.thread_id)).unwrap();
    assert_eq!(reply.thread_id, sent.thread_id);
    let mut back = Vec::new();
    gmail.raw(TOKEN, &sent.id, &mut back).unwrap();
    assert_eq!(back, raw);
}

#[test]
fn a_big_message_streams_through_the_buffer() {
    let fake = FakeGmail::start(Mailbox::new(3, 1)).unwrap();
    let gmail = client(&fake);
    let mut raw = b"Subject: big\r\n\r\n".to_vec();
    raw.extend((0..3_000_000u32).map(|i| (i % 251) as u8));
    let id = fake.add_message(&raw, &["INBOX"]);
    let mut back = Vec::new();
    let got = gmail.raw(TOKEN, &id, &mut back).unwrap();
    assert_eq!(got.size, raw.len() as u64);
    assert!(back == raw);
}

#[test]
fn reads_wait_and_try_again_on_rate_limits_and_5xx() {
    let fake = FakeGmail::start(Mailbox::new(3, 5)).unwrap();
    let gmail = client(&fake);
    fake.fail_next(&[429, 403, 500, 503]);
    assert_eq!(gmail.profile(TOKEN).unwrap().messages_total, 5);
    assert_eq!(fake.requests().len(), 5);
}

#[test]
fn a_write_does_not_try_again_on_5xx() {
    let fake = FakeGmail::start(Mailbox::new(3, 5)).unwrap();
    let gmail = client(&fake);
    fake.fail_next(&[429, 503]);
    let e = gmail
        .send(TOKEN, b"Subject: a\r\n\r\nb\r\n", None)
        .unwrap_err();
    assert!(matches!(e, Error::Api { status: 503, .. }));
    assert!(e.write_result_unknown());
    assert_eq!(fake.requests().len(), 2);
    assert_eq!(gmail.profile(TOKEN).unwrap().messages_total, 5);
}

#[test]
fn errors_that_do_not_try_again() {
    let fake = FakeGmail::start(Mailbox::new(3, 5)).unwrap();
    let gmail = client(&fake);
    assert!(matches!(gmail.profile(""), Err(Error::Unauthorized)));
    fake.fail_next(&[400]);
    assert!(matches!(
        gmail.profile(TOKEN),
        Err(Error::Api { status: 400, .. })
    ));
    let down = Gmail::new(Config {
        base_url: "http://127.0.0.1:1/gmail/v1".into(),
        ..Config::default()
    });
    assert!(matches!(down.profile(TOKEN), Err(Error::Transport(_))));
}

#[test]
fn the_client_waits_for_quota() {
    let fake = FakeGmail::start(Mailbox::new(3, 5)).unwrap();
    // 120 units for each minute: 2 each second. A profile call costs 1.
    let gmail = Gmail::new(Config {
        base_url: fake.base_url(),
        quota_per_minute: 120,
        ..Config::default()
    });
    let t = Instant::now();
    for _ in 0..122 {
        gmail.profile(TOKEN).unwrap();
    }
    assert!(
        t.elapsed() >= Duration::from_millis(800),
        "{:?}",
        t.elapsed()
    );
}
