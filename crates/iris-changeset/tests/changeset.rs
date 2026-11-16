use std::fs;
use std::path::PathBuf;

use iris_changeset::{
    add_action, approve, create, decide, revise, submit, write_document, ActionId, Author,
    AuthorKind, By, Error, State, Verb, BULK,
};
use iris_store::{NewMessage, Store};
use serde_json::{json, Map, Value};

/// A new empty folder for one test, removed at the end.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-cs-{name}-{}", std::process::id()));
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

fn agent() -> Author {
    Author {
        kind: AuthorKind::Agent,
        name: Some("Claude Code".into()),
        session: Some("se-12".into()),
        job: Some("j-142".into()),
    }
}

/// A store with thread 1 (messages 1 and 2, and message 3 in the trash),
/// thread 2 (message 4), thread 3 (message 5, in the trash), commitment 1,
/// record 2, and policy version 1 with the rule IDs 1 to 4.
fn store(dir: &TempDir) -> Store {
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    s.put_labels(
        a,
        &[
            ("INBOX", "INBOX", "system"),
            ("TRASH", "TRASH", "system"),
            ("Label_1", "Finance", "user"),
        ],
    )
    .unwrap();
    let m = |id: &str, t: &str, labels: &[&str]| NewMessage {
        provider_id: id.into(),
        thread_provider_id: t.into(),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        ..Default::default()
    };
    s.put_messages(
        a,
        &[
            m("g1", "gt1", &["INBOX"]),
            m("g2", "gt1", &["INBOX"]),
            m("g3", "gt1", &["TRASH"]),
            m("g4", "gt2", &[]),
            m("g5", "gt3", &["TRASH"]),
        ],
    )
    .unwrap();
    s.write(|tx| {
        for form in ["commitment", "record"] {
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES (?1, ?2, 'open', '{}', 0, 0)",
                rusqlite::params![form, a],
            )?;
        }
        // Policy version 1, with the rule IDs 1 to 4.
        tx.execute_batch(
            "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('a', 'h1', 0);
             INSERT INTO policy_rule_id(id) VALUES (1), (2), (3), (4);",
        )?;
        Ok(())
    })
    .unwrap();
    s
}

fn args(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn doc(s: &Store, cs: i64) -> Value {
    let mut out = Vec::new();
    write_document(s, cs, &mut out).unwrap();
    serde_json::from_slice(&out).unwrap()
}

fn refused<T: std::fmt::Debug>(r: Result<T, Error>) {
    assert!(matches!(r, Err(Error::Refused(_))), "{r:?}");
}

/// Decide past the API, as the policy and the review do. The triggers still
/// check each move.
fn decided(s: &Store, cs: i64, set: &str) {
    let sql = if set.contains(" WHERE ") {
        format!("UPDATE changeset_action SET {set} AND changeset_id = {cs}")
    } else {
        format!("UPDATE changeset_action SET {set} WHERE changeset_id = {cs}")
    };
    s.write(|tx| Ok(tx.execute(&sql, [])?)).unwrap();
}

/// A submitted action does not change, whoever writes: the approval hash
/// covers its targets, arguments and messages.
#[test]
fn a_submitted_action_does_not_change() {
    let dir = TempDir::new("fixed");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    for sql in [
        "UPDATE changeset_action SET targets = '{\"messages\":[\"m-2\"]}'",
        "UPDATE changeset_action SET args = '{\"x\":1}'",
        "UPDATE changeset_action SET verb = 'trash'",
        "UPDATE changeset_action SET hash = 'x'",
        "UPDATE changeset_action SET flags = '[\"bulk\",\"x\"]'",
        "UPDATE changeset_action SET submitted_at = 0",
        "UPDATE action_message SET message_id = 4",
        "DELETE FROM changeset_action",
        "UPDATE changeset_action SET why = 'x'",
        "UPDATE changeset_action SET reason = 'x'",
        "UPDATE changeset_action SET decided_by = 'person'",
        "INSERT INTO action_message VALUES (1, 2)",
        "DELETE FROM action_message",
    ] {
        assert!(s.write(|tx| Ok(tx.execute(sql, [])?)).is_err(), "{sql}");
    }
    assert_eq!(
        doc(&s, cs)["actions"][0]["resolved"],
        json!({"messages": ["m-1"]})
    );
}

/// One action of the document.
fn hash_doc(s: &Store, cs: i64, revision: i64, n: i64) -> Value {
    doc(s, cs)["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["revision"] == revision && a["n"] == n)
        .unwrap()
        .clone()
}

/// The hash that a review shows, from the document.
fn hash(s: &Store, cs: i64, revision: i64, n: i64) -> String {
    doc(s, cs)["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["revision"] == revision && a["n"] == n)
        .unwrap()["hash"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn submit_denies_an_action_that_is_not_valid() {
    let dir = TempDir::new("deny");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    let cases: [(Verb, &[&str], Value); 8] = [
        (Verb::Archive, &["t-99"], json!({})),
        (Verb::Archive, &["m-9"], json!({})),
        (Verb::Archive, &["c-1"], json!({})),
        (Verb::Archive, &["t-3"], json!({})),
        (Verb::Archive, &["m-1"], json!({"label": "x"})),
        // Part 2 of R2-01 adds these verbs.
        (Verb::Label, &["m-1"], json!({"label": "Finance"})),
        (Verb::Close, &["c-1"], json!({"reason": "Done."})),
        (Verb::Untrash, &["m-3"], json!({})),
    ];
    for (verb, targets, a) in &cases {
        add_action(&s, cs, &agent(), *verb, targets, &args(a.clone()), None).unwrap();
    }
    // Message 3 is in the trash: a message target can name it.
    add_action(&s, cs, &agent(), Verb::Inbox, &["m-3"], &Map::new(), None).unwrap();
    // Rows of another writer, with targets or arguments of a wrong shape.
    s.write(|tx| {
        for (n, targets, a) in [
            (29, r#"{"threads":"t-1"}"#, "{}"),
            (30, r#"{"threads":["m-1"]}"#, "{}"),
            (31, r#"{"threads":["t-1"],"messages":["m-1"]}"#, "{}"),
            (32, r#"{"threads":["t-1"]}"#, "[]"),
            (33, r#"{"messages":[]}"#, "{}"),
            (35, r#"{"messages":["m-1"]}"#, " { } "),
            (36, r#"{"messages":["m-1"]}"#, r#""x""#),
        ] {
            tx.execute(
                "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                 VALUES (?1, 1, ?2, 'archive', ?3, ?4)",
                rusqlite::params![cs, n, targets, a],
            )?;
        }
        Ok(())
    })
    .unwrap();
    let bad_json = s.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
             VALUES (?1, 1, 40, 'archive', 'not json', '{}')",
            [cs],
        )?)
    });
    assert!(bad_json.is_err());
    submit(&s, cs, &agent(), "Tidy up.", NOW).unwrap();
    let d = doc(&s, cs);
    for a in d["actions"].as_array().unwrap() {
        let expect = if a["n"] == 9 || a["n"] == 35 {
            "proposed"
        } else {
            "denied"
        };
        assert_eq!(a["decision"]["state"], expect, "{a}");
        if expect == "denied" {
            assert_eq!(a["decision"]["by"], "engine");
            assert!(a["decision"]["reason"]
                .as_str()
                .is_some_and(|r| !r.is_empty()));
            assert_eq!(a["resolved"]["messages"], json!([]), "{a}");
        }
    }
}

#[test]
fn add_and_submit_refusals() {
    let dir = TempDir::new("refuse");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    // Only the author changes or submits its draft (section 8.2).
    let other = Author {
        session: Some("se-13".into()),
        ..agent()
    };
    refused(add_action(
        &s,
        cs,
        &other,
        Verb::Archive,
        &["m-1"],
        &Map::new(),
        None,
    ));
    refused(add_action(
        &s,
        cs,
        &agent(),
        Verb::Archive,
        &[],
        &Map::new(),
        None,
    ));
    refused(add_action(
        &s,
        cs,
        &agent(),
        Verb::Archive,
        &["t-1", "m-1"],
        &Map::new(),
        None,
    ));
    refused(add_action(
        &s,
        cs,
        &agent(),
        Verb::Archive,
        &["p-1"],
        &Map::new(),
        None,
    ));
    refused(add_action(
        &s,
        cs,
        &agent(),
        Verb::Close,
        &["c-1", "r-2"],
        &Map::new(),
        None,
    ));
    refused(add_action(
        &s,
        cs,
        &agent(),
        Verb::Archive,
        &["x-1"],
        &Map::new(),
        None,
    ));
    assert!(matches!(
        add_action(
            &s,
            999,
            &agent(),
            Verb::Archive,
            &["m-1"],
            &Map::new(),
            None
        ),
        Err(Error::NotFound)
    ));
    refused(submit(&s, cs, &agent(), "No action.", NOW));
    add_action(&s, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    refused(submit(&s, cs, &agent(), "  ", NOW));
    refused(submit(&s, cs, &other, "Archive.", NOW));
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    refused(submit(&s, cs, &agent(), "Again.", NOW));
    refused(add_action(
        &s,
        cs,
        &agent(),
        Verb::Archive,
        &["m-2"],
        &Map::new(),
        None,
    ));
    // A refused submit wrote nothing.
    assert_eq!(doc(&s, cs)["why"], "Archive.");
}

#[test]
fn bulk_flag() {
    let dir = TempDir::new("bulk");
    let s = store(&dir);
    let n = BULK as usize + 1;
    let batch: Vec<NewMessage> = (0..n)
        .map(|i| NewMessage {
            provider_id: format!("b{i}"),
            thread_provider_id: if i < 300 { "bt1".into() } else { "bt2".into() },
            ..Default::default()
        })
        .collect();
    s.put_messages(1, &batch).unwrap();
    let thread = |p: &str| -> String {
        let id: i64 = s
            .read(|c| {
                Ok(
                    c.query_row("SELECT id FROM thread WHERE provider_id = ?1", [p], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        format!("t-{id}")
    };
    let (t3, t4) = (thread("bt1"), thread("bt2"));
    let cs = create(&s, &agent(), NOW).unwrap();
    // 300 and 201 messages. Each is under the limit, the changeset is over it.
    add_action(&s, cs, &agent(), Verb::Archive, &[&t3], &Map::new(), None).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &[&t4], &Map::new(), None).unwrap();
    submit(&s, cs, &agent(), "Clean up.", NOW).unwrap();
    let d = doc(&s, cs);
    assert_eq!(d["actions"][0]["flags"], json!(["bulk"]));
    assert_eq!(d["actions"][1]["flags"], json!(["bulk"]));

    let small = create(&s, &agent(), NOW).unwrap();
    add_action(
        &s,
        small,
        &agent(),
        Verb::Archive,
        &[&t3],
        &Map::new(),
        None,
    )
    .unwrap();
    submit(&s, small, &agent(), "Clean up.", NOW).unwrap();
    assert_eq!(doc(&s, small)["actions"][0]["flags"], json!([]));

    // Three actions on the same 201 messages change 603 messages in all.
    let again = create(&s, &agent(), NOW).unwrap();
    for _ in 0..3 {
        add_action(&s, again, &agent(), Verb::Read, &[&t4], &Map::new(), None).unwrap();
    }
    submit(&s, again, &agent(), "Mark read.", NOW).unwrap();
    for a in doc(&s, again)["actions"].as_array().unwrap() {
        assert_eq!(a["flags"], json!(["bulk"]));
    }

    // An approved action of revision 1 (300 messages) counts, so revision 2
    // (201 more) is over the limit. A denied one does not count. A proposed
    // or in_review one does not count: the submit of revision 2 replaces it.
    for (end, bulk) in [
        ("state = 'approved', decided_by = 'person'", true),
        (
            "state = 'denied', decided_by = 'person', reason = 'No.'",
            false,
        ),
        ("", false),
        ("state = 'in_review', decided_by = 'policy'", false),
    ] {
        let two = create(&s, &agent(), NOW).unwrap();
        add_action(&s, two, &agent(), Verb::Archive, &[&t3], &Map::new(), None).unwrap();
        submit(&s, two, &agent(), "Archive.", NOW).unwrap();
        if !end.is_empty() {
            decided(&s, two, end);
        }
        revise(&s, two, &agent()).unwrap();
        add_action(&s, two, &agent(), Verb::Archive, &[&t4], &Map::new(), None).unwrap();
        submit(&s, two, &agent(), "And more.", NOW).unwrap();
        let flags = if bulk { json!(["bulk"]) } else { json!([]) };
        assert_eq!(hash_doc(&s, two, 2, 1)["flags"], flags, "{end}");
    }
}

#[test]
fn a_new_revision_replaces_the_open_actions() {
    let dir = TempDir::new("revision");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["m-2"], &Map::new(), None).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["m-4"], &Map::new(), None).unwrap();
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    decided(
        &s,
        cs,
        "state = 'in_review', decided_by = 'policy' WHERE n = 2",
    );
    decided(
        &s,
        cs,
        "state = 'approved', decided_by = 'person' WHERE n = 3",
    );

    refused(revise(
        &s,
        cs,
        &Author {
            session: Some("se-13".into()),
            ..agent()
        },
    ));
    assert_eq!(revise(&s, cs, &agent()).unwrap(), 2);
    refused(revise(&s, cs, &agent()));
    // Revision 2 is a draft: submit and add work on it, not on revision 1.
    refused(submit(&s, cs, &agent(), "Nothing.", NOW));
    add_action(&s, cs, &agent(), Verb::Read, &["m-1"], &Map::new(), None).unwrap();
    submit(&s, cs, &agent(), "Mark it read.", NOW + 5).unwrap();
    refused(add_action(
        &s,
        cs,
        &agent(),
        Verb::Read,
        &["m-2"],
        &Map::new(),
        None,
    ));

    let d = doc(&s, cs);
    assert_eq!(d["revision"], 2);
    assert_eq!(d["why"], "Mark it read.");
    let states: Vec<_> = d["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["revision"].as_i64().unwrap(),
                a["n"].as_i64().unwrap(),
                a["decision"]["state"].clone(),
            )
        })
        .collect();
    assert_eq!(
        states,
        [
            (1, 1, json!("replaced")),
            (1, 2, json!("replaced")),
            (1, 3, json!("approved")),
            (2, 1, json!("proposed")),
        ]
    );
    assert_eq!(d["actions"][0]["decision"]["by"], "author");
}

#[test]
fn first_contact_flag_for_a_stranger_thread() {
    let dir = TempDir::new("stranger");
    let s = store(&dir);
    s.write(|tx| {
        tx.execute_batch(
            "UPDATE header SET from_addr = 'new@example.com' WHERE message_id = 2;
             INSERT INTO sender(address, kind) VALUES ('new@example.com', 'stranger');",
        )?;
        Ok(())
    })
    .unwrap();
    let cs = create(&s, &agent(), NOW).unwrap();
    // Message 1 is in the thread of the stranger message 2.
    add_action(&s, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["m-4"], &Map::new(), None).unwrap();
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    let d = doc(&s, cs);
    assert_eq!(d["actions"][0]["flags"], json!(["first_contact"]));
    assert_eq!(d["actions"][1]["flags"], json!([]));
}

/// Fuzz test of the target rows that the product does not control: random
/// JSON shapes and IDs, written as another writer would. Submit never
/// panics, and an action is `proposed` only with one or more messages.
#[test]
fn random_target_rows() {
    let dir = TempDir::new("fuzz");
    let s = store(&dir);
    let mut x: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    let keys = ["threads", "messages", "items", "x", ""];
    let ids = [
        "t-1", "t-2", "t-3", "t-9", "m-1", "m-3", "m-5", "c-1", "r-2", "p-1", "x", "m-0", "1",
    ];
    let mut proposed = 0;
    for round in 0..200 {
        let cs = create(&s, &agent(), NOW).unwrap();
        s.write(|tx| {
            for n in 1..=8 {
                let mut targets = Map::new();
                for _ in 0..next(3) {
                    let list: Vec<Value> = (0..next(4))
                        .map(|_| match next(6) {
                            0 => json!(5),
                            _ => json!(ids[next(ids.len() as u64) as usize]),
                        })
                        .collect();
                    let v = if next(8) == 0 {
                        json!("t-1")
                    } else {
                        json!(list)
                    };
                    targets.insert(keys[next(keys.len() as u64) as usize].into(), v);
                }
                let args = [
                    "{}",
                    " { } ",
                    "[]",
                    "{\"a\":1}",
                    "null",
                    "\"x\"",
                    "0",
                    "{\"\":{}}",
                ][next(16).saturating_sub(8) as usize];
                tx.execute(
                    "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                     VALUES (?1, 1, ?2, 'archive', ?3, ?4)",
                    rusqlite::params![cs, n, Value::Object(targets).to_string(), args],
                )?;
            }
            Ok(())
        })
        .unwrap();
        submit(&s, cs, &agent(), "Fuzz.", NOW).unwrap();
        for a in doc(&s, cs)["actions"].as_array().unwrap() {
            let resolved = a["resolved"]["messages"].as_array().unwrap();
            match a["decision"]["state"].as_str().unwrap() {
                "proposed" => {
                    assert!(!resolved.is_empty(), "round {round}: {a}");
                    assert_eq!(a["args"], json!({}), "round {round}: {a}");
                    proposed += 1;
                }
                "denied" => assert!(resolved.is_empty(), "round {round}: {a}"),
                other => panic!("round {round}: {other}"),
            }
        }
    }
    assert!(proposed > 0, "the test reaches the proposed path");
}

/// Submit gives the messages from the targets only: a row of another
/// writer in the draft goes away.
#[test]
fn submit_derives_the_messages_from_the_targets() {
    let dir = TempDir::new("derive");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    s.write(|tx| Ok(tx.execute("INSERT INTO action_message VALUES (1, 4)", [])?))
        .unwrap();
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    assert_eq!(
        doc(&s, cs)["actions"][0]["resolved"],
        json!({"messages": ["m-1"]})
    );
}

/// One action changes one account. Targets in two accounts are denied.
#[test]
fn an_action_changes_one_account() {
    let dir = TempDir::new("accounts");
    let s = store(&dir);
    let b = s.add_account("gmail", "second@example.com").unwrap();
    s.put_messages(
        b,
        &[NewMessage {
            provider_id: "h1".into(),
            thread_provider_id: "ht1".into(),
            ..Default::default()
        }],
    )
    .unwrap();
    let t: i64 = s
        .read(|c| {
            Ok(
                c.query_row("SELECT thread_id FROM message WHERE id = 6", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    let t = format!("t-{t}");
    let cs = create(&s, &agent(), NOW).unwrap();
    for targets in [&["m-1", "m-6"][..], &["t-1", &t], &["m-6"]] {
        add_action(&s, cs, &agent(), Verb::Archive, targets, &Map::new(), None).unwrap();
    }
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    let d = doc(&s, cs);
    for n in 0..2 {
        assert_eq!(d["actions"][n]["decision"]["state"], "denied", "{n}");
        assert_eq!(
            d["actions"][n]["decision"]["reason"],
            "the targets are in two or more accounts"
        );
    }
    assert_eq!(d["actions"][2]["decision"]["state"], "proposed");
}

/// Property test of the changeset ID parser on random text: it accepts
/// exactly `cs-` and a positive decimal `i64`, and round-trips.
#[test]
fn changeset_id_parser() {
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let alphabet = b"cs-0123456789+ x";
    for _ in 0..20_000 {
        let len = (next() % 24) as usize;
        let mut text: String = (0..len)
            .map(|_| alphabet[(next() % alphabet.len() as u64) as usize] as char)
            .collect();
        if next() % 2 == 0 {
            text = format!("cs-{text}");
        }
        let digits = text
            .strip_prefix("cs-")
            .filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
        let expect = digits
            .and_then(|d| d.parse::<i64>().ok())
            .filter(|n| *n > 0);
        assert_eq!(
            iris_changeset::parse_changeset_id(&text),
            expect,
            "{text:?}"
        );
        let row = (next() >> 1) as i64;
        if row > 0 {
            assert_eq!(
                iris_changeset::parse_changeset_id(&iris_changeset::format_changeset_id(row)),
                Some(row)
            );
        }
    }
    assert_eq!(
        iris_changeset::parse_changeset_id("cs-9223372036854775807"),
        Some(i64::MAX)
    );
    assert_eq!(
        iris_changeset::parse_changeset_id("cs-9223372036854775808"),
        None
    );
}

/// The hash of section 5.1: SHA-256 of the verb, the targets text and the
/// args text, each with its length first, then each resolved message ID.
#[test]
fn the_hash_covers_each_input() {
    use sha2::{Digest, Sha256};
    let dir = TempDir::new("hash");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["t-1"], &Map::new(), None).unwrap();
    add_action(&s, cs, &agent(), Verb::Read, &["t-1"], &Map::new(), None).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    let expect = |verb: &str, targets: &str, ids: &[&str]| -> String {
        let mut h = Sha256::new();
        for text in [verb, targets, "{}"] {
            h.update((text.len() as u64).to_be_bytes());
            h.update(text.as_bytes());
        }
        for id in ids {
            h.update(id.as_bytes());
            h.update(b",");
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    };
    let threads = r#"{"threads":["t-1"]}"#;
    assert_eq!(
        hash(&s, cs, 1, 1),
        expect("archive", threads, &["m-1", "m-2"])
    );
    assert_eq!(hash(&s, cs, 1, 2), expect("read", threads, &["m-1", "m-2"]));
    assert_eq!(
        hash(&s, cs, 1, 3),
        expect("archive", r#"{"messages":["m-1"]}"#, &["m-1"])
    );
}

fn at(changeset: i64, revision: i64, n: i64) -> ActionId {
    ActionId {
        changeset,
        revision,
        n,
    }
}

#[test]
fn a_person_approves_only_with_the_hash() {
    let dir = TempDir::new("person");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    refused(approve(&s, at(cs, 1, 1), "", NOW));
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    refused(decide(
        &s,
        at(cs, 1, 1),
        State::Approved,
        By::Person,
        None,
        NOW,
    ));
    for by in [By::Policy, By::Limit, By::Engine, By::Author, By::Expiry] {
        refused(decide(&s, at(cs, 1, 1), State::Approved, by, None, NOW));
    }
    let r = s.write(|tx| {
        Ok(tx.execute(
            "UPDATE changeset_action SET state = 'approved', decided_by = 'engine'",
            [],
        )?)
    });
    assert!(r.is_err());
    refused(approve(&s, at(cs, 1, 1), "0000", NOW));
    assert!(matches!(
        approve(&s, at(cs, 1, 7), "0000", NOW),
        Err(Error::NotFound)
    ));
    approve(&s, at(cs, 1, 1), &hash(&s, cs, 1, 1), NOW).unwrap();
}

/// A rule can change and keep its ID: each rule decision names the policy
/// version too, so the decision names one rule text.
#[test]
fn a_rule_decision_names_the_policy_version() {
    let dir = TempDir::new("policy");
    let s = store(&dir);
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('b', 'h2', 0);
             UPDATE policy_rule_id SET removed_at = 0 WHERE id = 4;",
        )?;
        Ok(())
    })
    .unwrap();
    let cs = create(&s, &agent(), NOW).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["m-4"], &Map::new(), None).unwrap();
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    let rule = |id, policy| By::Rule { id, policy };
    decide(&s, at(cs, 1, 1), State::Approved, rule(3, 2), None, NOW).unwrap();
    decide(
        &s,
        at(cs, 1, 2),
        State::Denied,
        rule(2, 2),
        Some("Rule 2 denies it."),
        NOW,
    )
    .unwrap();
    let d = doc(&s, cs);
    assert_eq!(d["actions"][0]["decision"]["rule"], 3);
    assert_eq!(d["actions"][0]["decision"]["policy"], 2);
    assert_eq!(d["actions"][1]["decision"]["rule"], 2);
    // Only a rule ID of the active policy decides: not an older version, not
    // a version or an ID that the store does not have, not a removed ID.
    let other = create(&s, &agent(), NOW).unwrap();
    add_action(
        &s,
        other,
        &agent(),
        Verb::Archive,
        &["m-2"],
        &Map::new(),
        None,
    )
    .unwrap();
    submit(&s, other, &agent(), "Archive.", NOW).unwrap();
    for (id, policy) in [(3, 1), (3, 9), (7, 2), (4, 2)] {
        refused(decide(
            &s,
            at(other, 1, 1),
            State::Approved,
            rule(id, policy),
            None,
            NOW,
        ));
    }
    assert_eq!(
        doc(&s, other)["actions"][0]["decision"]["state"],
        "proposed"
    );
}

/// An approved action of revision 1 stays open after revision 2. When it
/// comes back to review, the person approves it with its hash.
#[test]
fn a_person_approves_an_older_action_that_came_back() {
    let dir = TempDir::new("older");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    let rule = By::Rule { id: 1, policy: 1 };
    decide(&s, at(cs, 1, 1), State::Approved, rule, None, NOW).unwrap();
    let h1 = hash(&s, cs, 1, 1);
    revise(&s, cs, &agent()).unwrap();
    add_action(&s, cs, &agent(), Verb::Read, &["m-4"], &Map::new(), None).unwrap();
    submit(&s, cs, &agent(), "And read.", NOW).unwrap();
    decide(
        &s,
        at(cs, 1, 1),
        State::InReview,
        By::Policy,
        Some("The rule is gone."),
        NOW,
    )
    .unwrap();
    refused(approve(&s, at(cs, 1, 1), "0000", NOW));
    approve(&s, at(cs, 1, 1), &h1, NOW).unwrap();
    assert_eq!(doc(&s, cs)["actions"][0]["decision"]["by"], "person");
}

/// The policy and the review move a proposed action: in review, approved
/// or denied, each by its own actor (section 5.2).
#[test]
fn decisions_of_the_policy_and_the_person() {
    let dir = TempDir::new("states");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    for (verb, target) in [
        (Verb::Archive, "t-1"),
        (Verb::Read, "m-4"),
        (Verb::Trash, "m-4"),
    ] {
        add_action(&s, cs, &agent(), verb, &[target], &Map::new(), None).unwrap();
    }
    submit(&s, cs, &agent(), "Priya needs the numbers.", NOW).unwrap();
    decide(
        &s,
        at(cs, 1, 1),
        State::InReview,
        By::Policy,
        Some("No rule allows archive."),
        NOW,
    )
    .unwrap();
    // An action in review waits for a person: no rule decides it.
    refused(decide(
        &s,
        at(cs, 1, 1),
        State::Approved,
        By::Rule { id: 1, policy: 1 },
        None,
        NOW,
    ));
    approve(&s, at(cs, 1, 1), &hash(&s, cs, 1, 1), NOW + 1).unwrap();
    decide(
        &s,
        at(cs, 1, 2),
        State::Approved,
        By::Rule { id: 4, policy: 1 },
        None,
        NOW,
    )
    .unwrap();
    // decide sets only in_review, approved and denied, each by its actor.
    for (to, by) in [
        (State::Applied, By::Engine),
        (State::Expired, By::Expiry),
        (State::Cancelled, By::Author),
        (State::InReview, By::Person),
        (State::Denied, By::Author),
        (State::Denied, By::Expiry),
    ] {
        refused(decide(&s, at(cs, 1, 3), to, by, Some("r"), NOW));
    }
    refused(decide(
        &s,
        at(cs, 1, 3),
        State::Denied,
        By::Person,
        None,
        NOW,
    ));
    decide(
        &s,
        at(cs, 1, 3),
        State::Denied,
        By::Person,
        Some("Keep it."),
        NOW,
    )
    .unwrap();

    let d = doc(&s, cs);
    assert_eq!(d["state"], "open", "two actions are approved");
    assert_eq!(d["actions"][0]["decision"]["by"], "person");
    assert_eq!(d["actions"][1]["decision"]["by"], "rule");
    assert_eq!(d["actions"][1]["decision"]["rule"], 4);
    assert_eq!(d["actions"][2]["decision"]["reason"], "Keep it.");
    assert_eq!(
        d["actions"][2]["decision"]["at"],
        "2025-11-16T08:57:02.000Z"
    );
    // A decided action does not move again.
    refused(decide(
        &s,
        at(cs, 1, 3),
        State::Approved,
        By::Rule { id: 1, policy: 1 },
        None,
        NOW,
    ));
    // A draft action has no decision.
    let draft = create(&s, &agent(), NOW).unwrap();
    add_action(&s, draft, &agent(), Verb::Read, &["m-1"], &Map::new(), None).unwrap();
    refused(decide(
        &s,
        at(draft, 1, 1),
        State::Denied,
        By::Engine,
        Some("r"),
        NOW,
    ));
}

/// An open action expires 14 days after its submit (section 5.5): no
/// approval and no other decision lands at or after the deadline.
#[test]
fn no_decision_after_the_expiry() {
    const EXPIRY: i64 = 14 * 24 * 60 * 60 * 1000;
    let dir = TempDir::new("expiry");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    for m in ["m-1", "m-2", "m-4"] {
        add_action(&s, cs, &agent(), Verb::Archive, &[m], &Map::new(), None).unwrap();
    }
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    let late = NOW + EXPIRY;
    approve(&s, at(cs, 1, 1), &hash(&s, cs, 1, 1), late - 1).unwrap();
    refused(approve(&s, at(cs, 1, 2), &hash(&s, cs, 1, 2), late));
    for (to, by, reason) in [
        (State::InReview, By::Policy, None),
        (State::Denied, By::Limit, Some("Too many.")),
        (State::InReview, By::Engine, None),
    ] {
        refused(decide(&s, at(cs, 1, 3), to, by, reason, late));
        refused(decide(&s, at(cs, 1, 1), to, by, reason, late + 1));
    }
    for (n, state) in [(1, "approved"), (2, "proposed"), (3, "proposed")] {
        assert_eq!(hash_doc(&s, cs, 1, n)["decision"]["state"], state);
    }
}
