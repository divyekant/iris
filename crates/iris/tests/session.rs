//! The write verbs of a job session (docs/specs/2026-11-17-job-verbs.md):
//! the session author, one draft changeset for each session, and the grant
//! check at submit.

use iris::socket::{self, code};
use iris_changeset::{Author, AuthorKind};
use iris_job::{add_scope, create, finish, revoke_scope, By, End, Ref};
use iris_store::Store;
use serde_json::{json, Value};

const NOW: i64 = 1_763_283_422_000;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("iris-session-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Thread 1 (messages 1 and 2) is in the scope of the job. Thread 2
/// (message 7) is not. Commitment c-1 comes from message 1, c-4 from
/// message 7, and the signal s-5 from message 1.
fn fixture(name: &str) -> (Store, i64) {
    let store = Store::open(&dir(name)).unwrap();
    store
        .write(|tx| {
            tx.execute_batch(
                r#"
INSERT INTO account (id, provider, address) VALUES (1, 'gmail', 'me@example.com');
INSERT INTO thread (id, account_id, provider_id) VALUES (1, 1, 'a'), (2, 1, 'b');
INSERT INTO sender (address, name, kind) VALUES
  ('dana@example.com', 'Dana', 'person'), ('sam@example.com', 'Sam', 'person');
INSERT INTO message (id, account_id, thread_id, provider_id, internal_date, understood_at) VALUES
  (1, 1, 1, 'm1', 1000, 1), (2, 1, 1, 'm2', 2000, 1), (7, 1, 2, 'm7', 7000, 1);
INSERT INTO header (message_id, date, from_addr, from_name, to_addrs, subject) VALUES
  (1, 1000, 'dana@example.com', 'Dana', 'me@example.com', 'Contract'),
  (2, 2000, 'dana@example.com', 'Dana', 'me@example.com', 'Contract'),
  (7, 7000, 'sam@example.com', 'Sam', 'me@example.com', 'Dinner');
INSERT INTO body (message_id, subject, sender, text) VALUES
  (1, 'Contract', 'dana@example.com', 'Here is the contract.'),
  (2, 'Contract', 'dana@example.com', 'Did you sign it?'),
  (7, 'Dinner', 'sam@example.com', 'Dinner at 8?');
INSERT INTO item (id, form, account_id, status, doc, created_at, updated_at) VALUES
  (1, 'commitment', 1, 'open', '{"id":"c-1","form":"commitment","title":"Sign"}', 1, 1),
  (4, 'commitment', 1, 'open', '{"id":"c-4","form":"commitment","title":"Dinner"}', 1, 1);
INSERT INTO item_source (item_id, message_id, role) VALUES (1, 1, 'origin'), (4, 7, 'origin');
"#,
            )?;
            Ok(())
        })
        .unwrap();
    let job = create(&store, "Get the signed contract.", None, NOW).unwrap();
    add_scope(&store, job, Ref::Thread(1), By::Person, NOW).unwrap();
    (store, job)
}

fn session(job: i64, session: &str) -> Author {
    Author {
        kind: AuthorKind::Agent,
        name: Some("claude-code".into()),
        session: Some(session.into()),
        job: Some(iris_job::format_job_id(job)),
    }
}

/// One request as `me`: the result, or the code and the reason.
fn call(store: &Store, me: &Author, verb: &str, args: Value) -> Result<Value, (u8, String)> {
    let line = json!({ "v": 1, "verb": verb, "args": args }).to_string();
    let mut out = Vec::new();
    socket::answer_as(store, me, line.as_bytes(), &mut out).unwrap();
    let a: Value = serde_json::from_slice(&out).unwrap();
    if a["ok"] == json!(true) {
        Ok(a["result"].clone())
    } else {
        Err((a["code"].as_u64().unwrap() as u8, a["error"].to_string()))
    }
}

fn add(
    store: &Store,
    me: &Author,
    verb: &str,
    targets: &[&str],
    args: Value,
) -> Result<Value, (u8, String)> {
    call(
        store,
        me,
        "add",
        json!({ "verb": verb, "targets": targets, "args": args }),
    )
}

fn count(store: &Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

#[test]
fn each_verb_adds_one_line_to_one_draft_and_nothing_leaves_before_submit() {
    let (store, job) = fixture("lines");
    let me = session(job, "se-1");
    let a = add(&store, &me, "archive", &["t-1"], json!({})).unwrap();
    let cs = a["changeset"].as_str().unwrap().to_string();
    let b = add(
        &store,
        &me,
        "label",
        &["m-1"],
        json!({ "label": "Contract" }),
    )
    .unwrap();
    let c = add(
        &store,
        &me,
        "close",
        &["c-1"],
        json!({ "reason": "Signed." }),
    )
    .unwrap();
    let d = add(
        &store,
        &me,
        "defer",
        &["c-1"],
        json!({ "until": "2026-12-01" }),
    )
    .unwrap();
    for (r, n) in [(&a, 1), (&b, 2), (&c, 3), (&d, 4)] {
        assert_eq!(
            (r["changeset"].as_str(), r["n"].as_i64()),
            (Some(cs.as_str()), Some(n))
        );
    }
    assert_eq!(count(&store, "SELECT count(*) FROM changeset"), 1);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM changeset_action WHERE state IS NOT NULL"
        ),
        0
    );
    assert_eq!(count(&store, "SELECT count(*) FROM ledger"), 0);
    // The submit of the draft: each action gets a state.
    let doc = call(
        &store,
        &me,
        "submit",
        json!({ "changeset": cs, "why": "Tidy." }),
    )
    .unwrap();
    assert_eq!(
        doc["author"]["job"],
        json!(iris_job::format_job_id(job)),
        "{doc}"
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM changeset_action WHERE state IS NULL"
        ),
        0
    );
    // The next draft of the job is a new changeset.
    let other = session(job, "se-2");
    let e = add(&store, &other, "archive", &["t-1"], json!({})).unwrap();
    assert_ne!(e["changeset"], a["changeset"]);
}

#[test]
fn a_later_session_revises_the_changeset_of_its_job() {
    let (store, job) = fixture("revise");
    let first = session(job, "se-1");
    let r = add(&store, &first, "archive", &["t-1"], json!({})).unwrap();
    let cs = r["changeset"].clone();
    call(
        &store,
        &first,
        "submit",
        json!({ "changeset": cs, "why": "Tidy." }),
    )
    .unwrap();
    // The person comments. A later session of the job makes revision 2,
    // but only when the job has no other draft.
    let later = session(job, "se-2");
    let other = add(&store, &later, "archive", &["t-1"], json!({})).unwrap();
    let e = call(&store, &later, "revise", json!({ "changeset": cs })).unwrap_err();
    assert_eq!(e.0, code::USAGE, "{e:?}");
    assert!(e.1.contains("draft"), "{e:?}");
    call(
        &store,
        &later,
        "withdraw",
        json!({ "changeset": other["changeset"] }),
    )
    .unwrap();
    let doc = call(&store, &later, "revise", json!({ "changeset": cs })).unwrap();
    assert_eq!(doc["revision"], json!(2), "{doc}");
    let r = call(
        &store,
        &later,
        "add",
        json!({ "verb": "read", "targets": ["t-1"], "changeset": cs }),
    )
    .unwrap();
    assert_eq!(r["changeset"], cs);
    // A new draft waits until the revision is submitted.
    assert!(iris_changeset::create(&store, &later, NOW).is_err());
    let doc = call(
        &store,
        &later,
        "submit",
        json!({ "changeset": cs, "why": "As you asked." }),
    )
    .unwrap();
    let old: Vec<&Value> = doc["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["revision"] == json!(1))
        .collect();
    assert!(
        old.iter()
            .all(|a| a["decision"]["state"] == json!("replaced")),
        "{doc}"
    );
}

#[test]
fn a_target_outside_the_grant_fails_the_whole_submit() {
    for (name, verb, target, args) in [
        ("thread", "archive", "t-2", json!({})),
        ("bad-args", "archive", "t-2", json!({ "foo": 1 })),
        ("bad-item-args", "close", "c-4", json!({})),
        ("message", "read", "m-7", json!({})),
        ("item", "close", "c-4", json!({ "reason": "x" })),
    ] {
        let (store, job) = fixture(&format!("outside-{name}"));
        let me = session(job, "se-1");
        let r = add(&store, &me, "archive", &["t-1"], json!({})).unwrap();
        let cs = r["changeset"].as_str().unwrap();
        add(&store, &me, verb, &[target], args).unwrap();
        let e = call(
            &store,
            &me,
            "submit",
            json!({ "changeset": cs, "why": "x" }),
        )
        .unwrap_err();
        assert_eq!(e.0, code::DENIED, "{name}: {e:?}");
        // Nothing changed: the changeset is a draft, with no message rows.
        assert_eq!(
            count(
                &store,
                "SELECT count(*) FROM changeset WHERE state = 'draft'"
            ),
            1
        );
        assert_eq!(
            count(
                &store,
                "SELECT count(*) FROM changeset_action WHERE state IS NOT NULL"
            ),
            0
        );
        assert_eq!(
            count(&store, "SELECT count(*) FROM action_message"),
            0,
            "{name}"
        );
    }
}

#[test]
fn a_thread_with_one_message_outside_the_grant_fails() {
    let (store, job) = fixture("partial");
    let me = session(job, "se-1");
    // A new message comes to thread 1 after the add of the scope.
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO message (id, account_id, thread_id, provider_id, internal_date, understood_at)
                   VALUES (3, 1, 1, 'm3', 3000, 1);",
            )?;
            Ok(())
        })
        .unwrap();
    let r = add(&store, &me, "archive", &["t-1"], json!({})).unwrap();
    let e = call(
        &store,
        &me,
        "submit",
        json!({ "changeset": r["changeset"], "why": "x" }),
    )
    .unwrap_err();
    assert_eq!(e.0, code::DENIED);
    assert!(e.1.contains("m-3"), "{e:?}");
}

#[test]
fn a_revoke_or_the_end_of_the_job_empties_the_grant() {
    let (s, job) = fixture("revoke");
    let store = s;
    let me = session(job, "se-1");
    let r = add(&store, &me, "archive", &["t-1"], json!({})).unwrap();
    revoke_scope(&store, job, Ref::Thread(1), By::Person, NOW).unwrap();
    let e = call(
        &store,
        &me,
        "submit",
        json!({ "changeset": r["changeset"], "why": "x" }),
    )
    .unwrap_err();
    assert_eq!(e.0, code::DENIED);

    let (done, job) = fixture("done");
    let store = done;
    let me = session(job, "se-1");
    let r = add(&store, &me, "close", &["c-1"], json!({ "reason": "x" })).unwrap();
    finish(&store, job, iris_job::By::Person, End::Cancel, None, NOW).unwrap();
    // A done job changes nothing: each change by its author is refused.
    let e = call(
        &store,
        &me,
        "submit",
        json!({ "changeset": r["changeset"], "why": "x" }),
    )
    .unwrap_err();
    assert_eq!(e.0, code::USAGE, "{e:?}");
    let e = add(&store, &me, "archive", &["t-1"], json!({})).unwrap_err();
    assert_eq!(e.0, code::USAGE, "{e:?}");
    // A done job gets no new changeset.
    let e = add(
        &store,
        &session(job, "se-2"),
        "archive",
        &["t-1"],
        json!({}),
    )
    .unwrap_err();
    assert_eq!(e.0, code::USAGE, "{e:?}");
}

#[test]
fn a_session_cannot_draft_send_or_nudge() {
    let (store, job) = fixture("write");
    let me = session(job, "se-1");
    for verb in ["draft", "send", "nudge"] {
        let e = add(&store, &me, verb, &["t-1"], json!({})).unwrap_err();
        assert_eq!(e.0, code::DENIED, "{verb}");
    }
    assert_eq!(count(&store, "SELECT count(*) FROM changeset"), 0);
    // A draft action that another path wrote into a session changeset.
    let cs = add(&store, &me, "archive", &["t-1"], json!({})).unwrap();
    let row = iris_changeset::parse_changeset_id(cs["changeset"].as_str().unwrap()).unwrap();
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                 VALUES (?1, 1, 2, 'draft', '{\"threads\":[\"t-1\"]}', '{}')",
                [row],
            )?;
            Ok(())
        })
        .unwrap();
    let e = call(
        &store,
        &me,
        "submit",
        json!({ "changeset": cs["changeset"], "why": "x" }),
    )
    .unwrap_err();
    assert_eq!(e.0, code::DENIED, "{e:?}");
}

#[test]
fn a_session_sees_only_the_changesets_of_its_job() {
    let (store, job) = fixture("other");
    let other = create(&store, "Another goal.", None, NOW).unwrap();
    add_scope(&store, other, Ref::Thread(1), By::Person, NOW).unwrap();
    let mine = session(job, "se-1");
    // The same session ID, another job.
    let theirs = session(other, "se-1");
    let r = add(&store, &theirs, "archive", &["t-1"], json!({})).unwrap();
    let cs = r["changeset"].clone();
    for verb in ["show", "withdraw"] {
        let e = call(&store, &mine, verb, json!({ "changeset": cs })).unwrap_err();
        assert_eq!(e.0, code::USAGE, "{verb}: {e:?}");
        assert!(e.1.contains("no changeset"), "{e:?}");
    }
    let e = call(
        &store,
        &mine,
        "submit",
        json!({ "changeset": cs, "why": "x" }),
    )
    .unwrap_err();
    assert_eq!(e.0, code::USAGE);
    let e = call(&store, &mine, "revise", json!({ "changeset": cs })).unwrap_err();
    assert_eq!(e.0, code::USAGE);
    let e = call(
        &store,
        &mine,
        "add",
        json!({ "verb": "archive", "targets": ["t-1"], "changeset": cs }),
    )
    .unwrap_err();
    assert_eq!(e.0, code::USAGE);
    // The host changesets are not of the job either, and drafts is host only.
    let host = socket::local();
    let h = call(
        &store,
        &host,
        "add",
        json!({ "verb": "archive", "targets": ["t-1"] }),
    )
    .unwrap();
    let e = call(
        &store,
        &mine,
        "show",
        json!({ "changeset": h["changeset"] }),
    )
    .unwrap_err();
    assert_eq!(e.0, code::USAGE);
    assert_eq!(
        call(&store, &mine, "drafts", json!({})).unwrap_err().0,
        code::USAGE
    );
    assert!(call(&store, &theirs, "show", json!({ "changeset": cs })).is_ok());
}

#[test]
fn only_an_agent_session_of_an_open_canonical_job_has_a_job() {
    let (store, job) = fixture("author");
    let j = iris_job::format_job_id(job);
    let bad = [
        Author {
            kind: AuthorKind::Local,
            name: None,
            session: Some("se-1".into()),
            job: Some(j.clone()),
        },
        Author {
            kind: AuthorKind::Agent,
            name: None,
            session: None,
            job: Some(j.clone()),
        },
        Author {
            job: Some(format!("j-0{job}")),
            ..session(job, "se-1")
        },
        Author {
            job: Some("j-99".into()),
            ..session(job, "se-1")
        },
    ];
    for a in bad {
        assert!(iris_changeset::create(&store, &a, NOW).is_err(), "{a:?}");
    }
    assert!(iris_changeset::create(&store, &session(job, "se-1"), NOW).is_ok());
}

#[test]
fn the_policy_decides_a_session_submit_as_agent() {
    let (store, job) = fixture("policy");
    // A rule for `local` allows each archive: it must not match an agent.
    let policy = "version = 0\nmode = \"apply\"\n\n[[rule]]\nid = 1\nname = \"Local archive\"\neffect = \"allow\"\nverbs = [\"archive\"]\nwhen = { author = \"local\" }\nby = \"person\"\nadded = 2026-11-10\n";
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES (?1, 'h', 0)",
                [policy],
            )?;
            tx.execute("INSERT INTO policy_rule_id(id) VALUES (1)", [])?;
            Ok(())
        })
        .unwrap();
    let local = socket::local();
    let me = session(job, "se-1");
    let mut by = Vec::new();
    for who in [&local, &me] {
        let r = add(&store, who, "archive", &["t-1"], json!({})).unwrap();
        let doc = call(
            &store,
            who,
            "submit",
            json!({ "changeset": r["changeset"], "why": "x" }),
        )
        .unwrap();
        by.push(doc["actions"][0]["decision"]["by"].clone());
    }
    // The rule decides for `local` only: the session action goes to a person.
    assert_eq!(by[0], json!("rule"), "{by:?}");
    assert_ne!(by[1], json!("rule"), "{by:?}");
}

#[test]
fn another_job_and_a_missing_id_give_the_same_answer() {
    let (store, job) = fixture("same-answer");
    let other = create(&store, "Another goal.", None, NOW).unwrap();
    add_scope(&store, other, Ref::Thread(1), By::Person, NOW).unwrap();
    let theirs = session(other, "se-9");
    let r = add(&store, &theirs, "archive", &["t-1"], json!({})).unwrap();
    let mine = session(job, "se-1");
    for (verb, extra) in [
        ("show", json!({})),
        ("withdraw", json!({})),
        ("revise", json!({})),
        ("submit", json!({ "why": "x" })),
        ("add", json!({ "verb": "archive", "targets": ["t-1"] })),
    ] {
        let answer = |cs: &str| {
            let mut a = extra.clone();
            a["changeset"] = json!(cs);
            call(&store, &mine, verb, a).unwrap_err()
        };
        let (taken, missing) = (answer(r["changeset"].as_str().unwrap()), answer("cs-999"));
        assert_eq!(taken.0, missing.0, "{verb}");
        assert_eq!(
            taken.1.replace(r["changeset"].as_str().unwrap(), "cs-999"),
            missing.1,
            "{verb}"
        );
    }
}

#[test]
fn a_job_author_with_no_session_changes_nothing() {
    let (store, job) = fixture("no-session");
    let me = session(job, "se-1");
    let r = add(&store, &me, "archive", &["t-1"], json!({})).unwrap();
    let cs = iris_changeset::parse_changeset_id(r["changeset"].as_str().unwrap()).unwrap();
    let bare = Author {
        session: None,
        ..me.clone()
    };
    assert!(iris_changeset::submit(&store, cs, &bare, "x", NOW).is_err());
    assert!(iris_changeset::withdraw(&store, cs, Some(&bare)).is_err());
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM changeset WHERE state = 'draft'"
        ),
        1
    );
}

#[test]
fn a_rule_of_a_job_matches_only_that_job() {
    let (store, job) = fixture("job-rule");
    let other = create(&store, "Another goal.", None, NOW).unwrap();
    add_scope(&store, other, Ref::Thread(1), By::Person, NOW).unwrap();
    let policy = format!(
        "version = 0\nmode = \"apply\"\n\n[[rule]]\nid = 1\nname = \"Job archive\"\neffect = \"allow\"\nverbs = [\"archive\"]\nwhen = {{ author = \"agent\" }}\njob = \"{}\"\nby = \"person\"\nadded = 2026-11-10\n",
        iris_job::format_job_id(job)
    );
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES (?1, 'h', 0)",
                [&policy],
            )?;
            tx.execute("INSERT INTO policy_rule_id(id) VALUES (1)", [])?;
            Ok(())
        })
        .unwrap();
    let mut by = Vec::new();
    for who in [session(job, "se-1"), session(other, "se-2")] {
        let r = add(&store, &who, "archive", &["t-1"], json!({})).unwrap();
        let doc = call(
            &store,
            &who,
            "submit",
            json!({ "changeset": r["changeset"], "why": "x" }),
        )
        .unwrap();
        by.push(doc["actions"][0]["decision"]["by"].clone());
    }
    assert_eq!(by[0], json!("rule"), "{by:?}");
    assert_ne!(by[1], json!("rule"), "{by:?}");
}
