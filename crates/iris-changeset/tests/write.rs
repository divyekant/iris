//! The write verbs `draft` and `send` at submit
//! (docs/specs/2026-11-16-send.md, sections 4 and 5).

use std::fs;
use std::path::PathBuf;

use iris_changeset::{add_action, create, submit, write_document, Author, AuthorKind, Verb};
use iris_store::{Header, NewMessage, Store};
use serde_json::{json, Map, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-cw-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const NOW: i64 = 1_763_283_422_000;

fn author(kind: AuthorKind) -> Author {
    Author {
        kind,
        name: Some("t".into()),
        session: None,
        job: None,
    }
}

/// Account 1. Thread 1: messages 1 and 2, with Message-IDs (2 is newer).
/// Thread 2 (message 3's thread): one message with no Message-ID. Draft 1, a reply in thread 1.
fn store(dir: &TempDir) -> Store {
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    let m = |id: &str, t: &str, date: i64, mid: Option<&str>| NewMessage {
        provider_id: id.into(),
        thread_provider_id: t.into(),
        internal_date: Some(date),
        header: Header {
            from_addr: Some("priya@example.com".into()),
            subject: Some("Q4 numbers".into()),
            message_id_hdr: mid.map(str::to_string),
            ..Default::default()
        },
        ..Default::default()
    };
    s.put_messages(
        a,
        &[
            m("g1", "gt1", 1, Some("<a@x.org>")),
            m("g2", "gt1", 2, Some("<b@x.org>")),
            m("g3", "gt2", 3, None),
        ],
    )
    .unwrap();
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO changeset(author_kind, created_at) VALUES ('person', 0);
             INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
               VALUES (1, 1, 1, 'draft', '{\"threads\":[\"t-1\"]}', '{}');
",
        )?;
        // The draft that the apply made: its raw bytes and their hash.
        let c = iris_compose::Content {
            to: vec!["priya@example.com".into()],
            cc: Vec::new(),
            subject: "Re: Plan".into(),
            body: "Thanks".into(),
            ..Default::default()
        }
        .check()
        .unwrap();
        let raw = iris_compose::compose("owner@example.com", "iris.1@example.com", &c, None, 1);
        tx.execute(
            "INSERT INTO draft(account_id, thread_id, provider_id, message_id_hdr, content, hash,
                               raw, action_id, created_at, updated_at)
             VALUES (1, 1, 'r1', 'iris.1@example.com', '{}', ?1, ?2, 1, 0, 0)",
            rusqlite::params![iris_compose::hash(&c, None), raw],
        )?;
        Ok(())
    })
    .unwrap();
    s
}

fn args(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

/// Submit one action and give its document entry. The document agrees
/// with the changeset schema.
fn one(s: &Store, by: AuthorKind, verb: Verb, targets: &[&str], a: Value) -> Value {
    let who = author(by);
    let cs = create(s, &who, NOW).unwrap();
    add_action(s, cs, &who, verb, targets, &args(a), None).unwrap();
    submit(s, cs, &who, "Write.", NOW).unwrap();
    let mut out = Vec::new();
    write_document(s, cs, &mut out).unwrap();
    let d: Value = serde_json::from_slice(&out).unwrap();
    let errors: Vec<String> = iris_spec::validator("changeset")
        .iter_errors(&d)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    d["actions"][0].clone()
}

fn state(a: &Value) -> &str {
    a["decision"]["state"].as_str().unwrap()
}

const MAIL: fn() -> Value =
    || json!({"to": ["Priya@Example.com"], "subject": "Hi", "body": "Hello"});
const REPLY: fn() -> Value = || json!({"to": ["priya@example.com"], "body": "Thanks"});

#[test]
fn a_new_mail_and_a_reply_are_proposed() {
    let dir = TempDir::new("ok");
    let s = store(&dir);
    let a = one(&s, AuthorKind::Agent, Verb::Draft, &["a-1"], MAIL());
    assert_eq!(state(&a), "proposed", "{a}");
    assert_eq!(a["resolved"]["messages"], json!([]));
    let a = one(&s, AuthorKind::Agent, Verb::Draft, &["t-1"], REPLY());
    assert_eq!(state(&a), "proposed", "{a}");
    // A reply resolves to its parent only; the hostile check reads the thread.
    assert_eq!(a["resolved"]["messages"], json!(["m-2"]));
    let a = one(&s, AuthorKind::Agent, Verb::Send, &["d-1"], json!({}));
    assert_eq!(state(&a), "proposed", "{a}");
}

#[test]
fn submit_denies_a_draft_that_is_not_valid() {
    let dir = TempDir::new("bad");
    let s = store(&dir);
    // Upserts of a thread can skip row IDs, so read the ID of thread 2.
    let t2: i64 = s
        .read(|c| {
            Ok(
                c.query_row("SELECT thread_id FROM message WHERE id = 3", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    let t2 = format!("t-{t2}");
    let both = ["t-1", t2.as_str()];
    let cases: Vec<(&[&str], Value, &str)> = vec![
        (&["a-1"], json!({"to": ["p@x.org"], "body": "x"}), "subject"),
        (
            &["a-1"],
            json!({"to": ["p x@x.org"], "subject": "s", "body": "x"}),
            "address",
        ),
        (
            &["a-1"],
            json!({"to": [], "subject": "s", "body": "x"}),
            "to",
        ),
        (
            &["a-1"],
            json!({"to": ["p@x.org"], "subject": "s", "body": "x", "reply_to": []}),
            "draft takes",
        ),
        (
            &["a-1"],
            json!({"to": ["p@x.org"], "subject": "a\nBcc: q@x.org", "body": "x"}),
            "control",
        ),
        (&["a-9"], MAIL(), "not in the mirror"),
        (
            &["t-1"],
            json!({"to": ["p@x.org"], "subject": "s", "body": "x"}),
            "no subject",
        ),
        (&["m-1"], REPLY(), "one account or one thread"),
        // A draft change waits for its undo (spec 2, section 7.5).
        (&["d-1"], REPLY(), "one account or one thread"),
        (&both, REPLY(), "one target"),
        // No mail with no text, from any author (issue 741).
        (
            &["a-1"],
            json!({"to": ["p@x.org"], "subject": "s", "body": ""}),
            "needs a body",
        ),
        (
            &["t-1"],
            json!({"to": ["p@x.org"], "body": " \n\t"}),
            "needs a body",
        ),
    ];
    for (targets, a, why) in cases {
        let got = one(&s, AuthorKind::Agent, Verb::Draft, targets, a.clone());
        assert_eq!(state(&got), "denied", "{targets:?} {a}");
        let reason = got["decision"]["reason"].as_str().unwrap();
        assert!(reason.contains(why), "{targets:?} {a}: {reason}");
    }
    for (targets, a) in [(&["d-1"][..], json!({"x": 1})), (&["t-1"], json!({}))] {
        let got = one(&s, AuthorKind::Agent, Verb::Send, targets, a.clone());
        assert_eq!(state(&got), "denied", "{targets:?} {a}");
    }
}

#[test]
fn only_a_person_writes_into_a_hostile_thread() {
    let dir = TempDir::new("hostile");
    let s = store(&dir);
    // An empty mark list (understanding found no mark) is not hostile.
    for empty in ["[]", ""] {
        s.write(|tx| Ok(tx.execute("UPDATE message SET hostile = ?1 WHERE id = 1", [empty])?))
            .unwrap();
        for (verb, t, a) in [
            (Verb::Draft, "t-1", REPLY()),
            (Verb::Send, "d-1", json!({})),
        ] {
            let got = one(&s, AuthorKind::Agent, verb, &[t], a);
            assert_eq!(state(&got), "proposed", "{verb:?} {empty:?}");
            // Withdraw it, so the send of the draft is free again.
            s.write(|tx| {
                Ok(tx.execute(
                    "UPDATE changeset_action SET state = 'denied', decided_by = 'engine',
                       reason = 'test', decided_at = 1 WHERE state = 'proposed'",
                    [],
                )?)
            })
            .unwrap();
        }
    }
    // A message that waits for its link check (migration 20) counts as
    // hostile mail, also with an empty mark list.
    s.write(|tx| {
        tx.execute("DELETE FROM link_check WHERE message_id = 1", [])?;
        Ok(tx.execute("INSERT INTO link_check(message_id) VALUES (1)", [])?)
    })
    .unwrap();
    for (verb, t, a) in [
        (Verb::Draft, "t-1", REPLY()),
        (Verb::Send, "d-1", json!({})),
    ] {
        let got = one(&s, AuthorKind::Agent, verb, &[t], a);
        assert_eq!(state(&got), "denied", "{verb:?} waits for the link check");
    }
    s.write(|tx| Ok(tx.execute("DELETE FROM link_check WHERE message_id = 1", [])?))
        .unwrap();
    s.write(|tx| Ok(tx.execute("UPDATE message SET hostile = '[\"link\"]' WHERE id = 1", [])?))
        .unwrap();
    for verb in [Verb::Draft, Verb::Send] {
        let (t, a) = if verb == Verb::Draft {
            ("t-1", REPLY())
        } else {
            ("d-1", json!({}))
        };
        let got = one(&s, AuthorKind::Agent, verb, &[t], a.clone());
        assert_eq!(state(&got), "denied", "{verb:?}");
        assert!(got["decision"]["reason"]
            .as_str()
            .unwrap()
            .contains("hostile"));
        assert_eq!(
            state(&one(&s, AuthorKind::Person, verb, &[t], a)),
            "proposed"
        );
    }
    // A gone message after the content cleanup has no header: its sender
    // is not known, so an agent cannot write into the thread.
    s.write(|tx| {
        tx.execute("UPDATE message SET hostile = NULL WHERE id = 1", [])?;
        tx.execute("UPDATE message SET gone_at = 1 WHERE id = 1", [])?;
        Ok(tx.execute("DELETE FROM header WHERE message_id = 1", [])?)
    })
    .unwrap();
    for (verb, t, a) in [
        (Verb::Draft, "t-1", REPLY()),
        (Verb::Send, "d-1", json!({})),
    ] {
        let got = one(&s, AuthorKind::Agent, verb, &[t], a);
        assert_eq!(state(&got), "denied", "{verb:?}");
    }
    s.write(|tx| Ok(tx.execute("UPDATE message SET hostile = '[\"link\"]' WHERE id = 1", [])?))
        .unwrap();
    // Each message of the thread is gone: the send still reads the thread
    // of its draft, and the gone hostile message counts.
    s.write(|tx| Ok(tx.execute("UPDATE message SET gone_at = 1 WHERE thread_id = 1", [])?))
        .unwrap();
    let got = one(&s, AuthorKind::Agent, Verb::Send, &["d-1"], json!({}));
    assert_eq!(state(&got), "denied");
    assert!(got["decision"]["reason"]
        .as_str()
        .unwrap()
        .contains("hostile"));
}

/// One draft has at most one send that can go out.
#[test]
fn a_draft_has_one_send() {
    let dir = TempDir::new("one");
    let s = store(&dir);
    // Two sends in one revision: the second is denied.
    let who = author(AuthorKind::Agent);
    let cs = create(&s, &who, NOW).unwrap();
    add_action(&s, cs, &who, Verb::Send, &["d-1"], &Map::new(), None).unwrap();
    add_action(&s, cs, &who, Verb::Send, &["d-1"], &Map::new(), None).unwrap();
    submit(&s, cs, &who, "Send.", NOW).unwrap();
    let st = |n: i64| -> String {
        s.read(|c| {
            Ok(c.query_row(
                "SELECT state FROM changeset_action WHERE changeset_id = ?1 AND n = ?2",
                [cs, n],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    };
    assert_eq!((st(1).as_str(), st(2).as_str()), ("proposed", "denied"));
    // An open send of another changeset blocks a send.
    assert_eq!(
        state(&one(&s, AuthorKind::Agent, Verb::Send, &["d-1"], json!({}))),
        "denied"
    );
    // A sent draft, a foreign draft and a send claim block too.
    for set in [
        "UPDATE changeset_action SET state = 'denied', decided_by = 'engine', reason = 'no', decided_at = 1 WHERE verb = 'send' AND state = 'proposed'",
        "UPDATE draft SET sent_at = 1, sent_message = 'g9'",
        "UPDATE draft SET sent_at = NULL, sent_message = NULL, foreign_at = 1",
    ] {
        s.write(|tx| Ok(tx.execute(set, [])?)).unwrap();
        if set.contains("draft SET") {
            assert_eq!(state(&one(&s, AuthorKind::Person, Verb::Send, &["d-1"], json!({}))), "denied", "{set}");
        }
    }
    s.write(|tx| Ok(tx.execute("UPDATE draft SET foreign_at = NULL", [])?))
        .unwrap();
    assert_eq!(
        state(&one(
            &s,
            AuthorKind::Person,
            Verb::Send,
            &["d-1"],
            json!({})
        )),
        "proposed"
    );
}

/// The hash of a send covers the content hash of its draft.
#[test]
fn the_send_hash_covers_the_draft_content() {
    let dir = TempDir::new("hash");
    let s = store(&dir);
    let a = one(&s, AuthorKind::Person, Verb::Send, &["d-1"], json!({}));
    let id: i64 = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT max(id) FROM changeset_action WHERE verb = 'send'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    let before = s
        .read(|c| Ok(iris_changeset::hash(c, id).unwrap().1))
        .unwrap();
    assert_eq!(a["hash"], json!(before));
    s.write(|tx| Ok(tx.execute("UPDATE draft SET hash = 'h2'", [])?))
        .unwrap();
    let after = s
        .read(|c| Ok(iris_changeset::hash(c, id).unwrap().1))
        .unwrap();
    assert_ne!(before, after);
}

/// The parent of a reply is the newest message that submit resolved. A
/// message that comes into the thread later is never the parent, and a
/// parent that is gone later shows as gone.
#[test]
fn the_parent_of_a_reply_is_fixed_at_submit() {
    let dir = TempDir::new("parent");
    let s = store(&dir);
    one(&s, AuthorKind::Person, Verb::Draft, &["t-1"], REPLY());
    let id: i64 = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT max(id) FROM changeset_action WHERE verb = 'draft'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    let account = 1;
    s.put_messages(
        account,
        &[NewMessage {
            provider_id: "g4".into(),
            thread_provider_id: "gt1".into(),
            internal_date: Some(9),
            header: Header {
                subject: Some("Other".into()),
                message_id_hdr: Some("<new@x.org>".into()),
                ..Default::default()
            },
            ..Default::default()
        }],
    )
    .unwrap();
    let p = s
        .read(|c| Ok(iris_changeset::parent(c, id).unwrap().unwrap()))
        .unwrap();
    assert_eq!(
        (p.message, p.gone, p.message_id.as_deref()),
        (2, false, Some("<b@x.org>"))
    );
    s.write(|tx| Ok(tx.execute("UPDATE message SET gone_at = 1 WHERE id = 2", [])?))
        .unwrap();
    let p = s
        .read(|c| Ok(iris_changeset::parent(c, id).unwrap().unwrap()))
        .unwrap();
    assert_eq!((p.message, p.gone), (2, true));
}

/// The flags of a write action read the same threads as the hostile check:
/// the threads of its messages and the thread of its draft. A send whose
/// thread has only gone messages still flags a stranger (issue #346).
#[test]
fn a_stranger_thread_flags_first_contact_with_gone_messages() {
    let dir = TempDir::new("first");
    let s = store(&dir);
    s.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO sender(address, kind) VALUES ('priya@example.com', 'stranger')",
            [],
        )?)
    })
    .unwrap();
    // A stranger: the owner never wrote to priya, so new_recipient too.
    let flags = |a: &Value| a["flags"].clone();
    let got = one(&s, AuthorKind::Person, Verb::Draft, &["t-1"], REPLY());
    assert_eq!(
        flags(&got),
        json!(["first_contact", "new_recipient"]),
        "{got}"
    );
    s.write(|tx| Ok(tx.execute("UPDATE message SET gone_at = 1 WHERE thread_id = 1", [])?))
        .unwrap();
    let got = one(&s, AuthorKind::Person, Verb::Send, &["d-1"], json!({}));
    assert_eq!(state(&got), "proposed", "{got}");
    // The send flags the raw bytes of its draft: priya is new.
    assert_eq!(
        flags(&got),
        json!(["first_contact", "new_recipient"]),
        "{got}"
    );
    // A new mail has no thread, so no thread can make it a first contact.
    let got = one(&s, AuthorKind::Person, Verb::Draft, &["a-1"], MAIL());
    assert_eq!(flags(&got), json!(["new_recipient"]), "{got}");
}
