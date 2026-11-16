//! People profiles from a store that the first sync filled from the fake
//! Gmail server. No network.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use iris_fakegmail::FakeGmail;
use iris_gmail::{Config, Gmail};
use iris_people::{addresses, people, person};
use iris_store::{Header, NewMessage, Store};
use iris_synth::{Mailbox, Sender, OWNER};

fn data_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("iris-people-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The value of one header of a raw message, unfolded.
fn header(raw: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let head = text.split("\r\n\r\n").next().unwrap_or("");
    let mut out: Option<String> = None;
    let mut inside = false;
    for line in head.split("\r\n") {
        if line.starts_with([' ', '\t']) {
            if inside {
                out.as_mut().unwrap().push_str(line);
            }
            continue;
        }
        inside = out.is_none()
            && line
                .split_once(':')
                .is_some_and(|(n, _)| n.eq_ignore_ascii_case(name));
        if inside {
            out = Some(line.split_once(':').unwrap().1.trim().to_string());
        }
    }
    out
}

/// The display name of a `From` value such as `Name <a@b>`.
fn display_name(from: &str) -> Option<String> {
    from.rfind('<')
        .map(|i| from[..i].trim().trim_matches('"').to_string())
}

/// Fill the `header` table from the content files, as the understanding
/// step does (specification 1, section 7.1, step 1).
fn fill_headers(store: &Store) {
    let rows: Vec<(i64, String)> = store
        .read(|c| {
            let mut st =
                c.prepare("SELECT id, content_sha256 FROM message WHERE gone_at IS NULL")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    store
        .write(|tx| {
            for (id, sha) in &rows {
                let raw = std::fs::read(store.content_path(sha).unwrap()).unwrap();
                let from = header(&raw, "From").unwrap_or_default();
                tx.execute(
                    "UPDATE header SET from_addr = ?2, from_name = ?3, to_addrs = ?4, cc_addrs = ?5
                     WHERE message_id = ?1",
                    rusqlite::params![
                        id,
                        addresses(&from).first(),
                        display_name(&from),
                        header(&raw, "To"),
                        header(&raw, "Cc")
                    ],
                )?;
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn each_known_person_of_the_synthetic_mailbox_has_a_profile() {
    let mailbox = Mailbox::new(7, 3_000);
    let fake = FakeGmail::start(Mailbox::new(7, 3_000)).unwrap();
    let gmail = Gmail::new(Config {
        base_url: fake.base_url(),
        quota_per_minute: 10_000_000,
        backoff_start: Duration::from_millis(1),
        backoff_max: Duration::from_millis(4),
        ..Config::default()
    });
    let dir = data_dir("synth");
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("gmail", OWNER).unwrap();
    iris_sync::sync(&store, &gmail, account, "token").unwrap();
    fill_headers(&store);

    // The truth: each known person, with the dates of the messages from
    // that person. Known means the person rule of specification 1, section
    // 6.7: you sent mail to the address, or sent mail in a thread with it.
    // Each 25th message of the owner is a draft in the fake (no mail sent).
    let mut wrote_to: BTreeSet<String> = BTreeSet::new();
    let mut your_threads: BTreeSet<u32> = BTreeSet::new();
    for i in 0..mailbox.len() {
        if mailbox.truth(i).sender == Sender::Owner && i % 25 != 3 {
            wrote_to.extend(addresses(&header(&mailbox.raw(i), "To").unwrap()));
            your_threads.insert(mailbox.truth(i).thread);
        }
    }
    let mut truth: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    let mut bad: BTreeSet<String> = BTreeSet::new();
    let mut unknown = 0;
    for i in 0..mailbox.len() {
        let t = mailbox.truth(i);
        let from = addresses(&header(&mailbox.raw(i), "From").unwrap())[0].clone();
        match t.sender {
            Sender::KnownPerson if wrote_to.contains(&from) || your_threads.contains(&t.thread) => {
                truth.entry(from).or_default().insert(t.date);
            }
            Sender::KnownPerson => unknown += 1,
            Sender::Hostile | Sender::KnownService => {
                bad.insert(from);
            }
            _ => {}
        }
    }
    println!(
        "known people: {}, messages of people you never wrote to: {unknown}",
        truth.len()
    );
    assert!(
        truth.len() >= 50,
        "the test needs known people: {}",
        truth.len()
    );

    let all = people(&store, account).unwrap();
    for (addr, dates) in &truth {
        let p = all
            .iter()
            .find(|p| p.addresses.iter().any(|a| &a.address == addr))
            .unwrap_or_else(|| panic!("no profile for {addr}"));
        assert!(p.id.starts_with("p-"), "{}", p.id);
        assert!(p.name.is_some(), "{addr} has no name");
        assert!(p.from_them as usize >= dates.len(), "{addr}: {p:?}");
        assert!(!p.threads.is_empty() && p.threads.iter().all(|t| t.starts_with("t-")));
        let first = *dates.iter().next().unwrap() * 1000;
        let last = *dates.iter().next_back().unwrap() * 1000;
        assert!(p.first_at <= first && p.last_at >= last, "{addr}: {p:?}");
        assert_eq!(person(&store, account, &p.id).unwrap().as_ref(), Some(p));
    }
    for p in &all {
        for a in &p.addresses {
            assert_ne!(a.address, OWNER, "you are not one of your people");
            assert!(!bad.contains(&a.address), "{} is not a person", a.address);
        }
    }
    // A second run gives the same IDs: the sender rows stay.
    assert_eq!(people(&store, account).unwrap(), all);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A small store with hand-written messages.
struct Hand {
    store: Store,
    account: i64,
    next: i64,
    dir: std::path::PathBuf,
}

impl Hand {
    fn new(name: &str) -> Hand {
        let dir = data_dir(name);
        let store = Store::open(&dir).unwrap();
        let account = store.add_account("gmail", "me@example.com").unwrap();
        store
            .put_labels(
                account,
                &[
                    ("SENT", "SENT", "system"),
                    ("DRAFT", "DRAFT", "system"),
                    ("SPAM", "SPAM", "system"),
                    ("INBOX", "INBOX", "system"),
                ],
            )
            .unwrap();
        Hand {
            store,
            account,
            next: 0,
            dir,
        }
    }

    fn add(&mut self, thread: &str, label: &str, from: &str, name: &str, to: &str) -> i64 {
        self.next += 1;
        self.store
            .put_messages(
                self.account,
                &[NewMessage {
                    provider_id: format!("m{}", self.next),
                    thread_provider_id: thread.into(),
                    internal_date: Some(self.next * 1000),
                    labels: vec![label.into()],
                    header: Header {
                        from_addr: Some(from.into()),
                        from_name: Some(name.into()).filter(|n: &String| !n.is_empty()),
                        to_addrs: Some(to.into()),
                        ..Header::default()
                    },
                    ..NewMessage::default()
                }],
            )
            .unwrap()[0]
    }

    fn people(&self) -> Vec<iris_people::Person> {
        people(&self.store, self.account).unwrap()
    }
}

impl Drop for Hand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn addrs(p: &iris_people::Person) -> Vec<&str> {
    p.addresses.iter().map(|a| a.address.as_str()).collect()
}

#[test]
fn the_person_rule() {
    let mut h = Hand::new("rule");
    // You wrote to Ana. Bo wrote in a thread where you replied.
    h.add(
        "t1",
        "SENT",
        "me@example.com",
        "Me",
        "Ana Lind <Ana@Example.org>",
    );
    h.add("t2", "INBOX", "bo@example.org", "Bo Hart", "me@example.com");
    h.add("t2", "SENT", "me@example.com", "Me", "list@example.org");
    // A draft to Cy does not count. Dee wrote, and you never answered.
    h.add("t3", "DRAFT", "me@example.com", "Me", "cy@example.org");
    h.add(
        "t4",
        "INBOX",
        "dee@example.org",
        "Dee Ray",
        "me@example.com",
    );
    // Mail from your alias is from you.
    h.add("t5", "SENT", "alias@example.com", "Me", "ana@example.org");
    h.add("t6", "INBOX", "alias@example.com", "Me", "me@example.com");

    let all = h.people();
    let found: BTreeSet<&str> = all.iter().flat_map(addrs).collect();
    assert_eq!(
        found,
        BTreeSet::from(["ana@example.org", "bo@example.org", "list@example.org"])
    );
    let ana = all
        .iter()
        .find(|p| addrs(p) == ["ana@example.org"])
        .unwrap();
    assert_eq!((ana.from_you, ana.from_them, ana.threads.len()), (2, 0, 2));
    assert_eq!(ana.first_at, 1000);
    // Ana is known only from sent mail: the name comes from the To field.
    assert_eq!(ana.name.as_deref(), Some("Ana Lind"));
    assert_eq!(ana.names, ["Ana Lind"]);
    assert_eq!(ana.last_at, 6000);
    let bo = all.iter().find(|p| addrs(p) == ["bo@example.org"]).unwrap();
    assert_eq!((bo.from_you, bo.from_them), (0, 1));
    assert_eq!(bo.name.as_deref(), Some("Bo Hart"));
    // Newest first: Ana (6000), then the list and Bo.
    assert_eq!(addrs(&all[0]), ["ana@example.org"]);
}

#[test]
fn two_addresses_of_one_person_make_one_profile() {
    let mut h = Hand::new("merge");
    h.add(
        "t1",
        "INBOX",
        "ana@work.example.org",
        "Ana Lind",
        "me@example.com",
    );
    h.add("t1", "SENT", "me@example.com", "Me", "ana@work.example.org");
    h.add(
        "t2",
        "INBOX",
        "ana@home.example.net",
        "ana  lind",
        "me@example.com",
    );
    h.add(
        "t2",
        "SENT",
        "me@example.com",
        "Me",
        "ana@home.example.net, ana@work.example.org",
    );
    // Only a first name: no merge.
    h.add("t3", "INBOX", "kim@a.example.org", "Kim", "me@example.com");
    h.add("t3", "SENT", "me@example.com", "Me", "kim@a.example.org");
    h.add("t4", "INBOX", "kim@b.example.org", "Kim", "me@example.com");
    h.add("t4", "SENT", "me@example.com", "Me", "kim@b.example.org");

    let all = h.people();
    assert_eq!(all.len(), 3, "{all:?}");
    let ana = all.iter().find(|p| p.addresses.len() == 2).unwrap();
    assert_eq!(addrs(ana), ["ana@home.example.net", "ana@work.example.org"]);
    assert_eq!(ana.id, ana.addresses[0].id);
    assert_eq!(ana.names, ["Ana Lind", "ana  lind"]);
    assert_eq!(ana.name.as_deref(), Some("ana  lind"));
    // The message to both addresses counts one time.
    assert_eq!((ana.from_them, ana.from_you, ana.threads.len()), (2, 2, 2));
    // The ID of each address finds the profile.
    let by_second = person(&h.store, h.account, &ana.addresses[1].id).unwrap();
    assert_eq!(by_second.as_ref(), Some(ana));
    assert_eq!(person(&h.store, h.account, "m-1").unwrap(), None);
    assert_eq!(person(&h.store, h.account, "p-999").unwrap(), None);
}

#[test]
fn hostile_mail_does_not_make_or_join_a_person() {
    let mut h = Hand::new("hostile");
    h.add(
        "t1",
        "INBOX",
        "ana@example.org",
        "Ana Lind",
        "me@example.com",
    );
    h.add("t1", "SENT", "me@example.com", "Me", "ana@example.org");
    // A spoof with the name of Ana, in spam. You replied to it.
    h.add(
        "t2",
        "SPAM",
        "ana@evil.example.net",
        "Ana Lind",
        "me@example.com",
    );
    h.add("t2", "SENT", "me@example.com", "Me", "ana@evil.example.net");
    // A spoof with a hostile mark, in a thread with you.
    let marked = h.add(
        "t1",
        "INBOX",
        "ana@fake.example.net",
        "Ana Lind",
        "me@example.com",
    );
    h.store
        .write(|tx| {
            tx.execute(
                "UPDATE message SET hostile = '[\"spoof\"]' WHERE id = ?1",
                [marked],
            )?;
            Ok(())
        })
        .unwrap();

    let all = h.people();
    let ana = all
        .iter()
        .find(|p| addrs(p) == ["ana@example.org"])
        .unwrap();
    assert_eq!(ana.from_them, 1, "the marked message does not count");
    assert!(
        all.iter()
            .all(|p| !addrs(p).contains(&"ana@fake.example.net")),
        "{all:?}"
    );
    // The spam address has a profile of its own (you wrote to it), never
    // the profile of Ana.
    let evil = all
        .iter()
        .find(|p| addrs(p).contains(&"ana@evil.example.net"))
        .unwrap();
    assert_eq!(addrs(evil), ["ana@evil.example.net"]);
}

#[test]
fn the_kind_in_the_sender_table_wins() {
    let mut h = Hand::new("kind");
    h.add(
        "t1",
        "SENT",
        "me@example.com",
        "Me",
        "ana@example.org, bo@example.org",
    );
    h.add("t2", "INBOX", "cy@example.org", "Cy Moss", "me@example.com");
    h.store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO sender(address, kind, decided_by) VALUES
                   ('ana@example.org', 'hostile', 'engine'),
                   ('bo@example.org', 'stranger', 'person'),
                   ('cy@example.org', 'person', 'person');",
            )?;
            Ok(())
        })
        .unwrap();
    let all = h.people();
    let found: Vec<&str> = all.iter().flat_map(addrs).collect();
    assert_eq!(found, ["cy@example.org"]);
}

#[test]
fn spam_sent_mail_does_not_make_a_thread_yours() {
    let mut h = Hand::new("spam-sent");
    h.add("t1", "SPAM", "me@example.com", "Me", "x@example.org");
    h.add(
        "t1",
        "INBOX",
        "eve@example.org",
        "Eve Moor",
        "me@example.com",
    );
    // The SPAM label makes the sent message bad, also with SENT.
    let sent = h.add("t2", "SENT", "me@example.com", "Me", "y@example.org");
    h.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO message_label(message_id, label_id)
                 SELECT ?1, id FROM label WHERE provider_id = 'SPAM'",
                [sent],
            )?;
            Ok(())
        })
        .unwrap();
    h.add(
        "t2",
        "INBOX",
        "zed@example.org",
        "Zed Pike",
        "me@example.com",
    );
    assert_eq!(h.people(), vec![], "no valid sent message");
}

#[test]
fn a_bcc_recipient_is_a_known_person() {
    let mut h = Hand::new("bcc");
    let raw = b"From: Me <me@example.com>\r\nTo: ana@example.org\r\nBcc: Bo Hart\r\n <Bo@Example.org>, cy@example.org (Cy Moss)\r\nSubject: hi\r\n\r\nBcc: not@example.org\r\n";
    let sha = h.store.put_content(raw).unwrap();
    h.next += 1;
    h.store
        .put_messages(
            h.account,
            &[NewMessage {
                provider_id: "bcc".into(),
                thread_provider_id: "t1".into(),
                content_sha256: Some(sha),
                internal_date: Some(1000),
                labels: vec!["SENT".into()],
                header: Header {
                    from_addr: Some("me@example.com".into()),
                    to_addrs: Some("ana@example.org".into()),
                    ..Header::default()
                },
                ..NewMessage::default()
            }],
        )
        .unwrap();
    let all = h.people();
    let found: BTreeSet<&str> = all.iter().flat_map(addrs).collect();
    assert_eq!(
        found,
        BTreeSet::from(["ana@example.org", "bo@example.org", "cy@example.org"])
    );
    let cy = all.iter().find(|p| addrs(p) == ["cy@example.org"]).unwrap();
    assert_eq!(cy.name.as_deref(), Some("Cy Moss"));
}

#[test]
fn a_quoted_local_part_is_an_address() {
    let mut h = Hand::new("quoted");
    h.add(
        "t1",
        "SENT",
        "me@example.com",
        "Me",
        r#"John <"john.doe"@example.org>"#,
    );
    let all = h.people();
    assert_eq!(all.len(), 1);
    assert_eq!(addrs(&all[0]), [r#""john.doe"@example.org"#]);
    assert_eq!(all[0].name.as_deref(), Some("John"));
}

#[test]
fn a_repeated_recipient_keeps_its_name() {
    let h = Hand::new("repeat");
    let raw = b"From: me@example.com\r\nTo: ana@example.org\r\nBcc: Ana Lind <ana@example.org>\r\n\r\nhi\r\n";
    let sha = h.store.put_content(raw).unwrap();
    h.store
        .put_messages(
            h.account,
            &[NewMessage {
                provider_id: "r1".into(),
                thread_provider_id: "t1".into(),
                content_sha256: Some(sha),
                internal_date: Some(1000),
                labels: vec!["SENT".into()],
                header: Header {
                    from_addr: Some("me@example.com".into()),
                    to_addrs: Some("ana@example.org".into()),
                    cc_addrs: Some("Ana Lind <ana@example.org>".into()),
                    ..Header::default()
                },
                ..NewMessage::default()
            }],
        )
        .unwrap();
    let all = h.people();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].name.as_deref(), Some("Ana Lind"));
    assert_eq!(
        all[0].from_you, 1,
        "one message, also with the address three times"
    );
}

#[test]
fn group_labels_and_encoded_names_do_not_stop_a_merge() {
    let mut h = Hand::new("group");
    h.add(
        "t1",
        "INBOX",
        "ana@home.example.net",
        "Ana Lind",
        "me@example.com",
    );
    h.add("t1", "SENT", "me@example.com", "Me", "ana@home.example.net");
    h.add(
        "t2",
        "SENT",
        "me@example.com",
        "Me",
        "Team: Ana Lind <ana@work.example.org>;",
    );
    let raw = b"From: me@example.com\r\nTo: x@example.com\r\nBcc: =?UTF-8?Q?Bo_H=C3=A4rt?= <bo@a.example.org>\r\n\r\nhi\r\n";
    let sha = h.store.put_content(raw).unwrap();
    h.store
        .put_messages(
            h.account,
            &[NewMessage {
                provider_id: "enc".into(),
                thread_provider_id: "t3".into(),
                content_sha256: Some(sha),
                internal_date: Some(9000),
                labels: vec!["SENT".into()],
                header: Header {
                    from_addr: Some("me@example.com".into()),
                    to_addrs: Some("x@example.com".into()),
                    ..Header::default()
                },
                ..NewMessage::default()
            }],
        )
        .unwrap();
    h.add(
        "t4",
        "INBOX",
        "bo@b.example.org",
        "Bo Härt",
        "me@example.com",
    );
    h.add("t4", "SENT", "me@example.com", "Me", "bo@b.example.org");

    let all = h.people();
    let ana = all
        .iter()
        .find(|p| addrs(p).contains(&"ana@work.example.org"))
        .unwrap();
    assert_eq!(addrs(ana), ["ana@home.example.net", "ana@work.example.org"]);
    assert_eq!(ana.names, ["Ana Lind"]);
    let bo = all
        .iter()
        .find(|p| addrs(p).contains(&"bo@a.example.org"))
        .unwrap();
    assert_eq!(addrs(bo), ["bo@a.example.org", "bo@b.example.org"]);
    assert_eq!(bo.names, ["Bo Härt"]);
}
