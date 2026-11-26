use std::fs;
use std::path::PathBuf;

use iris_changeset::{
    add_action, add_action_in, approve, approve_in, create, create_in, decide, decide_in, expire,
    revise, submit, submit_in, transaction, withdraw, write_document, ActionId, Author, AuthorKind,
    By, Error, State, Verb, BULK,
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
        job: None,
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
        // The examples of the open model, as items c-1 and r-2. The
        // commitment has message 1 as its origin.
        for (form, file, id) in [
            ("commitment", "commitment-you-owe.json", "c-1"),
            ("record", "record-receipt.json", "r-2"),
        ] {
            let mut doc =
                iris_spec::read_json(&iris_spec::schema_dir().join("examples").join(file));
            doc["id"] = json!(id);
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 0, 0)",
                rusqlite::params![form, a, doc["status"].as_str().unwrap(), doc.to_string()],
            )?;
        }
        tx.execute(
            "INSERT INTO item_source(item_id, message_id, role) VALUES (1, 1, 'origin')",
            [],
        )?;
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

/// The JSON document of a changeset. Each document of each test agrees
/// with the published changeset schema (R2-12).
fn doc(s: &Store, cs: i64) -> Value {
    let mut out = Vec::new();
    write_document(s, cs, &mut out).unwrap();
    let d: Value = serde_json::from_slice(&out).unwrap();
    let errors: Vec<String> = iris_spec::validator("changeset")
        .iter_errors(&d)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    d
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
        (Verb::Label, &["m-1"], json!({"label": "INBOX"})),
        (Verb::Close, &["t-1"], json!({"reason": "Done."})),
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
            (37, r#"{"messages":["m-1","m-1"]}"#, "{}"),
            (38, r#"{"messages":["m-01"]}"#, "{}"),
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
        Verb::Correct,
        &["c-1", "r-1"],
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
/// verbs, JSON shapes, IDs and arguments, written as another writer would.
/// Submit never panics. A mailbox action is `proposed` only with one or
/// more messages, a state action only with items of its kind, a draft only
/// with one account, one thread or (a change by the person) one draft, and
/// a send only with the one draft.
#[test]
fn random_target_rows() {
    let dir = TempDir::new("fuzz");
    let s = store(&dir);
    // Draft d-1: a reply in thread t-1, from a draft action of its own.
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0);
             INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
               VALUES (last_insert_rowid(), 1, 1, 'draft', '{}', '{}');
             INSERT INTO draft(account_id, thread_id, provider_id, message_id_hdr, content,
                               hash, raw, action_id, created_at, updated_at)
               VALUES (1, 1, 'r1', 'iris.1@example.com', '{}', 'h', x'00',
                       last_insert_rowid(), 0, 0);",
        )?;
        // A feed item f-3, a conversation v-4 and a signal s-5 for skip,
        // mute and dismiss.
        for (row, id, form, file, status) in [
            (3, "f-3", "feed", "feed.json", "new"),
            (4, "v-4", "conversation", "conversation.json", "active"),
            (5, "s-5", "signal", "signal-code.json", "active"),
        ] {
            let mut doc =
                iris_spec::read_json(&iris_spec::schema_dir().join("examples").join(file));
            doc["id"] = json!(id);
            tx.execute(
                "INSERT INTO item(id, form, account_id, status, doc, created_at, updated_at)
                 VALUES (?1, ?2, 1, ?3, ?4, 0, 0)",
                rusqlite::params![row, form, status, doc.to_string()],
            )?;
        }
        Ok(())
    })
    .unwrap();
    let mut x: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    let keys = [
        "threads", "messages", "items", "accounts", "drafts", "x", "",
    ];
    let ids = [
        "t-1", "t-2", "t-3", "t-9", "m-1", "m-3", "m-5", "c-1", "r-2", "p-1", "x", "m-0", "1",
        "a-1", "a-2", "d-1", "d-9", "f-3", "v-4", "s-5", "f-9",
    ];
    let verbs = [
        "archive", "close", "decline", "defer", "correct", "draft", "send", "skip", "mute",
        "dismiss",
    ];
    let (mut proposed, mut state, mut write, mut one) = (0, 0, 0, 0);
    for round in 0..400 {
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
                let v = next(verbs.len() as u64) as usize;
                let verb = verbs[v];
                // A third of the rows get a target of the right kind for the verb.
                if next(3) == 0 {
                    let fits = [
                        json!({"threads": ["t-1"]}),
                        json!({"items": ["c-1"]}),
                        json!({"items": ["c-1"]}),
                        json!({"items": ["c-1"]}),
                        json!({"items": ["r-2"]}),
                        [json!({"accounts": ["a-1"]}), json!({"threads": ["t-1"]})]
                            [next(2) as usize]
                            .clone(),
                        json!({"drafts": ["d-1"]}),
                        json!({"items": ["f-3"]}),
                        json!({"items": ["v-4"]}),
                        json!({"items": ["s-5"]}),
                    ];
                    targets = fits[v].as_object().unwrap().clone();
                }
                let args = [
                    "{\"reason\":\"Done.\"}",
                    "{\"date\":\"2028-02-29\"}",
                    "{\"date\":\"2027-02-29\"}",
                    "{\"field\":\"due.date\",\"value\":\"2026-12-01\"}",
                    "{\"field\":\"fields.order\",\"value\":\"EB-1\"}",
                    "{}",
                    " { } ",
                    "[]",
                    "{\"a\":1}",
                    "null",
                    "\"x\"",
                    "0",
                    "{\"\":{}}",
                    "{\"to\":[\"p@x.org\"],\"subject\":\"Hi\",\"body\":\"B\"}",
                    "{\"to\":[\"p@x.org\"],\"body\":\"B\"}",
                    "{\"to\":[\"p@x.org\"],\"subject\":\"\",\"body\":\"B\"}",
                    "{\"to\":[],\"body\":\"B\"}",
                    "{\"to\":\"p@x.org\",\"body\":1}",
                ];
                // Half the rows get the valid shape of the verb, the rest any shape.
                let fit = [
                    5,
                    0,
                    0,
                    1,
                    3 + next(2) as usize,
                    13 + next(2) as usize,
                    5,
                    0,
                    0,
                    0,
                ][v];
                let args = args[if next(2) == 0 {
                    fit
                } else {
                    next(args.len() as u64) as usize
                }];
                tx.execute(
                    "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                     VALUES (?1, 1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![cs, n, verb, Value::Object(targets).to_string(), args],
                )?;
            }
            Ok(())
        })
        .unwrap();
        submit(&s, cs, &agent(), "Fuzz.", NOW).unwrap();
        // The rows of another writer can break the schema (issue #158), so
        // only the proposed actions go through the schema check.
        let mut out = Vec::new();
        write_document(&s, cs, &mut out).unwrap();
        let mut d: Value = serde_json::from_slice(&out).unwrap();
        let all = d["actions"].as_array().unwrap().clone();
        d["actions"]
            .as_array_mut()
            .unwrap()
            .retain(|a| a["decision"]["state"] == "proposed");
        if !d["actions"].as_array().unwrap().is_empty() {
            let errors: Vec<String> = iris_spec::validator("changeset")
                .iter_errors(&d)
                .map(|e| format!("{e} at {}", e.instance_path()))
                .collect();
            assert!(errors.is_empty(), "round {round}: {errors:#?}");
        }
        for a in &all {
            let resolved = a["resolved"]["messages"].as_array().unwrap();
            match a["decision"]["state"].as_str().unwrap() {
                "proposed" if a["verb"] == "draft" => {
                    let t = &a["targets"];
                    assert!(
                        t == &json!({"accounts": ["a-1"]})
                            || t["threads"].as_array().is_some_and(|l| l.len() == 1),
                        "round {round}: {a}"
                    );
                    write += 1;
                }
                "proposed" if a["verb"] == "send" => {
                    assert_eq!(
                        a["targets"],
                        json!({"drafts": ["d-1"]}),
                        "round {round}: {a}"
                    );
                    assert_eq!(a["args"], json!({}), "round {round}: {a}");
                    write += 1;
                }
                "proposed" if a["verb"] == "archive" => {
                    assert!(!resolved.is_empty(), "round {round}: {a}");
                    assert_eq!(a["args"], json!({}), "round {round}: {a}");
                    proposed += 1;
                }
                "proposed"
                    if ["skip", "mute", "dismiss"].contains(&a["verb"].as_str().unwrap()) =>
                {
                    // One item of the one form of the verb.
                    let item = match a["verb"].as_str().unwrap() {
                        "skip" => "f-3",
                        "mute" => "v-4",
                        _ => "s-5",
                    };
                    assert_eq!(a["targets"], json!({"items": [item]}), "round {round}: {a}");
                    assert_eq!(a["args"]["reason"], "Done.", "round {round}: {a}");
                    one += 1;
                }
                "proposed" => {
                    // The fixture has the commitment c-1 and the record r-2.
                    let kinds: &[&str] = match a["verb"].as_str().unwrap() {
                        "correct" => &["c-1", "r-2"],
                        _ => &["c-1"],
                    };
                    let items = a["targets"]["items"].as_array().unwrap();
                    assert!(!items.is_empty(), "round {round}: {a}");
                    for i in items {
                        assert!(kinds.contains(&i.as_str().unwrap()), "round {round}: {a}");
                    }
                    state += 1;
                }
                "denied" => assert!(resolved.is_empty(), "round {round}: {a}"),
                other => panic!("round {round}: {other}"),
            }
        }
    }
    assert!(proposed > 0, "the test reaches the proposed path");
    assert!(state > 0, "the test reaches a proposed state action");
    assert!(write > 0, "the test reaches a proposed draft or send");
    assert!(one > 0, "the test reaches a proposed skip, mute or dismiss");
}

/// A change of a draft by the person, with the switch on: submit never
/// panics, and proposes only one draft of Iris. A reply change (d-1)
/// resolves to one parent and has no subject. A new-mail change (d-2)
/// resolves to no message and has a subject. Each proposed action agrees
/// with the schema.
#[test]
fn random_draft_change_rows() {
    let dir = TempDir::new("fuzz-change");
    let s = store(&dir);
    let person = Author {
        kind: AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    };
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO changeset(author_kind, created_at) VALUES ('person', 0);
             INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
               VALUES (last_insert_rowid(), 1, 1, 'draft', '{}', '{}');
             INSERT INTO draft(account_id, thread_id, provider_id, message_id_hdr, content,
                               hash, raw, action_id, created_at, updated_at)
               VALUES (1, 1, 'r1', 'iris.1@example.com',
                       '{\"content\":{\"to\":[\"p@x.org\"],\"subject\":\"Re: Q\",\"body\":\"B\"}}',
                       'h1', x'00', last_insert_rowid(), 0, 0);
             INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
               VALUES (1, 1, 2, 'draft', '{}', '{}');
             INSERT INTO draft(account_id, provider_id, message_id_hdr, content,
                               hash, raw, action_id, created_at, updated_at)
               VALUES (1, 'r2', 'iris.2@example.com',
                       '{\"content\":{\"to\":[\"p@x.org\"],\"subject\":\"Q\",\"body\":\"B\"}}',
                       'h2', x'00', last_insert_rowid(), 0, 0);",
        )?;
        Ok(())
    })
    .unwrap();
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    let ids = [
        json!("d-1"),
        json!("d-2"),
        json!("d-9"),
        json!("d-0"),
        json!("t-1"),
        json!("x"),
        json!(1),
    ];
    let args = [
        "{\"to\":[\"p@x.org\"],\"body\":\"B\"}",
        "{\"to\":[\"p@x.org\"],\"subject\":\"Hi\",\"body\":\"B\"}",
        "{\"to\":[\"p@x.org\"],\"subject\":\"\",\"body\":\"B\"}",
        "{\"to\":[],\"body\":\"B\"}",
        "{}",
        "null",
    ];
    let (mut reply, mut new) = (0, 0);
    for round in 0..300 {
        let cs = create(&s, &person, NOW).unwrap();
        s.write(|tx| {
            for n in 1..=4 {
                let list: Vec<Value> = (0..next(4))
                    .map(|_| ids[next(ids.len() as u64) as usize].clone())
                    .collect();
                let targets = json!({ "drafts": list }).to_string();
                let a = args[next(args.len() as u64) as usize];
                tx.execute(
                    "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                     VALUES (?1, 1, ?2, 'draft', ?3, ?4)",
                    rusqlite::params![cs, n, targets, a],
                )?;
            }
            Ok(())
        })
        .unwrap();
        submit(&s, cs, &person, "Fuzz.", NOW).unwrap();
        let mut out = Vec::new();
        write_document(&s, cs, &mut out).unwrap();
        let mut d: Value = serde_json::from_slice(&out).unwrap();
        let all = d["actions"].as_array().unwrap().clone();
        d["actions"]
            .as_array_mut()
            .unwrap()
            .retain(|a| a["decision"]["state"] == "proposed");
        if !d["actions"].as_array().unwrap().is_empty() {
            let errors: Vec<String> = iris_spec::validator("changeset")
                .iter_errors(&d)
                .map(|e| format!("{e} at {}", e.instance_path()))
                .collect();
            assert!(errors.is_empty(), "round {round}: {errors:#?}");
        }
        for a in &all {
            let resolved = a["resolved"]["messages"].as_array().unwrap();
            let subject = a["args"].get("subject").is_some();
            match a["decision"]["state"].as_str().unwrap() {
                "proposed" if a["targets"] == json!({"drafts": ["d-1"]}) => {
                    assert_eq!(resolved.len(), 1, "round {round}: {a}");
                    assert!(!subject, "round {round}: {a}");
                    reply += 1;
                }
                "proposed" if a["targets"] == json!({"drafts": ["d-2"]}) => {
                    assert!(resolved.is_empty(), "round {round}: {a}");
                    assert!(subject, "round {round}: {a}");
                    new += 1;
                }
                "denied" => assert!(resolved.is_empty(), "round {round}: {a}"),
                other => panic!("round {round}: {other}: {a}"),
            }
        }
        // Withdraw the proposals, so that no change stays open for the
        // next round.
        withdraw(&s, cs, Some(&person)).unwrap();
    }
    assert!(reply > 0, "the test reaches a proposed reply change");
    assert!(new > 0, "the test reaches a proposed new-mail change");
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
        // Only the canonical form: the text of format_changeset_id (#309).
        let expect = digits
            .and_then(|d| d.parse::<i64>().ok())
            .filter(|n| *n > 0 && iris_changeset::format_changeset_id(*n) == text);
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
    for bad in ["cs-01", "cs-007", "cs-0"] {
        assert_eq!(iris_changeset::parse_changeset_id(bad), None, "{bad}");
    }
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

/// A caller can run create, add, submit and decide in one transaction: a
/// refusal at any step leaves no row.
#[test]
fn one_transaction_for_a_whole_run() {
    let dir = TempDir::new("tx");
    let s = store(&dir);
    let count = |s: &Store| -> i64 {
        s.read(|c| Ok(c.query_row("SELECT count(*) FROM changeset", [], |r| r.get(0))?))
            .unwrap()
    };
    refused(transaction(&s, |c| {
        let cs = create_in(c, &agent(), NOW)?;
        add_action_in(c, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None)?;
        submit_in(c, cs, &agent(), " ", NOW)
    }));
    refused(transaction(&s, |c| {
        let cs = create_in(c, &agent(), NOW)?;
        add_action_in(c, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None)?;
        submit_in(c, cs, &agent(), "Archive.", NOW)?;
        decide_in(c, at(cs, 1, 1), State::Approved, By::Person, None, NOW)
    }));
    assert_eq!(count(&s), 0);
    let cs = transaction(&s, |c| {
        let cs = create_in(c, &agent(), NOW)?;
        add_action_in(c, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None)?;
        submit_in(c, cs, &agent(), "Archive.", NOW)?;
        decide_in(c, at(cs, 1, 1), State::InReview, By::Policy, None, NOW)?;
        Ok(cs)
    })
    .unwrap();
    assert_eq!(count(&s), 1);
    assert_eq!(hash_doc(&s, cs, 1, 1)["decision"]["state"], "in_review");
}

/// A review sends one decision for a whole changeset: a stale hash on one
/// action rolls back the approvals and denials before it.
#[test]
fn one_review_decision_is_all_or_nothing() {
    let dir = TempDir::new("review-tx");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    for m in ["m-1", "m-2", "m-4"] {
        add_action(&s, cs, &agent(), Verb::Archive, &[m], &Map::new(), None).unwrap();
    }
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    let (h1, h2) = (hash(&s, cs, 1, 1), hash(&s, cs, 1, 2));
    refused(transaction(&s, |c| {
        approve_in(c, at(cs, 1, 1), &h1, NOW)?;
        decide_in(c, at(cs, 1, 3), State::Denied, By::Person, Some("No."), NOW)?;
        approve_in(c, at(cs, 1, 2), "stale", NOW)
    }));
    for n in 1..=3 {
        assert_eq!(hash_doc(&s, cs, 1, n)["decision"]["state"], "proposed");
    }
    transaction(&s, |c| {
        approve_in(c, at(cs, 1, 1), &h1, NOW)?;
        approve_in(c, at(cs, 1, 2), &h2, NOW)?;
        decide_in(c, at(cs, 1, 3), State::Denied, By::Person, Some("No."), NOW)
    })
    .unwrap();
    for (n, state) in [(1, "approved"), (2, "approved"), (3, "denied")] {
        assert_eq!(hash_doc(&s, cs, 1, n)["decision"]["state"], state);
    }
}

/// `label` and `unlabel` take one user label name. A system label, by name
/// or ID and in any case, changes only through its own verb (section 4).
#[test]
fn label_takes_one_user_label() {
    let dir = TempDir::new("label");
    let s = store(&dir);
    // A system label of the store that is not in the Gmail list.
    s.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO label(account_id, provider_id, name, kind)
             VALUES (1, 'Label_9', 'Muted', 'system'), (1, 'Label_10', 'Épinglé', 'system'),
               (1, 'Label_11', 'Σ', 'system')",
            [],
        )?)
    })
    .unwrap();
    let cs = create(&s, &agent(), NOW).unwrap();
    let cases: [(Verb, Value, bool); 24] = [
        (Verb::Label, json!({"label": "Finance"}), true),
        (Verb::Unlabel, json!({"label": "Finance"}), true),
        // A new label: the apply makes it (section 7.2).
        (Verb::Label, json!({"label": "Trips/2026"}), true),
        (Verb::Label, json!({"label": "inbox"}), false),
        (Verb::Unlabel, json!({"label": "Unread"}), false),
        (Verb::Label, json!({"label": "category_promotions"}), false),
        (Verb::Label, json!({"label": "MUTED"}), false),
        (Verb::Label, json!({"label": "label_9"}), false),
        (Verb::Label, json!({}), false),
        (Verb::Label, json!({"label": ""}), false),
        // A name with a space at an end is a user label too (#204).
        (Verb::Label, json!({"label": " Finance"}), true),
        (Verb::Label, json!({"label": " "}), false),
        // The whole name is compared: a spaced name is not the system label.
        (Verb::Label, json!({"label": " INBOX"}), true),
        (Verb::Label, json!({"label": "Muted "}), true),
        (Verb::Unlabel, json!({"label": " Σ"}), true),
        (Verb::Label, json!({"label": 7}), false),
        (Verb::Label, json!({"label": "Finance", "x": 1}), false),
        (Verb::Label, json!({"name": "Finance"}), false),
        (Verb::Unlabel, json!({"label": ["Finance"]}), false),
        (Verb::Unlabel, json!({"label": "SENT"}), false),
        // No regard to case, also outside ASCII.
        (Verb::Label, json!({"label": "épinglé"}), false),
        (Verb::Label, json!({"label": "ÉPINGLÉ"}), false),
        // Case folding, not a lower case: σ, ς and Σ are one letter.
        (Verb::Label, json!({"label": "ς"}), false),
        (Verb::Unlabel, json!({"label": "σ"}), false),
    ];
    for (verb, a, _) in &cases {
        add_action(&s, cs, &agent(), *verb, &["t-1"], &args(a.clone()), None).unwrap();
    }
    submit(&s, cs, &agent(), "Labels.", NOW).unwrap();
    for (i, (verb, a, ok)) in cases.iter().enumerate() {
        let d = hash_doc(&s, cs, 1, i as i64 + 1);
        let expect = if *ok { "proposed" } else { "denied" };
        assert_eq!(d["decision"]["state"], expect, "{} {a}", verb.as_str());
        if *ok {
            assert_eq!(d["resolved"]["messages"], json!(["m-1", "m-2"]), "{d}");
        }
    }
}

/// An action names each target once: two spellings of one ID are one
/// target. The check is linear: 200,000 targets take well under a second.
#[test]
fn an_action_names_each_target_once() {
    let dir = TempDir::new("once");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    for targets in [
        &["m-1", "m-1"][..],
        &["m-1", "m-01"],
        &["t-2", "t-1", "t-02"],
    ] {
        refused(add_action(
            &s,
            cs,
            &agent(),
            Verb::Archive,
            targets,
            &Map::new(),
            None,
        ));
    }
    let many: Vec<String> = (1..=200_000).map(|n| format!("m-{n}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let start = std::time::Instant::now();
    add_action(&s, cs, &agent(), Verb::Archive, &many, &Map::new(), None).unwrap();
    assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
}

/// The state verbs (section 4) take items: `close`, `decline` and `defer`
/// take commitments, `correct` takes any item. Each argument has one shape.
#[test]
fn state_verbs_take_items() {
    let dir = TempDir::new("state");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    let cases: [(Verb, &[&str], Value, bool); 22] = [
        (Verb::Close, &["c-1"], json!({"reason": "Done."}), true),
        (Verb::Decline, &["c-1"], json!({"reason": "No time."}), true),
        (Verb::Defer, &["c-1"], json!({"date": "2028-02-29"}), true),
        (
            Verb::Correct,
            &["c-1"],
            json!({"field": "due.date", "value": "2026-12-01"}),
            true,
        ),
        (
            Verb::Correct,
            &["r-2"],
            json!({"field": "fields.order", "value": "EB-1"}),
            true,
        ),
        (Verb::Close, &["r-2"], json!({"reason": "Done."}), false),
        (Verb::Close, &["c-9"], json!({"reason": "Done."}), false),
        // The prefix does not fit the item.
        (
            Verb::Correct,
            &["s-1"],
            json!({"field": "title", "value": "x"}),
            false,
        ),
        (Verb::Close, &["c-1"], json!({}), false),
        (Verb::Close, &["c-1"], json!({"reason": " "}), false),
        (
            Verb::Decline,
            &["c-1"],
            json!({"reason": "x", "why": "y"}),
            false,
        ),
        (Verb::Defer, &["c-1"], json!({"date": "2026-02-29"}), false),
        (Verb::Defer, &["c-1"], json!({"date": "2026-13-01"}), false),
        (Verb::Defer, &["c-1"], json!({"date": "2026-1-01"}), false),
        (Verb::Defer, &["c-1"], json!({"date": 20260101}), false),
        (
            Verb::Correct,
            &["c-1"],
            json!({"field": "status", "value": "done"}),
            false,
        ),
        (
            Verb::Correct,
            &["c-1"],
            json!({"field": "due.date", "value": 7}),
            false,
        ),
        (Verb::Correct, &["c-1"], json!({"field": "due.date"}), false),
        (
            Verb::Correct,
            &["c-1"],
            json!({"field": "", "value": "x"}),
            false,
        ),
        (
            Verb::Correct,
            &["r-2"],
            json!({"field": "issuer.sender", "value": "p-1"}),
            false,
        ),
        (Verb::Archive, &["c-1"], json!({}), false),
        (Verb::Close, &["m-1"], json!({"reason": "Done."}), false),
    ];
    for (verb, targets, a, _) in &cases {
        add_action(&s, cs, &agent(), *verb, targets, &args(a.clone()), None).unwrap();
    }
    submit(&s, cs, &agent(), "Tidy up.", NOW).unwrap();
    for (i, (verb, targets, a, ok)) in cases.iter().enumerate() {
        let d = hash_doc(&s, cs, 1, i as i64 + 1);
        let expect = if *ok { "proposed" } else { "denied" };
        assert_eq!(
            d["decision"]["state"],
            expect,
            "{} {targets:?} {a}: {d}",
            verb.as_str()
        );
        // A state verb has no bulk flag and no first_contact flag.
        assert_eq!(d["flags"], json!([]), "{d}");
    }
    // The origin message of the commitment is the resolved message, for the
    // rules. The record has no origin row in this store.
    assert_eq!(
        hash_doc(&s, cs, 1, 1)["resolved"]["messages"],
        json!(["m-1"])
    );
    assert_eq!(hash_doc(&s, cs, 1, 5)["resolved"]["messages"], json!([]));
}

/// `correct` takes items of any form in one action (section 4): the form
/// of each item does not need to agree. Each item must pass the rules of a
/// correction, so a field of the commitment only denies the record.
#[test]
fn correct_takes_items_of_two_forms() {
    let dir = TempDir::new("forms");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    let a = args(json!({"field": "due.date", "value": "2026-12-01"}));
    add_action(&s, cs, &agent(), Verb::Correct, &["c-1", "r-2"], &a, None).unwrap();
    submit(&s, cs, &agent(), "Fix the dates.", NOW).unwrap();
    let d = hash_doc(&s, cs, 1, 1);
    assert_eq!(d["targets"], json!({"items": ["c-1", "r-2"]}), "{d}");
    assert_eq!(d["decision"]["state"], "denied", "{d}");
    let why = d["decision"]["reason"].as_str().unwrap();
    assert!(why.contains("r-2") && !why.contains("target"), "{why}");
}

/// An item that an open action names is pinned (specification 1, section
/// 7.5): understanding does not delete it. A decided action does not pin.
#[test]
fn an_open_action_pins_its_items() {
    let dir = TempDir::new("pin");
    let s = store(&dir);
    let pins = |s: &Store| -> Vec<i64> {
        s.read(|c| {
            let mut st = c.prepare("SELECT item_id FROM item_pin ORDER BY 1")?;
            let v = st
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(v)
        })
        .unwrap()
    };
    let cs = create(&s, &agent(), NOW).unwrap();
    add_action(
        &s,
        cs,
        &agent(),
        Verb::Close,
        &["c-1"],
        &args(json!({"reason": "Done."})),
        None,
    )
    .unwrap();
    add_action(
        &s,
        cs,
        &agent(),
        Verb::Close,
        &["r-2"],
        &args(json!({"reason": "Done."})),
        None,
    )
    .unwrap();
    assert_eq!(pins(&s), Vec::<i64>::new(), "a draft does not pin");
    submit(&s, cs, &agent(), "Done.", NOW).unwrap();
    assert_eq!(pins(&s), vec![1], "a denied action does not pin");
    decide(&s, at(cs, 1, 1), State::InReview, By::Policy, None, NOW).unwrap();
    assert_eq!(pins(&s), vec![1]);
    decide(
        &s,
        at(cs, 1, 1),
        State::Denied,
        By::Person,
        Some("No."),
        NOW,
    )
    .unwrap();
    assert_eq!(pins(&s), Vec::<i64>::new());
    // The rows of a submitted action do not change.
    for sql in [
        "DELETE FROM action_item",
        "UPDATE action_item SET item_id = 2",
        "INSERT OR REPLACE INTO action_item(action_id, item_id) VALUES (1, 1)",
        "INSERT INTO action_item(action_id, item_id) VALUES (1, 2)",
    ] {
        assert!(s.write(|tx| Ok(tx.execute(sql, [])?)).is_err(), "{sql}");
    }
}

/// Claim an approved action with a ledger row in `writing`, as the apply
/// does before its first write call (migration 11).
fn start_writing(s: &Store, cs: i64, n: i64) {
    s.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO ledger(action_id, ops, state, undo, at)
             SELECT id, '{}', 'writing', 'inverse', 1 FROM changeset_action
             WHERE changeset_id = ?1 AND n = ?2",
            rusqlite::params![cs, n],
        )?)
    })
    .unwrap();
}

/// A withdraw (section 5.5) comes from the author or the person. It
/// cancels each open action and each draft action. An action whose
/// provider calls started stays `approved`, to end as applied or failed.
#[test]
fn a_withdraw_cancels_the_open_actions() {
    let dir = TempDir::new("withdraw");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    for m in ["m-1", "m-2", "m-4"] {
        add_action(&s, cs, &agent(), Verb::Archive, &[m], &Map::new(), None).unwrap();
    }
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    approve(&s, at(cs, 1, 1), &hash(&s, cs, 1, 1), NOW).unwrap();
    approve(&s, at(cs, 1, 2), &hash(&s, cs, 1, 2), NOW).unwrap();
    decide(&s, at(cs, 1, 3), State::InReview, By::Policy, None, NOW).unwrap();
    start_writing(&s, cs, 1);
    let other = Author {
        session: Some("se-13".into()),
        ..agent()
    };
    refused(withdraw(&s, cs, Some(&other)));
    assert!(matches!(withdraw(&s, 999, None), Err(Error::NotFound)));
    withdraw(&s, cs, Some(&agent())).unwrap();
    let d = doc(&s, cs);
    assert_eq!(d["state"], "withdrawn");
    let states: Vec<&Value> = (0..3)
        .map(|i| &d["actions"][i]["decision"]["state"])
        .collect();
    assert_eq!(
        states,
        [&json!("approved"), &json!("cancelled"), &json!("cancelled")]
    );
    assert_eq!(d["actions"][1]["decision"]["by"], "author");
    // Decision D16: the document names who withdrew.
    assert_eq!(d["withdrawn_by"], "author");
    refused(withdraw(&s, cs, None));
    refused(revise(&s, cs, &agent()));

    // A draft: the person withdraws it, and it takes no new action.
    let draft = create(&s, &agent(), NOW).unwrap();
    add_action(
        &s,
        draft,
        &agent(),
        Verb::Archive,
        &["m-1"],
        &Map::new(),
        None,
    )
    .unwrap();
    assert!(doc(&s, draft).get("withdrawn_by").is_none());
    withdraw(&s, draft, None).unwrap();
    assert_eq!(
        doc(&s, draft)["actions"][0]["decision"]["state"],
        "cancelled"
    );
    assert_eq!(doc(&s, draft)["withdrawn_by"], "person");
    refused(add_action(
        &s,
        draft,
        &agent(),
        Verb::Archive,
        &["m-2"],
        &Map::new(),
        None,
    ));
}

/// The expiry (section 5.5): 14 days after its submit, each open action
/// becomes `expired`, by `expiry`. Not one millisecond before. An action
/// whose provider calls started continues to its end.
#[test]
fn open_actions_expire_14_days_after_the_submit() {
    const EXPIRY: i64 = 14 * 24 * 60 * 60 * 1000;
    let dir = TempDir::new("expire");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    for m in ["m-1", "m-2", "m-4", "t-1"] {
        add_action(&s, cs, &agent(), Verb::Archive, &[m], &Map::new(), None).unwrap();
    }
    add_action(&s, cs, &agent(), Verb::Archive, &["m-9"], &Map::new(), None).unwrap();
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    approve(&s, at(cs, 1, 1), &hash(&s, cs, 1, 1), NOW).unwrap();
    approve(&s, at(cs, 1, 2), &hash(&s, cs, 1, 2), NOW).unwrap();
    decide(&s, at(cs, 1, 3), State::InReview, By::Policy, None, NOW).unwrap();
    start_writing(&s, cs, 1);
    // A draft of another changeset has no submit, so it does not expire.
    let draft = create(&s, &agent(), NOW).unwrap();
    add_action(
        &s,
        draft,
        &agent(),
        Verb::Archive,
        &["m-1"],
        &Map::new(),
        None,
    )
    .unwrap();

    assert_eq!(expire(&s, NOW + EXPIRY - 1).unwrap(), 0);
    assert_eq!(expire(&s, NOW + EXPIRY).unwrap(), 3);
    assert_eq!(expire(&s, NOW + EXPIRY + 1).unwrap(), 0);
    let d = doc(&s, cs);
    let states: Vec<&Value> = (0..5)
        .map(|i| &d["actions"][i]["decision"]["state"])
        .collect();
    assert_eq!(
        states,
        [
            &json!("approved"),
            &json!("expired"),
            &json!("expired"),
            &json!("expired"),
            &json!("denied")
        ]
    );
    assert_eq!(d["actions"][1]["decision"]["by"], "expiry");
    assert_eq!(
        d["state"], "open",
        "the action with a writing ledger row is still open"
    );
    assert!(doc(&s, draft)["actions"][0]["decision"].is_null());
}

/// No state verb counts toward the bulk limit of a changeset (section 5.3):
/// a state verb changes no message. For each verb of `STATE_VERBS` but
/// `correct`, an item with more origin messages than the limit sits next
/// to an archive of one message, and the archive gets no `bulk` flag.
#[test]
fn no_state_verb_counts_toward_bulk() {
    let dir = TempDir::new("bulk-state");
    let s = store(&dir);
    let n = BULK as usize + 1;
    let batch: Vec<NewMessage> = (0..n)
        .map(|i| NewMessage {
            provider_id: format!("o{i}"),
            thread_provider_id: "ot".into(),
            ..Default::default()
        })
        .collect();
    s.put_messages(1, &batch).unwrap();
    let forms = [
        ("commitment", "commitment-you-owe.json", "c", "open"),
        ("feed", "feed.json", "f", "new"),
        ("conversation", "conversation.json", "v", "active"),
        ("signal", "signal-code.json", "s", "active"),
    ];
    let mut ids = Vec::new();
    s.write(|tx| {
        for (form, file, prefix, status) in forms {
            let mut doc =
                iris_spec::read_json(&iris_spec::schema_dir().join("examples").join(file));
            let row: i64 = tx.query_row("SELECT coalesce(max(id), 0) + 1 FROM item", [], |r| {
                r.get(0)
            })?;
            doc["id"] = json!(format!("{prefix}-{row}"));
            doc["status"] = json!(status);
            tx.execute(
                "INSERT INTO item(id, form, account_id, status, doc, created_at, updated_at)
                 VALUES (?1, ?2, 1, ?3, ?4, 0, 0)",
                rusqlite::params![row, form, status, doc.to_string()],
            )?;
            tx.execute(
                "INSERT INTO item_source(item_id, message_id, role)
                 SELECT ?1, id, 'origin' FROM message WHERE provider_id LIKE 'o%'",
                [row],
            )?;
            ids.push(format!("{prefix}-{row}"));
        }
        Ok(())
    })
    .unwrap();
    for verb in iris_changeset::STATE_VERBS {
        let verb = Verb::parse(verb).unwrap();
        let (target, a) = match verb {
            Verb::Correct => continue,
            Verb::Defer => (&ids[0], json!({"date": "2099-01-05"})),
            Verb::Skip => (&ids[1], json!({"reason": "No."})),
            Verb::Mute => (&ids[2], json!({"reason": "No."})),
            Verb::Dismiss => (&ids[3], json!({"reason": "No."})),
            _ => (&ids[0], json!({"reason": "No."})),
        };
        let cs = create(&s, &agent(), NOW).unwrap();
        add_action(&s, cs, &agent(), verb, &[target], &args(a), None).unwrap();
        add_action(&s, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
        submit(&s, cs, &agent(), "Tidy up.", NOW).unwrap();
        let state = hash_doc(&s, cs, 1, 1);
        assert_eq!(state["decision"]["state"], "proposed", "{state}");
        assert_eq!(
            state["resolved"]["messages"].as_array().unwrap().len(),
            n,
            "{}",
            verb.as_str()
        );
        assert_eq!(
            hash_doc(&s, cs, 1, 2)["flags"],
            json!([]),
            "{}",
            verb.as_str()
        );
    }
}

/// `skip`, `mute` and `dismiss` take one item each (R5-02): an action with
/// two items is denied at submit.
#[test]
fn skip_mute_and_dismiss_take_one_item() {
    let dir = TempDir::new("one-item");
    let s = store(&dir);
    let forms = [
        (Verb::Skip, "feed", "feed.json", "f", "new"),
        (
            Verb::Mute,
            "conversation",
            "conversation.json",
            "v",
            "active",
        ),
        (Verb::Dismiss, "signal", "signal-code.json", "s", "active"),
    ];
    for (verb, form, file, prefix, status) in forms {
        let mut ids = Vec::new();
        s.write(|tx| {
            for _ in 0..2 {
                let row: i64 =
                    tx.query_row("SELECT coalesce(max(id), 0) + 1 FROM item", [], |r| {
                        r.get(0)
                    })?;
                let mut doc =
                    iris_spec::read_json(&iris_spec::schema_dir().join("examples").join(file));
                doc["id"] = json!(format!("{prefix}-{row}"));
                tx.execute(
                    "INSERT INTO item(id, form, account_id, status, doc, created_at, updated_at)
                     VALUES (?1, ?2, 1, ?3, ?4, 0, 0)",
                    rusqlite::params![row, form, status, doc.to_string()],
                )?;
                ids.push(format!("{prefix}-{row}"));
            }
            Ok(())
        })
        .unwrap();
        let cs = create(&s, &agent(), NOW).unwrap();
        let targets: Vec<&str> = ids.iter().map(String::as_str).collect();
        let a = args(json!({"reason": "No."}));
        add_action(&s, cs, &agent(), verb, &targets, &a, None).unwrap();
        add_action(&s, cs, &agent(), verb, &targets[..1], &a, None).unwrap();
        // A changeset action needs a reason, as `close` does.
        add_action(&s, cs, &agent(), verb, &targets[..1], &Map::new(), None).unwrap();
        submit(&s, cs, &agent(), "Tidy up.", NOW).unwrap();
        let none = hash_doc(&s, cs, 1, 3);
        assert_eq!(none["decision"]["state"], "denied", "{none}");
        assert!(none["decision"]["reason"]
            .as_str()
            .unwrap()
            .contains("needs one reason"));
        let two = hash_doc(&s, cs, 1, 1);
        assert_eq!(two["decision"]["state"], "denied", "{two}");
        assert!(two["decision"]["reason"]
            .as_str()
            .unwrap()
            .contains("one target"));
        assert_eq!(hash_doc(&s, cs, 1, 2)["decision"]["state"], "proposed");
    }
}

/// Submit refuses a reason longer than `MAX_REASON` for each verb with a
/// reason, on each path: an agent changeset here, the web client in the
/// API tests.
#[test]
fn submit_refuses_a_long_reason() {
    let dir = TempDir::new("long-reason");
    let s = store(&dir);
    let long = "x".repeat(iris_changeset::MAX_REASON + 1);
    let fit = "x".repeat(iris_changeset::MAX_REASON);
    let cs = create(&s, &agent(), NOW).unwrap();
    for verb in [Verb::Close, Verb::Decline] {
        add_action(
            &s,
            cs,
            &agent(),
            verb,
            &["c-1"],
            &args(json!({"reason": long})),
            None,
        )
        .unwrap();
    }
    add_action(
        &s,
        cs,
        &agent(),
        Verb::Close,
        &["c-1"],
        &args(json!({"reason": fit})),
        None,
    )
    .unwrap();
    submit(&s, cs, &agent(), "Tidy up.", NOW).unwrap();
    for n in 1..=2 {
        let d = hash_doc(&s, cs, 1, n);
        assert_eq!(d["decision"]["state"], "denied", "{d}");
        assert!(
            d["decision"]["reason"]
                .as_str()
                .unwrap()
                .contains("longer than 500"),
            "{d}"
        );
    }
    assert_eq!(hash_doc(&s, cs, 1, 3)["decision"]["state"], "proposed");
}

/// Random `ledger` targets (section 7.4): submit proposes an undo only for
/// the exact target of its own linked entry, with the changed messages of
/// the entry, and denies each other shape with no message row.
#[test]
fn random_ledger_targets() {
    let dir = TempDir::new("fuzz-ledger");
    let s = store(&dir);
    // Entry l-1: an archive of messages 1 and 2, applied.
    let cs = create(&s, &agent(), NOW).unwrap();
    add_action(
        &s,
        cs,
        &agent(),
        Verb::Archive,
        &["m-1", "m-2"],
        &Map::new(),
        None,
    )
    .unwrap();
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    s.write(|tx| {
        tx.execute_batch(
            "UPDATE changeset_action SET state = 'approved', decided_by = 'person',
               decided_at = 1 WHERE id = 1;
             INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
             VALUES (1, 1, '{}', 'writing', 'inverse', 1);
             INSERT INTO ledger_message(ledger_id, message_id, call, pre, post)
             VALUES (1, 1, 1, '[\"INBOX\"]', '[]'), (1, 2, 1, '[\"INBOX\"]', '[]');
             UPDATE ledger SET call = 1;
             UPDATE ledger_message SET done = 1;
             UPDATE ledger SET state = 'applied';",
        )?;
        Ok(())
    })
    .unwrap();
    let person = Author {
        kind: AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    };
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    let ids = [
        json!("l-1"),
        json!("l-2"),
        json!("l-01"),
        json!("l-0"),
        json!("l-"),
        json!("l-1 "),
        json!("L-1"),
        json!("m-1"),
        json!(1),
        json!(null),
        json!("l-99999999999999999999"),
    ];
    let (mut proposed, mut denied) = (0, 0);
    for round in 0..300 {
        let list: Vec<Value> = (0..next(3))
            .map(|_| ids[next(ids.len() as u64) as usize].clone())
            .collect();
        let mut targets = Map::new();
        targets.insert("ledger".into(), json!(list));
        if next(6) == 0 {
            targets.insert("messages".into(), json!(["m-1"]));
        }
        let targets = Value::Object(targets).to_string();
        let exact = targets == "{\"ledger\":[\"l-1\"]}";
        let cs = create(&s, &person, NOW).unwrap();
        let action = s
            .write(|tx| {
                tx.execute(
                    "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                     VALUES (?1, 1, 1, 'inbox', ?2, '{}')",
                    rusqlite::params![cs, targets],
                )?;
                Ok(tx.last_insert_rowid())
            })
            .unwrap();
        // The link needs the whole entry as the target (migration 23).
        let linked = next(3) != 0
            && s.write(|tx| {
                Ok(tx.execute(
                    "INSERT INTO ledger_undo(action_id, ledger_id, step) VALUES (?1, 1, 1)",
                    [action],
                )?)
            })
            .map_err(|e| assert!(!exact && e.to_string().contains("iris:"), "{e}"))
            .is_ok();
        submit(&s, cs, &person, "Undo.", NOW).unwrap();
        let mut out = Vec::new();
        write_document(&s, cs, &mut out).unwrap();
        let d: Value = serde_json::from_slice(&out).unwrap();
        let a = &d["actions"][0];
        let resolved = a["resolved"]["messages"].as_array().unwrap();
        match a["decision"]["state"].as_str().unwrap() {
            "proposed" => {
                assert!(linked && exact, "round {round}: {a}");
                assert_eq!(resolved, &vec![json!("m-1"), json!("m-2")], "round {round}");
                let errors: Vec<String> = iris_spec::validator("changeset")
                    .iter_errors(&d)
                    .map(|e| format!("{e} at {}", e.instance_path()))
                    .collect();
                assert!(errors.is_empty(), "round {round}: {errors:#?}");
                proposed += 1;
                // The next round can link the entry again.
                withdraw(&s, cs, Some(&person)).unwrap();
            }
            "denied" => {
                assert!(!(linked && exact), "round {round}: {a}");
                assert!(resolved.is_empty(), "round {round}: {a}");
                denied += 1;
            }
            other => panic!("round {round}: {other}"),
        }
    }
    assert!(proposed > 0 && denied > 0, "{proposed} {denied}");
}

/// True when two other threads can each hold a read connection now: this
/// thread holds no store connection. It waits at most 2 seconds.
fn readers_free(s: &Store) -> bool {
    use std::sync::{mpsc, Barrier};
    use std::time::Duration;
    let held = Barrier::new(2);
    let (release, wait) = mpsc::channel::<()>();
    let (got, second) = mpsc::channel();
    std::thread::scope(|scope| {
        let held = &held;
        scope.spawn(move || {
            s.read(|_| {
                held.wait();
                let _ = wait.recv();
                Ok(())
            })
            .unwrap()
        });
        held.wait();
        scope.spawn(|| {
            let _ = s.read(|_| Ok(()));
            let _ = got.send(());
        });
        let free = second.recv_timeout(Duration::from_secs(2)).is_ok();
        drop(release);
        free
    })
}

// Invariant (engine hang of 2026-11-18): the document goes to its peer (a
// socket or the channel of the web server) only after the store read ends.
// A peer that stops reading then never holds a store connection.
#[test]
fn the_document_goes_out_after_the_read() {
    struct Probe<'a>(&'a Store, Vec<u8>, bool);
    impl std::io::Write for Probe<'_> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.2 |= !readers_free(self.0);
            self.1.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let dir = TempDir::new("after-read");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    add_action(&s, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    submit(&s, cs, &agent(), "Archive.", NOW).unwrap();
    let mut out = Probe(&s, Vec::new(), false);
    write_document(&s, cs, &mut out).unwrap();
    assert!(!out.1.is_empty());
    assert!(
        !out.2,
        "a store connection was held during a write to the peer"
    );
}

/// Decision D16 (issue #221): each withdraw names who withdrew, in the same
/// update as the state, and the value never changes. The store refuses
/// each other path.
#[test]
fn the_store_keeps_who_withdrew() {
    let dir = TempDir::new("withdrawnby");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    let sql = |q: &str| s.write(|tx| Ok(tx.execute(q, [cs])?));
    // A withdraw with no actor, and an actor with no withdraw.
    assert!(sql("UPDATE changeset SET state = 'withdrawn' WHERE id = ?1").is_err());
    assert!(sql("UPDATE changeset SET withdrawn_by = 'person' WHERE id = ?1").is_err());
    assert!(
        sql("UPDATE changeset SET state = 'withdrawn', withdrawn_by = 'robot' WHERE id = ?1")
            .is_err()
    );
    withdraw(&s, cs, None).unwrap();
    for q in [
        "UPDATE changeset SET withdrawn_by = 'author' WHERE id = ?1",
        "UPDATE changeset SET withdrawn_by = NULL WHERE id = ?1",
    ] {
        assert!(sql(q).is_err(), "{q}");
    }
    assert_eq!(doc(&s, cs)["withdrawn_by"], "person");
}

/// The invariant of `withdrawn_by` for each write: a value only on a withdrawn
/// changeset, each new withdraw names who withdrew, and the value does not
/// change after the withdraw. The test tries each insert and each update of
/// `state` and `withdrawn_by` from each start row, and checks the row after
/// each write that the store takes.
#[test]
fn each_write_keeps_withdrawn_by_with_the_withdraw() {
    let dir = TempDir::new("withdrawnby-all");
    let s = store(&dir);
    let states = ["draft", "open", "closed", "withdrawn"];
    let actors: [Option<&str>; 4] = [None, Some("author"), Some("person"), Some("robot")];
    let row = |id: i64| -> (String, Option<String>) {
        s.read(|c| {
            Ok(c.query_row(
                "SELECT state, withdrawn_by FROM changeset WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
    };
    let ok = |(state, by): &(String, Option<String>)| by.is_none() || state == "withdrawn";
    // Each insert.
    for st in states {
        for by in actors {
            let r = s.write(|tx| {
                tx.execute(
                    "INSERT INTO changeset(author_kind, created_at, state, withdrawn_by)
                     VALUES ('agent', 0, ?1, ?2)",
                    rusqlite::params![st, by],
                )?;
                Ok(tx.last_insert_rowid())
            });
            if let Ok(id) = r {
                assert!(ok(&row(id)), "insert {st} {by:?}");
                assert!(by.is_none(), "insert {st} {by:?} took an actor");
            }
        }
    }
    // Each update from each start row that the store holds.
    let starts: Vec<(&str, Option<&str>)> = vec![
        ("draft", None),
        ("open", None),
        ("closed", None),
        ("withdrawn", Some("author")),
        ("withdrawn", Some("person")),
    ];
    let news: Vec<Option<&str>> = states.iter().map(|s| Some(*s)).chain([None]).collect();
    let mut tried = 0;
    for (st0, by0) in &starts {
        for new_state in &news {
            for new_by in actors.iter().map(Some).chain([None]) {
                if new_state.is_none() && new_by.is_none() {
                    continue;
                }
                // Another trigger can refuse a start row (the state of a
                // changeset comes from its actions); skip that start.
                let Ok(id) = s.write(|tx| {
                    tx.execute(
                        "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0)",
                        [],
                    )?;
                    let id = tx.last_insert_rowid();
                    tx.execute(
                        "UPDATE changeset SET state = ?2, withdrawn_by = ?3 WHERE id = ?1",
                        rusqlite::params![id, st0, by0],
                    )?;
                    Ok(id)
                }) else {
                    continue;
                };
                tried += 1;
                let mut sets = Vec::new();
                if let Some(ns) = new_state {
                    sets.push(format!("state = '{ns}'"));
                }
                if let Some(nb) = new_by {
                    sets.push(match nb {
                        Some(v) => format!("withdrawn_by = '{v}'"),
                        None => "withdrawn_by = NULL".into(),
                    });
                }
                let q = format!("UPDATE changeset SET {} WHERE id = ?1", sets.join(", "));
                if s.write(|tx| Ok(tx.execute(&q, [id])?)).is_ok() {
                    let after = row(id);
                    assert!(ok(&after), "{st0} {by0:?}: {q} gave {after:?}");
                    if *st0 != "withdrawn" && after.0 == "withdrawn" {
                        assert!(after.1.is_some(), "{st0}: {q} withdrew with no actor");
                    }
                    if *st0 == "withdrawn" && after.0 == "withdrawn" {
                        assert_eq!(after.1.as_deref(), *by0, "{q} changed the actor");
                    }
                }
            }
        }
    }
    // The draft start and both withdrawn starts are always possible.
    assert!(tried >= 3 * news.len() * 4, "{tried} updates tried");
}
