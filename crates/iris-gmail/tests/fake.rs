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
    assert!(p.cursor.parse::<u64>().unwrap() > 0);

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
    assert!(raw.internal_date > 1_700_000_000_000);

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
    let start = gmail.profile(TOKEN).unwrap().cursor;
    let a = FakeGmail::id_of(4);
    fake.modify_labels(&a, &["STARRED"], &[]);
    fake.delete_message(&FakeGmail::id_of(5));
    let new = fake.add_message(b"Subject: x\r\n\r\ny\r\n", &["INBOX"]);

    let page = gmail
        .history(TOKEN, &start, &HistoryType::ALL, None)
        .unwrap();
    let changes: Vec<&Change> = page.history.iter().flat_map(|h| &h.changes).collect();
    assert!(matches!(changes[0], Change::LabelsAdded(m, l) if m.id == a && l == &["STARRED"]));
    assert!(matches!(changes[1], Change::Deleted(_)));
    assert!(matches!(changes[2], Change::Added(m) if m.id == new && m.label_ids == ["INBOX"]));
    assert!(page.cursor.parse::<u64>().unwrap() > start.parse::<u64>().unwrap());
    assert!(page.next_page_token.is_none());

    let only = gmail
        .history(TOKEN, &start, &[HistoryType::MessageAdded], None)
        .unwrap();
    assert_eq!(only.history.len(), 1);

    // A cursor that is not a history ID never reaches Gmail.
    assert!(matches!(
        gmail.history(TOKEN, "12a", &HistoryType::ALL, None),
        Err(Error::Decode(_))
    ));

    fake.expire_history_before(start.parse::<u64>().unwrap() + 1);
    assert!(matches!(
        gmail.history(TOKEN, &start, &HistoryType::ALL, None),
        Err(Error::HistoryExpired)
    ));
}

#[test]
fn history_pages_at_500_records() {
    let fake = FakeGmail::start(Mailbox::new(3, 700)).unwrap();
    let gmail = client(&fake);
    let first = gmail.history(TOKEN, "100000", &[], None).unwrap();
    assert_eq!(first.history.len(), 500);
    let token = first.next_page_token.unwrap();
    let second = gmail.history(TOKEN, "100000", &[], Some(&token)).unwrap();
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
fn a_send_with_a_bad_answer_has_an_unknown_result() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = s.read(&mut buf);
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nxyz")
            .unwrap();
    });
    let gmail = Gmail::new(Config {
        base_url: format!("http://{addr}/gmail/v1"),
        ..Config::default()
    });
    let e = gmail
        .send(TOKEN, b"Subject: a\r\n\r\nb\r\n", None)
        .unwrap_err();
    assert!(e.write_result_unknown(), "{e}");
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

const DRAFT: &[u8] =
    b"From: owner@example.com\r\nSubject: Hi\r\nMessage-ID: <iris.1@example.com>\r\n\r\nHello\r\n";

#[test]
fn a_draft_is_created_read_and_sent_in_its_thread() {
    let fake = FakeGmail::start(Mailbox::new(3, 5)).unwrap();
    let gmail = client(&fake);
    let thread = gmail.messages(TOKEN, &[], true, None).unwrap().messages[0]
        .thread_id
        .clone();
    let d = gmail.create_draft(TOKEN, DRAFT, Some(&thread)).unwrap();
    assert_eq!(d.message.thread_id, thread);
    let got = gmail.draft_raw(TOKEN, &d.id).unwrap();
    assert_eq!(got.message.thread_id, thread);
    // Like Gmail, the fake sets its own Message-ID and adds `Received`.
    let got = String::from_utf8(got.raw.unwrap()).unwrap();
    assert!(got.starts_with("Received: "), "{got}");
    assert!(got.contains("Message-ID: <CACe_fake0000000001@mail.gmail.com>\r\n"));
    assert!(!got.contains("iris.1@example.com"));
    assert!(got.contains("\r\nSubject: Hi\r\n") && got.ends_with("\r\n\r\nHello\r\n"));

    // The person edits the draft at Gmail; the send carries the bytes of Iris.
    assert!(fake.edit_draft(
        &d.id,
        b"Subject: Hi\r\nMessage-ID: <iris.1@example.com>\r\n\r\nBye\r\n"
    ));

    let found = gmail
        .drafts(TOKEN, "rfc822msgid:<iris.1@example.com>")
        .unwrap();
    assert_eq!(found.iter().map(|d| &d.id).collect::<Vec<_>>(), [&d.id]);
    assert!(gmail
        .drafts(TOKEN, "rfc822msgid:iris.2@example.com")
        .unwrap()
        .is_empty());

    // With no thread ID, the sent message goes into a new thread: the
    // thread ID of the request decides.
    let lone_raw = b"From: owner@example.com\r\nSubject: Hi\r\nMessage-ID: <iris.9@example.com>\r\n\r\nHello\r\n";
    let other = gmail.create_draft(TOKEN, lone_raw, Some(&thread)).unwrap();
    let lone = gmail.send_draft(TOKEN, &other.id, lone_raw, None).unwrap();
    assert_ne!(lone.thread_id, thread);
    let sent = gmail
        .send_draft(TOKEN, &d.id, DRAFT, Some(&thread))
        .unwrap();
    assert_eq!(
        (sent.thread_id.as_str(), sent.label_ids.as_slice()),
        (thread.as_str(), &["SENT".to_string()][..])
    );
    let mut out = Vec::new();
    gmail.raw(TOKEN, &sent.id, &mut out).unwrap();
    assert_eq!(out, DRAFT);
    assert!(matches!(
        gmail.draft_raw(TOKEN, &d.id),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        gmail.send_draft(TOKEN, &d.id, DRAFT, None),
        Err(Error::Api { status: 404, .. }) | Err(Error::NotFound)
    ));

    // The search finds the sent message by its Message-ID, also in another form.
    let page = gmail
        .search(TOKEN, "rfc822msgid:iris.1@example.com", None)
        .unwrap();
    assert_eq!(
        page.messages.iter().map(|m| &m.id).collect::<Vec<_>>(),
        [&sent.id]
    );
}

#[test]
fn a_draft_send_with_a_lost_answer_is_unknown_and_the_search_finds_it() {
    let fake = FakeGmail::start(Mailbox::new(3, 5)).unwrap();
    let gmail = client(&fake);
    let d = gmail.create_draft(TOKEN, DRAFT, None).unwrap();
    fake.fail_next_write(&[503], true);
    let e = gmail.send_draft(TOKEN, &d.id, DRAFT, None).unwrap_err();
    assert!(e.write_result_unknown(), "{e}");
    let sends = fake
        .requests()
        .iter()
        .filter(|r| r.ends_with("/drafts/send"))
        .count();
    assert_eq!(sends, 1);
    let page = gmail
        .search(TOKEN, "rfc822msgid:<iris.1@example.com>", None)
        .unwrap();
    assert_eq!(page.messages.len(), 1);
}

#[test]
fn a_rate_limit_of_a_draft_write_is_unknown_and_has_no_second_request() {
    let fake = FakeGmail::start(Mailbox::new(3, 5)).unwrap();
    let gmail = client(&fake);
    fake.fail_next_write(&[429], false);
    assert!(gmail
        .create_draft(TOKEN, DRAFT, None)
        .unwrap_err()
        .write_result_unknown());
    let d = gmail.create_draft(TOKEN, DRAFT, None).unwrap();
    fake.fail_next_write(&[429], false);
    let e = gmail.send_draft(TOKEN, &d.id, DRAFT, None).unwrap_err();
    assert!(e.write_result_unknown(), "{e}");
    let count = |end: &str| fake.requests().iter().filter(|r| r.ends_with(end)).count();
    assert_eq!((count("/drafts"), count("/drafts/send")), (2, 1));
}

#[test]
fn the_person_can_edit_and_delete_a_draft_at_the_fake() {
    let fake = FakeGmail::start(Mailbox::new(3, 5)).unwrap();
    let gmail = client(&fake);
    let d = gmail.create_draft(TOKEN, DRAFT, None).unwrap();
    assert!(fake.edit_draft(&d.id, b"Subject: x\r\n\r\ny\r\n"));
    assert_eq!(
        gmail.draft_raw(TOKEN, &d.id).unwrap().raw.as_deref(),
        Some(&b"Subject: x\r\n\r\ny\r\n"[..])
    );
    assert!(fake.delete_draft(&d.id));
    assert!(fake.draft_ids().is_empty());
    assert!(matches!(
        gmail.draft_raw(TOKEN, &d.id),
        Err(Error::NotFound)
    ));
}

/// A draft larger than the answer limit of a JSON call still reads: the
/// client streams it, and gives no raw for a message over DRAFT_LIMIT.
#[test]
fn a_large_draft_reads_with_no_raw() {
    let fake = FakeGmail::start(Mailbox::new(3, 5)).unwrap();
    let gmail = client(&fake);
    let d = gmail.create_draft(TOKEN, DRAFT, None).unwrap();
    // 13 MiB of attachment text: more than 16 MiB as base64 in the answer.
    let mut big = b"Subject: x\r\n\r\n".to_vec();
    big.resize(13 * 1024 * 1024, b'a');
    assert!(fake.edit_draft(&d.id, &big));
    let got = gmail.draft_raw(TOKEN, &d.id).unwrap();
    assert_eq!((got.id.as_str(), got.raw), (d.id.as_str(), None));
    // At the limit, the raw is kept.
    big.truncate(iris_gmail::DRAFT_LIMIT);
    assert!(fake.edit_draft(&d.id, &big));
    assert_eq!(
        gmail.draft_raw(TOKEN, &d.id).unwrap().raw.map(|r| r.len()),
        Some(iris_gmail::DRAFT_LIMIT)
    );
}

#[test]
fn a_draft_search_reads_every_page() {
    let fake = FakeGmail::start(Mailbox::new(3, 10)).unwrap();
    let gmail = client(&fake);
    let other = b"Subject: Hi\r\nMessage-ID: <other@example.com>\r\n\r\nHello\r\n";
    let raw = b"Subject: Hi\r\nMessage-ID: <iris.5@example.com>\r\n\r\nHello\r\n";
    gmail.create_draft(TOKEN, other, None).unwrap();
    // Gmail gives each new draft its own Message-ID: an edit at Gmail
    // gives the three drafts the same ID.
    let ids: Vec<String> = (0..3)
        .map(|_| gmail.create_draft(TOKEN, raw, None).unwrap().id)
        .collect();
    for id in &ids {
        assert!(fake.edit_draft(id, raw));
    }
    fake.set_page_limit(1);

    // The matching drafts are on three pages. Without the page tokens,
    // the search finds only the first one.
    let found = gmail
        .drafts(TOKEN, "rfc822msgid:<iris.5@example.com>")
        .unwrap();
    assert_eq!(found.iter().map(|d| d.id.clone()).collect::<Vec<_>>(), ids);
    assert!(fake
        .requests()
        .iter()
        .any(|r| r.contains("drafts?") && r.contains("pageToken=2")));
}

// The send recovery must see each message with the Message-ID, also past
// the first page of 500 (provider spec, section 2).
#[test]
fn messages_with_id_reads_every_page() {
    let fake = FakeGmail::start(Mailbox::new(3, 2)).unwrap();
    let gmail = client(&fake);
    let raw = b"Message-ID: <many@example.com>\r\nSubject: x\r\n\r\ny\r\n";
    let added: std::collections::BTreeSet<String> =
        (0..501).map(|_| fake.add_message(raw, &["SENT"])).collect();
    let found = iris_gmail::Provider::messages_with_id(&gmail, TOKEN, "many@example.com").unwrap();
    let ids: std::collections::BTreeSet<String> = found.into_iter().map(|m| m.id).collect();
    assert_eq!(ids, added);
}
