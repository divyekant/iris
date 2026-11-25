//! Tests of the conversation rule on a real store: hand-written threads, and
//! each thread of a synthetic mailbox. Each conversation must validate
//! against `docs/spec/schema/conversation.schema.json`.

use std::fs;
use std::path::{Path, PathBuf};

use iris_store::{Body, Header, NewMessage, Store};
use iris_synth::{Mailbox, Sender, OWNER};
use iris_talk::{rfc3339, update, QUIET_MS};
use jsonschema::{Draft, Registry, Validator};
use serde_json::Value;

const DAY: i64 = 24 * 60 * 60 * 1000;
/// 2026-11-16T00:00:00Z.
const NOW: i64 = 1_794_787_200_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-talk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn validator() -> Validator {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/schema");
    let read =
        |p: &Path| -> Value { serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap() };
    let mut registry = Registry::new();
    for e in fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "json") {
            let s = read(&p);
            registry = registry
                .add(String::from(s["$id"].as_str().unwrap()), s)
                .unwrap();
        }
    }
    let registry = registry.prepare().unwrap();
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .should_validate_formats(true)
        .offline()
        .with_registry(&registry)
        .build(&read(&dir.join("conversation.schema.json")))
        .unwrap()
}

fn check(v: &Validator, doc: &Value) {
    let errors: Vec<String> = v
        .iter_errors(doc)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{doc:#}\n{errors:#?}");
}

struct Box_ {
    store: Store,
    account: i64,
    _dir: TempDir,
}

/// One message of a hand-written thread.
struct M<'a> {
    id: &'a str,
    thread: &'a str,
    at: i64,
    from: (&'a str, &'a str),
    to: &'a str,
    cc: &'a str,
    subject: &'a str,
    text: &'a str,
    labels: &'a [&'a str],
}

impl Default for M<'_> {
    fn default() -> Self {
        M {
            id: "",
            thread: "t1",
            at: NOW - DAY,
            from: ("Sam Ortiz", "sam@fabrikam.example"),
            to: OWNER,
            cc: "",
            subject: "Dinner on Thursday?",
            text: "Are you free?",
            labels: &["INBOX"],
        }
    }
}

impl Box_ {
    fn new(name: &str) -> Box_ {
        let dir = TempDir::new(name);
        let store = Store::open(&dir.0).unwrap();
        let account = store.add_account("gmail", OWNER).unwrap();
        store
            .put_labels(
                account,
                &[
                    ("INBOX", "INBOX", "system"),
                    ("SENT", "SENT", "system"),
                    ("DRAFT", "DRAFT", "system"),
                    ("UNSENT", "UNSENT", "system"),
                    ("SENT_COPY", "SENT_COPY", "system"),
                ],
            )
            .unwrap();
        Box_ {
            store,
            account,
            _dir: dir,
        }
    }

    /// Write one understood message. `text` is the clean text.
    fn put(&self, m: M) -> i64 {
        let none = |s: &str| (!s.is_empty()).then(|| s.to_string());
        let msg = NewMessage {
            provider_id: m.id.into(),
            thread_provider_id: m.thread.into(),
            internal_date: Some(m.at),
            fetched_at: Some(m.at),
            labels: m.labels.iter().map(|l| l.to_string()).collect(),
            header: Header {
                date: Some(m.at),
                from_addr: Some(m.from.1.into()),
                from_name: none(m.from.0),
                to_addrs: none(m.to),
                cc_addrs: none(m.cc),
                subject: Some(m.subject.into()),
                ..Header::default()
            },
            body: Some(Body {
                subject: m.subject.into(),
                sender: m.from.1.into(),
                text: m.text.into(),
            }),
            ..NewMessage::default()
        };
        let id = self.store.put_messages(self.account, &[msg]).unwrap()[0];
        self.sql(&format!(
            "UPDATE message SET understood_at = {} WHERE id = {id}",
            m.at
        ));
        id
    }

    fn sql(&self, s: &str) {
        self.store.write(|tx| Ok(tx.execute_batch(s)?)).unwrap();
    }

    fn kind(&self, address: &str, kind: &str) {
        self.sql(&format!(
            "INSERT INTO sender(address, kind) VALUES ('{address}', '{kind}')
             ON CONFLICT(address) DO UPDATE SET kind = excluded.kind"
        ));
    }

    fn thread(&self, pid: &str) -> i64 {
        self.store
            .read(|c| {
                Ok(
                    c.query_row("SELECT id FROM thread WHERE provider_id = ?1", [pid], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap()
    }

    /// Run the rule on a thread, and give the saved document.
    fn talk(&self, pid: &str, now: i64) -> Option<Value> {
        let t = self.thread(pid);
        let id = self
            .store
            .write(|tx| Ok(update(tx, t, None, now)?))
            .unwrap()?;
        Some(self.doc(id))
    }

    fn doc(&self, id: i64) -> Value {
        let (doc, status): (String, String) = self
            .store
            .read(|c| {
                Ok(
                    c.query_row("SELECT doc, status FROM item WHERE id = ?1", [id], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })?,
                )
            })
            .unwrap();
        let doc: Value = serde_json::from_str(&doc).unwrap();
        assert_eq!(
            doc["status"],
            status.as_str(),
            "the status column follows the document"
        );
        doc
    }

    fn sources(&self, item: &str) -> Vec<(i64, String)> {
        let id: i64 = item[2..].parse().unwrap();
        self.store
            .read(|c| {
                let mut q = c.prepare("SELECT message_id, role FROM item_source WHERE item_id = ?1 ORDER BY message_id")?;
                let rows = q.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap()
    }
}

/// A raw reply with quoted history, as common clients write it.
fn raw(from: &str, subject: &str, body: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: {OWNER}\r\nSubject: {subject}\r\nIn-Reply-To: <x@example.com>\r\n\
         MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}"
    )
    .replace('\n', "\r\n")
    .replace("\r\r\n", "\r\n")
    .into_bytes()
}

#[test]
fn a_thread_with_replies_is_one_talk_without_quoted_text() {
    let b = Box_::new("reply");
    let v = validator();
    let first = "Are you free for dinner on Thursday?\n\nSam\nFabrikam Ltd\n+1 555 0100";
    let reply = iris_clean::clean(&raw(
        OWNER,
        "Re: Dinner on Thursday?",
        "Yes, Thursday works.\n\nOn Mon, 9 Nov 2026 at 10:00, Sam Ortiz <sam@fabrikam.example> wrote:\n> Are you free for dinner on Thursday?\n> Sam",
    ))
    .text;
    let again = iris_clean::clean(&raw(
        "Sam Ortiz <sam@fabrikam.example>",
        "Re: Dinner on Thursday?",
        "Great, 7 pm at the usual place?\n\nSam\nFabrikam Ltd\n+1 555 0100\n\n> Yes, Thursday works.\n>\n> On Mon, 9 Nov 2026, Sam Ortiz wrote:\n>> Are you free for dinner on Thursday?",
    ))
    .text;
    assert!(!reply.contains("wrote:") && !reply.contains('>'), "{reply}");

    let m1 = b.put(M {
        id: "a",
        at: NOW - 3 * DAY,
        text: first,
        ..M::default()
    });
    let m2 = b.put(M {
        id: "b",
        at: NOW - 2 * DAY,
        from: ("Avel Torun", OWNER),
        to: "sam@fabrikam.example",
        subject: "Re: Dinner on Thursday?",
        text: &reply,
        labels: &["SENT"],
        ..M::default()
    });
    let m3 = b.put(M {
        id: "c",
        at: NOW - DAY,
        subject: "Re: Dinner on Thursday?",
        text: &again,
        ..M::default()
    });

    let doc = b.talk("t1", NOW).expect("a conversation");
    check(&v, &doc);
    assert_eq!(doc["messages"], 3);
    assert_eq!(doc["title"], "Dinner on Thursday?");
    assert_eq!(doc["role"], "direct");
    assert_eq!(doc["your_turn"], true);
    assert_eq!(doc["new"], 3, "never viewed");
    assert_eq!(doc["status"], "active");
    assert_eq!(doc["derived"]["rule"], "thread");
    assert_eq!(doc["participants"].as_array().unwrap().len(), 2);
    assert_eq!(doc["participants"][1]["address"], "sam@fabrikam.example");
    assert_eq!(doc["participants"][1]["name"], "Sam Ortiz");
    assert_eq!(
        doc["latest"]["from"]["sender"],
        doc["participants"][1]["sender"]
    );
    assert_eq!(doc["latest"]["at"], rfc3339(NOW - DAY));
    // No quoted history and no repeated signature in the latest message.
    assert_eq!(doc["latest"]["snippet"], "Great, 7 pm at the usual place?");
    let roles: Vec<(&str, &str)> = doc["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["message"].as_str().unwrap(), s["role"].as_str().unwrap()))
        .collect();
    let (s1, s2, s3) = (format!("m-{m1}"), format!("m-{m2}"), format!("m-{m3}"));
    assert_eq!(
        roles,
        [
            (s1.as_str(), "origin"),
            (s2.as_str(), "update"),
            (s3.as_str(), "update")
        ]
    );
    assert_eq!(doc["sources"][0]["quote"], "Dinner on Thursday?");
    let id = doc["id"].as_str().unwrap();
    assert_eq!(
        b.sources(id),
        [
            (m1, "origin".into()),
            (m2, "update".into()),
            (m3, "update".into())
        ]
    );

    // A second run with no change writes nothing.
    let again = b.talk("t1", NOW + 1000).unwrap();
    assert_eq!(again, doc);
}

#[test]
fn groups_cc_and_turns() {
    let b = Box_::new("roles");
    let v = validator();
    b.put(M {
        id: "g1",
        thread: "g",
        to: "owner@example.com, \"Ortiz, Ana\" <Ana@x.example>",
        ..M::default()
    });
    let doc = b.talk("g", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["role"], "group");
    assert_eq!(doc["participants"][2]["address"], "ana@x.example");
    assert!(
        doc["participants"][2].get("name").is_none(),
        "no From gave a name"
    );

    b.put(M {
        id: "c1",
        thread: "c",
        to: "lee@x.example",
        cc: OWNER,
        ..M::default()
    });
    let doc = b.talk("c", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["role"], "cc");
    assert_eq!(doc["your_turn"], false, "you are not in To");

    // The last message is yours: not your turn.
    b.put(M {
        id: "y1",
        thread: "y",
        ..M::default()
    });
    b.put(M {
        id: "y2",
        thread: "y",
        at: NOW - 1000,
        from: ("", OWNER),
        to: "sam@fabrikam.example",
        labels: &["SENT"],
        ..M::default()
    });
    let doc = b.talk("y", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["your_turn"], false);
    assert_eq!(doc["latest"]["from"]["kind"], "self");
    assert_eq!(doc["role"], "direct");

    // A sent message from an alias is yours, and the alias is no party.
    b.put(M {
        id: "a1",
        thread: "al",
        from: ("Avel", "avel@alias.example"),
        to: "sam@fabrikam.example",
        labels: &["SENT"],
        ..M::default()
    });
    let doc = b.talk("al", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["participants"].as_array().unwrap().len(), 2);
    assert_eq!(doc["latest"]["from"]["kind"], "self");
}

#[test]
fn services_hostile_mail_and_drafts_do_not_make_a_talk() {
    let b = Box_::new("kinds");
    let v = validator();
    b.kind("news@shop.example", "service");
    b.kind("bad@evil.example", "hostile");
    let svc = ("Shop", "news@shop.example");
    b.put(M {
        id: "s1",
        thread: "s",
        from: svc,
        subject: "Weekly",
        ..M::default()
    });
    assert!(
        b.talk("s", NOW).is_none(),
        "a thread with only service mail"
    );

    b.put(M {
        id: "h1",
        thread: "h",
        from: ("Bad", "bad@evil.example"),
        ..M::default()
    });
    b.put(M {
        id: "h2",
        thread: "h",
        from: ("Sam", "sam@fabrikam.example"),
        ..M::default()
    });
    b.sql("UPDATE message SET hostile = '[\"spoof\"]' WHERE provider_id = 'h2'");
    assert!(
        b.talk("h", NOW).is_none(),
        "hostile sender and hostile mark"
    );

    b.put(M {
        id: "d1",
        thread: "d",
        from: ("", OWNER),
        labels: &["DRAFT"],
        ..M::default()
    });
    assert!(b.talk("d", NOW).is_none(), "a draft");
    // An IMAP draft and a copy in a Sent folder from a person, understood
    // before they got the label: no talk.
    for (id, label) in [("u1", "UNSENT"), ("c1", "SENT_COPY")] {
        b.put(M {
            id,
            thread: id,
            labels: &[label],
            ..M::default()
        });
        assert!(b.talk(id, NOW).is_none(), "{label}");
    }

    // A service message and then a person: the person is the origin, and
    // the service message counts.
    let s = b.put(M {
        id: "m1",
        thread: "m",
        from: svc,
        at: NOW - 2 * DAY,
        ..M::default()
    });
    let p = b.put(M {
        id: "m2",
        thread: "m",
        ..M::default()
    });
    let doc = b.talk("m", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["messages"], 2);
    assert_eq!(
        b.sources(doc["id"].as_str().unwrap()),
        [(s, "update".into()), (p, "origin".into())]
    );

    // A stranger and a sender with no kind yet make a talk.
    b.kind("who@x.example", "stranger");
    b.put(M {
        id: "x1",
        thread: "x",
        from: ("Who", "who@x.example"),
        ..M::default()
    });
    check(&v, &b.talk("x", NOW).unwrap());
}

#[test]
fn new_quiet_gone_and_back() {
    let b = Box_::new("status");
    let v = validator();
    let m1 = b.put(M {
        id: "a",
        at: NOW - 30 * DAY,
        ..M::default()
    });
    let m2 = b.put(M {
        id: "b",
        at: NOW - 20 * DAY,
        ..M::default()
    });
    let t = b.thread("t1");
    b.sql(&format!(
        "UPDATE thread SET viewed_at = {} WHERE id = {t}",
        NOW - 25 * DAY
    ));
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["new"], 1);
    assert_eq!(doc["status"], "quiet");
    assert_eq!(
        b.talk("t1", NOW - 20 * DAY + QUIET_MS - 1).unwrap()["status"],
        "active"
    );
    let id = doc["id"].as_str().unwrap().to_string();

    // The origin is gone: it stays the origin.
    b.sql(&format!(
        "UPDATE message SET gone_at = {NOW} WHERE id = {m1}"
    ));
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["id"], id.as_str());
    assert_eq!(doc["messages"], 1);
    assert_eq!(doc["sources"][0]["message"], format!("m-{m1}"));
    assert_eq!(doc["sources"][0]["role"], "origin");

    // Each message is gone: the conversation is gone, not deleted.
    b.sql(&format!(
        "UPDATE message SET gone_at = {NOW} WHERE id = {m2}"
    ));
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["status"], "gone");
    assert_eq!(doc["id"], id.as_str());

    // A message comes back, and a new one comes. A message that comes
    // back waits for the understanding again (store migration 64), so the
    // test marks it understood, as the next run does.
    b.sql(&format!(
        "UPDATE message SET gone_at = NULL WHERE id = {m2}"
    ));
    b.sql(&format!(
        "UPDATE message SET understood_at = {NOW} WHERE id = {m2}"
    ));
    b.put(M {
        id: "c",
        at: NOW - DAY,
        ..M::default()
    });
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["status"], "active");
    assert_eq!(doc["messages"], 2);
    assert_eq!(doc["id"], id.as_str());

    // A muted conversation stays muted, also through gone.
    b.sql("UPDATE item SET status = 'muted', doc = json_set(doc, '$.status', 'muted')");
    b.put(M {
        id: "d",
        at: NOW - 1000,
        ..M::default()
    });
    assert_eq!(b.talk("t1", NOW).unwrap()["status"], "muted");
    b.sql(&format!("UPDATE message SET gone_at = {NOW}"));
    assert_eq!(b.talk("t1", NOW).unwrap()["status"], "gone");
    b.sql("UPDATE message SET gone_at = NULL");
    b.sql(&format!("UPDATE message SET understood_at = {NOW}"));
    assert_eq!(b.talk("t1", NOW).unwrap()["status"], "muted");
}

#[test]
fn the_current_message_counts_before_it_is_understood() {
    let b = Box_::new("current");
    let m = b.put(M {
        id: "a",
        ..M::default()
    });
    b.sql("UPDATE message SET understood_at = NULL");
    assert!(b.talk("t1", NOW).is_none());
    let t = b.thread("t1");
    let id = b
        .store
        .write(|tx| Ok(update(tx, t, Some(m), NOW)?))
        .unwrap();
    assert!(id.is_some());
}

/// Each thread of a synthetic mailbox. Headers and clean text come from the
/// raw messages, and the sender kinds from the truth of the mailbox.
#[test]
fn synthetic_mailbox() {
    let b = Box_::new("synth");
    let v = validator();
    let mb = Mailbox::new(7, 1500);
    let mut threads = std::collections::BTreeMap::<u32, Vec<Sender>>::new();
    let mut batch = Vec::new();
    for i in 0..mb.len() {
        let t = mb.truth(i);
        let raw = mb.raw(i);
        let head =
            String::from_utf8_lossy(&raw[..raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap()])
                .into_owned();
        let get = |n: &str| {
            head.split("\r\n")
                .find_map(|l| l.strip_prefix(n))
                .map(str::to_string)
        };
        let from = get("From: ").unwrap();
        let addr = from
            .rsplit('<')
            .next()
            .unwrap()
            .trim_end_matches('>')
            .to_lowercase();
        let kind = match t.sender {
            Sender::Owner => None,
            Sender::KnownPerson => Some("person"),
            Sender::KnownService => Some("service"),
            Sender::Stranger => Some("stranger"),
            Sender::Hostile => Some("hostile"),
        };
        if let Some(k) = kind {
            b.kind(&addr, k);
        }
        let clean = iris_clean::clean(&raw);
        threads.entry(t.thread).or_default().push(t.sender);
        batch.push(NewMessage {
            provider_id: t.message_id.clone(),
            thread_provider_id: format!("th{}", t.thread),
            internal_date: Some(t.date * 1000),
            labels: vec![if t.sender == Sender::Owner {
                "SENT"
            } else {
                "INBOX"
            }
            .into()],
            header: Header {
                from_addr: Some(addr.clone()),
                to_addrs: get("To: "),
                subject: Some(clean.subject.clone()),
                ..Header::default()
            },
            body: Some(Body {
                subject: clean.subject,
                sender: addr,
                text: clean.text,
            }),
            ..NewMessage::default()
        });
    }
    b.store.put_messages(b.account, &batch).unwrap();
    b.sql("UPDATE message SET understood_at = 1");

    let (mut talks, mut replies) = (0, 0);
    for (t, senders) in &threads {
        let doc = b.talk(&format!("th{t}"), NOW);
        let counted = senders.iter().filter(|s| **s != Sender::Hostile).count();
        let talk = senders
            .iter()
            .any(|s| matches!(s, Sender::Owner | Sender::KnownPerson | Sender::Stranger));
        assert_eq!(doc.is_some(), talk, "thread {t}: {senders:?}");
        if let Some(doc) = doc {
            check(&v, &doc);
            assert_eq!(doc["messages"], counted);
            assert_eq!(doc["role"], "direct");
            assert_eq!(doc["status"], "quiet", "the mailbox ends in 2025");
            talks += 1;
            if counted > 1 {
                replies += 1;
            }
        }
    }
    assert!(
        talks > 100 && replies > 50,
        "{talks} talks, {replies} with replies"
    );
}

#[test]
fn only_service_mail_left_makes_the_talk_gone() {
    let b = Box_::new("svcgone");
    let v = validator();
    b.kind("news@shop.example", "service");
    let p = b.put(M {
        id: "p",
        at: NOW - 2 * DAY,
        ..M::default()
    });
    b.put(M {
        id: "s",
        from: ("Shop", "news@shop.example"),
        ..M::default()
    });
    let doc = b.talk("t1", NOW).unwrap();
    assert_eq!(doc["status"], "active");
    b.sql(&format!(
        "UPDATE message SET hostile = '[\"spoof\"]' WHERE id = {p}"
    ));
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["status"], "gone");
    assert_eq!(
        doc["sources"][0]["message"],
        format!("m-{p}"),
        "the origin stays"
    );
    b.sql(&format!("UPDATE message SET hostile = '[]' WHERE id = {p}"));
    assert_eq!(b.talk("t1", NOW).unwrap()["status"], "active");
}

#[test]
fn a_correction_stays_after_the_next_message() {
    let b = Box_::new("correct");
    let v = validator();
    b.put(M {
        id: "a",
        at: NOW - 2 * DAY,
        ..M::default()
    });
    let doc = b.talk("t1", NOW).unwrap();
    let id: i64 = doc["id"].as_str().unwrap()[2..].parse().unwrap();
    b.sql(&format!(
        "INSERT INTO correction(item_id, field, value, by, at) VALUES ({id}, 'summary', '\"Dinner plan\"', 'person', 1)"
    ));
    b.put(M {
        id: "b",
        ..M::default()
    });
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["messages"], 2);
    assert_eq!(doc["summary"], "Dinner plan");
}

#[test]
fn an_address_in_a_display_name_is_no_party() {
    let b = Box_::new("dname");
    b.put(M {
        id: "a",
        to: "\"billing@acme.example\" <owner@example.com>",
        ..M::default()
    });
    let doc = b.talk("t1", NOW).unwrap();
    assert_eq!(doc["role"], "direct");
    assert_eq!(doc["participants"].as_array().unwrap().len(), 2);
    assert_eq!(doc["your_turn"], true);
}

#[test]
fn the_document_follows_a_gone_status_from_the_sync() {
    let b = Box_::new("syncgone");
    let v = validator();
    let m = b.put(M {
        id: "a",
        ..M::default()
    });
    assert_eq!(b.talk("t1", NOW).unwrap()["status"], "active");
    // The same writes as the sync when the message is gone (section 4.5).
    b.sql(&format!(
        "UPDATE message SET gone_at = {NOW} WHERE id = {m}"
    ));
    b.sql("UPDATE item SET status_before_gone = status, status = 'gone' WHERE status <> 'gone'");
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["status"], "gone");
    b.sql("UPDATE message SET gone_at = NULL");
    // The message waits for the understanding again (store migration 64).
    b.sql(&format!("UPDATE message SET understood_at = {NOW}"));
    b.sql("UPDATE item SET status = status_before_gone, status_before_gone = NULL WHERE status = 'gone'");
    assert_eq!(b.talk("t1", NOW).unwrap()["status"], "active");
}

#[test]
fn a_from_that_is_not_an_address_is_no_party() {
    let b = Box_::new("badfrom");
    let v = validator();
    b.put(M {
        id: "a",
        from: ("Sam", "not-an-address"),
        ..M::default()
    });
    assert!(b.talk("t1", NOW).is_none());
    b.put(M {
        id: "b",
        from: ("Sam", "Sam <SAM@Fabrikam.example>"),
        ..M::default()
    });
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["messages"], 1);
    assert_eq!(doc["participants"][1]["address"], "sam@fabrikam.example");
}

#[test]
fn latest_keeps_the_milliseconds() {
    let b = Box_::new("millis");
    let v = validator();
    b.put(M {
        id: "a",
        at: NOW - 900,
        ..M::default()
    });
    b.put(M {
        id: "b",
        thread: "t2",
        at: NOW - 100,
        ..M::default()
    });
    let (one, two) = (b.talk("t1", NOW).unwrap(), b.talk("t2", NOW).unwrap());
    check(&v, &one);
    let (a, b) = (
        one["latest"]["at"].as_str().unwrap(),
        two["latest"]["at"].as_str().unwrap(),
    );
    assert_eq!(a, "2026-11-15T23:59:59.100Z");
    assert!(a < b, "{a} sorts before {b}");
}

#[test]
fn a_gone_update_stays_a_source() {
    let b = Box_::new("goneupd");
    let v = validator();
    let m1 = b.put(M {
        id: "a",
        at: NOW - 3 * DAY,
        ..M::default()
    });
    let m2 = b.put(M {
        id: "b",
        at: NOW - 2 * DAY,
        ..M::default()
    });
    let m3 = b.put(M {
        id: "c",
        at: NOW - DAY,
        ..M::default()
    });
    b.talk("t1", NOW).unwrap();
    b.sql(&format!(
        "UPDATE message SET gone_at = {NOW} WHERE id = {m2}"
    ));
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["messages"], 2);
    let ids: Vec<&str> = doc["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["message"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [format!("m-{m1}"), format!("m-{m2}"), format!("m-{m3}")]
    );
    let id = doc["id"].as_str().unwrap();
    assert_eq!(
        b.sources(id),
        [
            (m1, "origin".into()),
            (m2, "update".into()),
            (m3, "update".into())
        ]
    );
}

#[test]
fn the_kind_comes_from_the_parsed_from_address() {
    let b = Box_::new("kindparse");
    b.kind("news@shop.example", "service");
    b.kind("bad@evil.example", "hostile");
    b.put(M {
        id: "s",
        thread: "s",
        from: ("", "Shop <News@Shop.example>"),
        ..M::default()
    });
    assert!(
        b.talk("s", NOW).is_none(),
        "service mail with a display name in From"
    );
    b.put(M {
        id: "h",
        thread: "h",
        from: ("", "\"Bad\" <bad@evil.example>"),
        ..M::default()
    });
    assert!(
        b.talk("h", NOW).is_none(),
        "hostile mail with a display name in From"
    );
}

#[test]
fn a_quoted_local_part_counts() {
    let b = Box_::new("quotedlocal");
    let v = validator();
    b.put(M {
        id: "a",
        // In `<>`: mail-parser 1.0.1 reads a bare quoted local part as a name (iris-people).
        from: ("", "<\"a,b\"@example.org>"),
        ..M::default()
    });
    let doc = b.talk("t1", NOW).expect("a conversation");
    check(&v, &doc);
    assert_eq!(doc["participants"][1]["address"], "\"a,b\"@example.org");
}

#[test]
fn a_gone_talk_drops_hostile_updates() {
    let b = Box_::new("gonehostile");
    let v = validator();
    b.kind("news@shop.example", "service");
    let p1 = b.put(M {
        id: "a",
        at: NOW - 3 * DAY,
        text: "First",
        ..M::default()
    });
    let s = b.put(M {
        id: "s",
        at: NOW - 2 * DAY,
        from: ("Shop", "news@shop.example"),
        text: "Sale",
        ..M::default()
    });
    let p2 = b.put(M {
        id: "b",
        at: NOW - DAY,
        text: "Second",
        ..M::default()
    });
    assert_eq!(b.talk("t1", NOW).unwrap()["messages"], 3);
    b.sql(&format!(
        "UPDATE message SET hostile = '[\"spoof\"]' WHERE id IN ({p1}, {p2})"
    ));
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["status"], "gone");
    assert_eq!(doc["messages"], 1);
    assert_eq!(doc["latest"]["snippet"], "Sale");
    let id = doc["id"].as_str().unwrap();
    assert_eq!(b.sources(id), [(p1, "origin".into()), (s, "update".into())]);
}

#[test]
fn encoded_headers_are_decoded() {
    let b = Box_::new("encoded");
    let v = validator();
    b.put(M {
        id: "a",
        subject: "Café on Friday",
        ..M::default()
    });
    // The header row keeps the encoded words; the clean text has the subject.
    b.sql(
        "UPDATE header SET subject = '=?UTF-8?Q?Caf=C3=A9_on_Friday?=', from_name = '=?UTF-8?Q?Ren=C3=A9e_Bo?='",
    );
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["title"], "Café on Friday");
    assert_eq!(doc["sources"][0]["quote"], "Café on Friday");
    assert_eq!(doc["participants"][1]["name"], "Renée Bo");
}

#[test]
fn a_gone_origin_with_a_live_service_update_keeps_the_status() {
    let b = Box_::new("goneorigin");
    let v = validator();
    b.kind("news@shop.example", "service");
    let p = b.put(M {
        id: "a",
        at: NOW - 2 * DAY,
        ..M::default()
    });
    b.put(M {
        id: "s",
        from: ("Shop", "news@shop.example"),
        ..M::default()
    });
    b.talk("t1", NOW).unwrap();
    b.sql(&format!(
        "UPDATE message SET gone_at = {NOW} WHERE id = {p}"
    ));
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(
        doc["status"], "active",
        "section 4.5: only a part of the sources is gone"
    );
    assert_eq!(doc["sources"][0]["message"], format!("m-{p}"));
}

#[test]
fn a_received_message_from_an_alias_is_from_a_party() {
    let b = Box_::new("aliasin");
    let v = validator();
    let alias = ("Avel", "avel@alias.example");
    b.put(M {
        id: "a",
        at: NOW - 2 * DAY,
        from: alias,
        to: "sam@fabrikam.example",
        labels: &["SENT"],
        ..M::default()
    });
    b.put(M {
        id: "b",
        from: alias,
        to: OWNER,
        ..M::default()
    });
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["latest"]["from"]["address"], "avel@alias.example");
}

#[test]
fn cleanup_of_a_gone_service_header_does_not_bring_a_talk_back() {
    let b = Box_::new("purged");
    let v = validator();
    b.kind("news@shop.example", "service");
    let svc = ("Shop", "news@shop.example");
    let p = b.put(M {
        id: "a",
        at: NOW - 3 * DAY,
        ..M::default()
    });
    let s1 = b.put(M {
        id: "s1",
        at: NOW - 2 * DAY,
        from: svc,
        ..M::default()
    });
    b.talk("t1", NOW).unwrap();
    b.sql(&format!(
        "UPDATE message SET hostile = '[\"spoof\"]' WHERE id = {p}"
    ));
    b.sql(&format!(
        "UPDATE message SET gone_at = {NOW} WHERE id = {s1}"
    ));
    assert_eq!(b.talk("t1", NOW).unwrap()["status"], "gone");
    // The content cleanup removes the header and the body of the gone message.
    b.sql(&format!(
        "DELETE FROM body WHERE message_id = {s1}; DELETE FROM header WHERE message_id = {s1}"
    ));
    b.put(M {
        id: "s2",
        from: svc,
        ..M::default()
    });
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["status"], "gone");
}

#[test]
fn a_gone_update_with_a_hostile_mark_is_no_source() {
    let b = Box_::new("gonehostileupd");
    let v = validator();
    let m1 = b.put(M {
        id: "a",
        at: NOW - 3 * DAY,
        ..M::default()
    });
    let m2 = b.put(M {
        id: "b",
        at: NOW - 2 * DAY,
        ..M::default()
    });
    let m3 = b.put(M {
        id: "c",
        at: NOW - DAY,
        ..M::default()
    });
    b.talk("t1", NOW).unwrap();
    b.sql(&format!(
        "UPDATE message SET gone_at = {NOW}, hostile = '[\"spam\"]' WHERE id = {m2}"
    ));
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    let id = doc["id"].as_str().unwrap();
    assert_eq!(
        b.sources(id),
        [(m1, "origin".into()), (m3, "update".into())]
    );
    assert_eq!(doc["sources"].as_array().unwrap().len(), 2);
}

#[test]
fn a_gone_origin_from_a_sender_that_became_hostile_ends_the_talk() {
    let b = Box_::new("gonehostilesender");
    let v = validator();
    b.kind("news@shop.example", "service");
    let p = b.put(M {
        id: "a",
        at: NOW - 2 * DAY,
        ..M::default()
    });
    b.put(M {
        id: "s",
        from: ("Shop", "news@shop.example"),
        ..M::default()
    });
    b.talk("t1", NOW).unwrap();
    b.sql(&format!(
        "UPDATE message SET gone_at = {NOW} WHERE id = {p}"
    ));
    b.kind("sam@fabrikam.example", "hostile");
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["status"], "gone");
    assert_eq!(
        doc["sources"][0]["message"],
        format!("m-{p}"),
        "the origin stays a source"
    );
}

#[test]
fn an_empty_mark_list_with_spaces_is_no_mark() {
    let b = Box_::new("emptymarks");
    let m1 = b.put(M {
        id: "a",
        at: NOW - 3 * DAY,
        ..M::default()
    });
    let m2 = b.put(M {
        id: "b",
        at: NOW - 2 * DAY,
        ..M::default()
    });
    let m3 = b.put(M {
        id: "c",
        at: NOW - DAY,
        ..M::default()
    });
    b.sql(&format!(
        "UPDATE message SET hostile = '[ ]' WHERE id = {m2}"
    ));
    b.talk("t1", NOW).unwrap();
    b.sql(&format!(
        "UPDATE message SET gone_at = {NOW} WHERE id = {m2}"
    ));
    let doc = b.talk("t1", NOW).unwrap();
    let id = doc["id"].as_str().unwrap();
    assert_eq!(
        b.sources(id),
        [
            (m1, "origin".into()),
            (m2, "update".into()),
            (m3, "update".into())
        ]
    );
}

#[test]
fn a_live_origin_that_becomes_hostile_gives_way_to_the_next_message() {
    let b = Box_::new("hostileorigin");
    let v = validator();
    let a = b.put(M {
        id: "a",
        at: NOW - 2 * DAY,
        subject: "First",
        ..M::default()
    });
    let r = b.put(M {
        id: "b",
        subject: "Re: First",
        ..M::default()
    });
    b.talk("t1", NOW).unwrap();
    b.sql(&format!(
        "UPDATE message SET hostile = '[\"spoof\"]' WHERE id = {a}"
    ));
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["status"], "active");
    assert_eq!(doc["messages"], 1);
    assert_eq!(doc["title"], "Re: First");
    assert_eq!(doc["sources"].as_array().unwrap().len(), 1);
    assert_eq!(doc["sources"][0]["message"], format!("m-{r}"));
    assert_eq!(
        b.sources(doc["id"].as_str().unwrap()),
        [(r, "origin".into())]
    );
}

#[test]
fn a_reply_to_an_alias_is_your_turn() {
    let b = Box_::new("aliasturn");
    let v = validator();
    let alias = ("Avel", "avel@alias.example");
    b.put(M {
        id: "a",
        at: NOW - 2 * DAY,
        from: alias,
        to: "sam@fabrikam.example",
        labels: &["SENT"],
        ..M::default()
    });
    b.put(M {
        id: "b",
        to: "avel@alias.example",
        ..M::default()
    });
    let doc = b.talk("t1", NOW).unwrap();
    check(&v, &doc);
    assert_eq!(doc["your_turn"], true);
    assert_eq!(doc["role"], "direct");
}

/// Issue 106: each change of a sender kind marks the threads with mail
/// from that address (store migration 40), and `update_due` computes their
/// conversations again. So a thread of a service gets its conversation when
/// the address becomes a person, also when no message of the thread is
/// understood again, and loses it when the address becomes a service.
#[test]
fn a_kind_change_updates_the_talk_of_each_thread_of_the_sender() {
    let b = Box_::new("kind-change");
    let svc = ("Ann", "ann@shop.example");
    b.kind(svc.1, "service");
    for (id, thread) in [("k1", "k"), ("l1", "l")] {
        b.put(M {
            id,
            thread,
            from: svc,
            ..M::default()
        });
    }
    let due = |b: &Box_| -> usize {
        b.store
            .write(|tx| Ok(iris_talk::update_due(tx, NOW, 100)?))
            .unwrap()
    };
    let talks = |b: &Box_| -> i64 {
        b.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT count(*) FROM item WHERE form = 'conversation' AND status <> 'gone'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    };
    due(&b);
    assert_eq!(talks(&b), 0, "service mail makes no talk");
    // An update of the kind, an insert and a delete of the row: each one
    // marks both threads.
    for (step, sql, want) in [
        (
            "person",
            "UPDATE sender SET kind = 'person' WHERE address = 'ann@shop.example'",
            2,
        ),
        (
            "service",
            "UPDATE sender SET kind = 'service' WHERE address = 'ann@shop.example'",
            0,
        ),
        (
            "delete",
            "DELETE FROM sender WHERE address = 'ann@shop.example'",
            2,
        ),
        (
            // The talk made a party row with no kind: its delete marks nothing.
            "insert",
            "DELETE FROM sender WHERE address = 'ann@shop.example';
             INSERT INTO sender(address, kind) VALUES ('ann@shop.example', 'service')",
            0,
        ),
    ] {
        b.sql(sql);
        assert_eq!(due(&b), 2, "{step}: two threads were due");
        assert_eq!(talks(&b), want, "{step}");
        assert_eq!(due(&b), 0, "{step}: the queue is empty");
    }
}
