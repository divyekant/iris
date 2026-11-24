//! Tests of the ledger endpoints (card R2-07, specification 2, section 7):
//! the list of entries, and the undo of an entry. The tests write the
//! approved actions and the ledger rows with raw SQL, as the apply does.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_store::{Header, NewMessage, Store};
use rusqlite::params;
use serde_json::{json, Value};
use tower::ServiceExt;

const NOW: i64 = 1_794_787_200_000;

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("iris-api-ledger-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    TempDir(dir)
}

/// Messages 1 to 3, all in the inbox, and a policy with rule 2.
fn store(dir: &TempDir) -> Arc<Store> {
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    s.put_labels(a, &[("INBOX", "INBOX", "system")]).unwrap();
    let m = |id: &str| NewMessage {
        provider_id: id.into(),
        thread_provider_id: format!("t{id}"),
        labels: vec!["INBOX".into()],
        header: Header {
            date: Some(NOW - 3_600_000),
            subject: Some("Hi".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    s.put_messages(a, &[m("g1"), m("g2"), m("g3")]).unwrap();
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('a', 'h1', 0);
             INSERT INTO policy_rule_id(id) VALUES (1), (2);",
        )?;
        Ok(())
    })
    .unwrap();
    Arc::new(s)
}

/// An `archive` of `messages` by `author` (`person`, or `agent` of job 1),
/// approved by `decision`, and its ledger entry in the state `applied`,
/// with each message done. Gives the ledger row ID.
fn archived(s: &Store, author: &str, messages: &[i64], decision: &str) -> i64 {
    archived_why(s, author, messages, decision, Some("Old mail."))
}

/// As `archived`, with the reason `why` of the action.
fn archived_why(
    s: &Store,
    author: &str,
    messages: &[i64],
    decision: &str,
    why: Option<&str>,
) -> i64 {
    archived_args(s, author, messages, decision, why, "{}")
}

/// As `archived_why`, with the arguments `args` (JSON text).
fn archived_args(
    s: &Store,
    author: &str,
    messages: &[i64],
    decision: &str,
    why: Option<&str>,
    args: &str,
) -> i64 {
    s.write(|tx| {
        let job = (author == "agent").then_some("j-1");
        tx.execute(
            "INSERT INTO changeset(author_kind, author_job, created_at) VALUES (?1, ?2, 0)",
            params![author, job],
        )?;
        let cs = tx.last_insert_rowid();
        let ids: Vec<String> = messages.iter().map(|m| format!("\"m-{m}\"")).collect();
        tx.execute(
            "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, why)
             VALUES (?1, 1, 1, 'archive', ?2, ?4, ?3)",
            params![cs, format!("{{\"messages\":[{}]}}", ids.join(",")), why, args],
        )?;
        let a = tx.last_insert_rowid();
        for m in messages {
            tx.execute(
                "INSERT INTO action_message(action_id, message_id) VALUES (?1, ?2)",
                params![a, m],
            )?;
        }
        let (_, h) = iris_changeset::hash(tx, a).unwrap();
        tx.execute("UPDATE changeset_action SET hash = ?2 WHERE id = ?1", params![a, h])?;
        tx.execute(
            "UPDATE changeset SET why = 'Tidy up.', submitted_at = ?2 WHERE id = ?1",
            params![cs, NOW],
        )?;
        tx.execute(
            &format!("UPDATE changeset_action SET state = 'approved', {decision} WHERE id = ?1"),
            [a],
        )?;
        tx.execute(
            "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
             VALUES (?1, 1, '{\"call\":\"batchModify\",\"add\":[],\"remove\":[\"INBOX\"],\"calls\":1}',
                     'writing', 'inverse', ?2)",
            params![a, NOW],
        )?;
        let l = tx.last_insert_rowid();
        for m in messages {
            tx.execute(
                "INSERT INTO ledger_message(ledger_id, message_id, call, pre, post)
                 VALUES (?1, ?2, 1, '[\"INBOX\"]', '[]')",
                params![l, m],
            )?;
        }
        tx.execute("UPDATE ledger SET call = 1 WHERE id = ?1", [l])?;
        tx.execute("UPDATE ledger_message SET done = 1 WHERE ledger_id = ?1", [l])?;
        tx.execute("UPDATE ledger SET state = 'applied' WHERE id = ?1", [l])?;
        Ok(l)
    })
    .unwrap()
}

const PERSON: &str = "decided_by = 'person', decided_at = 1";
const RULE: &str = "decided_by = 'rule', rule = 2, policy = 1, reason = 'Rule 2 (Tidy) allows archive.', decided_at = 1";

async fn call(store: &Arc<Store>, post: bool, uri: &str) -> (StatusCode, Value) {
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let req = if post {
        Request::post(uri)
    } else {
        Request::get(uri)
    }
    .header("host", "127.0.0.1:4800")
    .header("authorization", auth.bearer())
    .body(Body::empty())
    .unwrap();
    let res = iris_api::router(store.clone(), auth)
        .oneshot(req)
        .await
        .unwrap();
    let code = res.status();
    let body = to_bytes(res.into_body(), 64 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

/// Each entry, newest first, page by page: what changed, who decided it,
/// why, and if an undo can start.
#[tokio::test]
async fn lists_each_entry_newest_first() {
    let dir = temp("list");
    let s = store(&dir);
    s.write(|tx| {
        tx.execute(
            "INSERT INTO job(goal, created_at, state, updated_at) VALUES ('Pay the invoice', 0, 'working', 0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let mine = archived(&s, "person", &[1, 2], PERSON);
    let ruled = archived(&s, "agent", &[3], RULE);

    let (code, p) = call(&s, false, "/api/ledger").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(p["next"], Value::Null);
    let e = &p["entries"];
    assert_eq!(e.as_array().unwrap().len(), 2);
    assert_eq!(e[0]["id"], format!("l-{ruled}"));
    assert_eq!(
        e[0]["author"],
        json!({"kind": "agent", "name": null, "job": "j-1", "goal": "Pay the invoice"})
    );
    assert_eq!(e[0]["decided_by"], "rule");
    assert_eq!(e[0]["rule"], 2);
    assert_eq!(e[0]["reason"], "Rule 2 (Tidy) allows archive.");
    assert_eq!(e[1]["id"], format!("l-{mine}"));
    assert_eq!(e[1]["verb"], "archive");
    assert_eq!(e[1]["args"], json!({}));
    assert_eq!(e[1]["state"], "applied");
    assert_eq!(e[1]["messages"], 2);
    assert_eq!(e[1]["changed"], 2);
    assert_eq!(e[1]["items"], 0);
    assert_eq!(e[1]["why"], "Old mail.");
    assert_eq!(e[1]["author"]["kind"], "person");
    assert_eq!(e[1]["author"]["goal"], Value::Null);
    assert_eq!(e[1]["can_undo"], true);
    assert_eq!(e[1]["undo_open"], false);
    assert_eq!(e[1]["undo_of"], Value::Null);

    // Pages of one: each entry once, in the same order.
    let (_, p1) = call(&s, false, "/api/ledger?limit=1").await;
    assert_eq!(p1["entries"][0]["id"], format!("l-{ruled}"));
    let next = p1["next"].as_str().unwrap();
    let (_, p2) = call(&s, false, &format!("/api/ledger?limit=1&after={next}")).await;
    assert_eq!(p2["entries"][0]["id"], format!("l-{mine}"));
    assert_eq!(p2["next"], Value::Null);

    for bad in ["after=0", "after=01", "after=x", "limit=0", "limit=201"] {
        let (code, _) = call(&s, false, &format!("/api/ledger?{bad}")).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{bad}");
    }
}

/// An undo starts one time: the entry shows the open undo, a second undo
/// is a conflict, and the action of the undo names the entry.
#[tokio::test]
async fn an_undo_starts_once() {
    let dir = temp("undo");
    let s = store(&dir);
    let l = archived(&s, "person", &[1, 2], PERSON);

    let (code, e) = call(&s, true, &format!("/api/ledger/l-{l}/undo")).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(e["id"], format!("l-{l}"));
    assert_eq!(e["undo_open"], true);
    assert_eq!(e["can_undo"], false);

    let (code, v) = call(&s, true, &format!("/api/ledger/l-{l}/undo")).await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert!(v["error"].as_str().unwrap().contains("open"), "{v}");

    // The undo is an approved `inbox` of the person, linked to the entry.
    let (verb, state, author): (String, String, String) = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT a.verb, a.state, c.author_kind FROM ledger_undo u
                 JOIN changeset_action a ON a.id = u.action_id JOIN changeset c ON c.id = a.changeset_id
                 WHERE u.ledger_id = ?1",
                [l],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        (verb.as_str(), state.as_str(), author.as_str()),
        ("inbox", "approved", "person")
    );

    // No entry, or an ID that is not canonical: not found, and nothing written.
    for id in ["l-999", "l-0", &format!("l-0{l}"), "1", "cs-1"] {
        let (code, _) = call(&s, true, &format!("/api/ledger/{id}/undo")).await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{id}");
    }
    let undos: i64 = s
        .read(|c| Ok(c.query_row("SELECT count(*) FROM ledger_undo", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(undos, 1);
}

/// The entry of an undo names the entry that it undoes.
#[tokio::test]
async fn the_entry_of_an_undo_names_its_entry() {
    let dir = temp("undo-of");
    let s = store(&dir);
    let l = archived(&s, "person", &[1], PERSON);
    call(&s, true, &format!("/api/ledger/l-{l}/undo")).await;
    s.write(|tx| {
        let a: i64 = tx.query_row("SELECT action_id FROM ledger_undo WHERE ledger_id = ?1", [l], |r| r.get(0))?;
        tx.execute(
            "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
             VALUES (?1, 1, '{\"call\":\"batchModify\",\"add\":[\"INBOX\"],\"remove\":[],\"calls\":1}', 'writing', 'inverse', ?2)",
            params![a, NOW + 1],
        )?;
        Ok(())
    })
    .unwrap();
    let (_, p) = call(&s, false, "/api/ledger").await;
    assert_eq!(p["entries"][0]["state"], "writing");
    assert_eq!(p["entries"][0]["verb"], "inbox");
    assert_eq!(p["entries"][0]["undo_of"], format!("l-{l}"));
    assert_eq!(p["entries"][0]["can_undo"], false);
    assert_eq!(p["entries"][1]["undo_open"], true);

    // The undo ends as the apply ends it: the entry of the undo is applied,
    // and the first entry is undone by it (section 7.4, step 5).
    let u: i64 = s
        .write(|tx| {
            let u: i64 = tx.query_row(
                "SELECT l.id FROM ledger l JOIN ledger_undo lu ON lu.action_id = l.action_id
                 WHERE lu.ledger_id = ?1",
                [l],
                |r| r.get(0),
            )?;
            tx.execute("UPDATE ledger SET state = 'applied' WHERE id = ?1", [u])?;
            tx.execute(
                "UPDATE ledger SET state = 'undone', undone_by = ?2 WHERE id = ?1",
                [l, u],
            )?;
            Ok(u)
        })
        .unwrap();
    let (_, p) = call(&s, false, "/api/ledger").await;
    // The undo changed no message: its one message counts as skipped.
    assert_eq!(p["entries"][0]["skipped"], 1);
    assert_eq!(p["entries"][1]["skipped"], 0);
    assert_eq!(p["entries"][0]["tried"], 1);
    let first = &p["entries"][1];
    assert_eq!(first["id"], format!("l-{l}"));
    assert_eq!(first["state"], "undone");
    assert_eq!(first["undone_by"], format!("l-{u}"));
    assert_eq!(first["undo_open"], false);
    assert_eq!(first["can_undo"], false);
}

/// The entry of an applied undo, with its id. `rows` are the message,
/// `done` and `listed` values of its ledger messages.
fn undo_applied(s: &Store, l: i64, rows: &[(i64, bool, Option<&str>)]) -> i64 {
    s.write(|tx| {
        let a: i64 = tx.query_row(
            "SELECT action_id FROM ledger_undo WHERE ledger_id = ?1 AND step = 1",
            [l],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
             VALUES (?1, 1, '{\"call\":\"batchModify\",\"add\":[\"INBOX\"],\"remove\":[],\"calls\":1}', 'writing', 'inverse', ?2)",
            params![a, NOW + 1],
        )?;
        let u = tx.last_insert_rowid();
        for (m, done, listed) in rows {
            tx.execute(
                "INSERT INTO ledger_message(ledger_id, message_id, call, pre, post)
                 VALUES (?1, ?2, 1, '[]', '[\"INBOX\"]')",
                params![u, m],
            )?;
        }
        tx.execute("UPDATE ledger SET call = 1 WHERE id = ?1", [u])?;
        for (m, done, listed) in rows {
            tx.execute(
                "UPDATE ledger_message SET listed = ?3 WHERE ledger_id = ?1 AND message_id = ?2",
                params![u, m, listed],
            )?;
            if *done {
                tx.execute(
                    "UPDATE ledger_message SET done = 1 WHERE ledger_id = ?1 AND message_id = ?2",
                    params![u, m],
                )?;
            }
        }
        tx.execute("UPDATE ledger SET state = 'applied' WHERE id = ?1", [u])?;
        Ok(u)
    })
    .unwrap()
}

/// The skipped count of an undo comes from the entry that it undoes: a
/// message that is gone before the undo starts counts, and a message that
/// the undo listed but did not change counts once (decision D18).
#[tokio::test]
async fn an_undo_counts_each_target_of_its_entry() {
    let dir = temp("undo-skipped");
    let s = store(&dir);
    let gone = |m: i64| {
        s.write(|tx| {
            tx.execute("UPDATE message SET gone_at = 1 WHERE id = ?1", [m])?;
            Ok(())
        })
        .unwrap()
    };
    let entry = |p: &Value, id: i64| {
        p["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == format!("l-{id}"))
            .unwrap()
            .clone()
    };

    // Message 2 is gone before the undo starts: submit leaves it out, and
    // the undo changes message 1 only.
    let l = archived(&s, "person", &[1, 2], PERSON);
    gone(2);
    let (code, _) = call(&s, true, &format!("/api/ledger/l-{l}/undo")).await;
    assert_eq!(code, StatusCode::OK);
    let u = undo_applied(&s, l, &[(1, true, None)]);
    let (_, p) = call(&s, false, "/api/ledger").await;
    let e = entry(&p, u);
    assert_eq!((e["changed"].clone(), e["tried"].clone(), e["skipped"].clone()), (json!(1), json!(2), json!(1)));
    assert_eq!(entry(&p, l)["tried"], 0);
    assert_eq!(entry(&p, l)["skipped"], 0);

    // Message 3 is gone after the undo starts: the undo lists it and does
    // not change it. It counts once, and the entry has no undo now.
    let l = archived(&s, "person", &[3], PERSON);
    call(&s, true, &format!("/api/ledger/l-{l}/undo")).await;
    gone(3);
    let u = undo_applied(&s, l, &[(3, false, Some("gone"))]);
    let (_, p) = call(&s, false, "/api/ledger").await;
    let e = entry(&p, u);
    assert_eq!((e["changed"].clone(), e["tried"].clone(), e["skipped"].clone()), (json!(0), json!(1), json!(1)));
    assert_eq!(e["retry"], false);
    assert_eq!(entry(&p, l)["can_undo"], false);
}

/// Each value of an entry is bounded: a large argument (the body of a
/// draft has up to 256 KiB) stays out, and each text has at most 2,000
/// characters, so a full page stays small.
#[tokio::test]
async fn a_page_of_large_actions_stays_small() {
    let dir = temp("large");
    let s = store(&dir);
    let long = "\u{e9}".repeat(3000);
    let args = json!({ "body": "x".repeat(256 << 10), "label": long, "other": [1, 2] }).to_string();
    for m in 1..=3 {
        archived_args(&s, "person", &[m], PERSON, Some(&long), &args);
    }
    let (code, p) = call(&s, false, "/api/ledger?limit=200").await;
    assert_eq!(code, StatusCode::OK);
    let size = p.to_string().len();
    assert!(size < 64 << 10, "{size} bytes");
    for e in p["entries"].as_array().unwrap() {
        let keys: Vec<&String> = e["args"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["label"]);
        assert_eq!(e["args"]["label"].as_str().unwrap().chars().count(), 2000);
        assert_eq!(e["why"].as_str().unwrap().chars().count(), 2000);
    }
}

/// An entry with no changed message has no undo: a conflict, with the reason.
#[tokio::test]
async fn an_entry_with_no_change_has_no_undo() {
    let dir = temp("none");
    let s = store(&dir);
    let l = archived(&s, "person", &[1], PERSON);
    s.write(|tx| {
        tx.execute("UPDATE message SET gone_at = 1", [])?;
        Ok(())
    })
    .unwrap();
    let (_, p) = call(&s, false, "/api/ledger").await;
    assert_eq!(p["entries"][0]["can_undo"], false);
    let (code, v) = call(&s, true, &format!("/api/ledger/l-{l}/undo")).await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert!(v["error"].is_string());
}

/// The entry of a state verb names its item, and its undo (`restore`,
/// issue #226) sets the item back.
#[tokio::test]
async fn a_state_verb_names_its_item() {
    let dir = temp("state");
    let s = store(&dir);
    let mut doc = iris_spec::read_json(
        &iris_spec::schema_dir()
            .join("examples")
            .join("commitment-you-owe.json"),
    );
    doc["id"] = json!("c-1");
    doc["status"] = json!("open");
    s.write(|tx| {
        tx.execute(
            "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
             VALUES ('commitment', 1, 'open', ?1, 0, 0)",
            [doc.to_string()],
        )?;
        tx.execute(
            "INSERT INTO item_source(item_id, message_id, role) VALUES (1, 1, 'origin')",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let req = Request::post("/api/items/c-1/close")
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth.bearer())
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let res = iris_api::router(s.clone(), auth)
        .oneshot(req)
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let (_, p) = call(&s, false, "/api/ledger").await;
    let e = &p["entries"][0];
    assert_eq!(e["verb"], "close");
    assert_eq!(e["state"], "applied");
    assert_eq!(e["items"], 1);
    assert_eq!(e["title"], doc["title"]);
    // The title of an item of hostile mail is not shown, and comes back
    // when the state clears.
    s.put_labels(1, &[("SPAM", "SPAM", "system")]).unwrap();
    for (set, clear) in HOSTILE {
        s.write(|tx| Ok(tx.execute_batch(set)?)).unwrap();
        let (_, p) = call(&s, false, "/api/ledger").await;
        assert_eq!(p["entries"][0]["title"], Value::Null, "{set}");
        s.write(|tx| Ok(tx.execute_batch(clear)?)).unwrap();
        let (_, p) = call(&s, false, "/api/ledger").await;
        assert_eq!(p["entries"][0]["title"], doc["title"], "{clear}");
    }
    assert_eq!(e["messages"], 0);
    assert_eq!(e["can_undo"], true);
    let undo = format!("/api/ledger/{}/undo", e["id"].as_str().unwrap());
    let (code, _) = call(&s, true, &undo).await;
    assert_eq!(code, StatusCode::OK);
    // The engine applies the `restore` at its next step: the item is open again.
    iris_apply::apply_states(&s).unwrap();
    let status: String = s
        .read(|c| Ok(c.query_row("SELECT status FROM item WHERE id = 1", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(status, "open");
    let (_, p) = call(&s, false, "/api/ledger").await;
    let first = p["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["verb"] == "close")
        .unwrap();
    assert_eq!(first["state"], "undone");
    let (code, _) = call(&s, true, &undo).await;
    assert_eq!(code, StatusCode::CONFLICT, "no second undo");
}

/// An action with no reason of its own shows the reason of its revision,
/// also after the author submits a new revision with another reason.
#[tokio::test]
async fn an_entry_keeps_the_reason_of_its_revision() {
    let dir = temp("revision");
    let s = store(&dir);
    archived_why(&s, "person", &[1], PERSON, None);
    let cs: i64 = s
        .read(|c| Ok(c.query_row("SELECT id FROM changeset", [], |r| r.get(0))?))
        .unwrap();
    let person = iris_changeset::Author {
        kind: iris_changeset::AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    };
    iris_changeset::revise(&s, cs, &person).unwrap();
    iris_changeset::add_action(
        &s,
        cs,
        &person,
        iris_changeset::Verb::Archive,
        &["m-2"],
        &serde_json::Map::new(),
        None,
    )
    .unwrap();
    iris_changeset::submit(&s, cs, &person, "Another reason.", NOW + 5).unwrap();
    let (_, p) = call(&s, false, "/api/ledger").await;
    assert_eq!(p["entries"][0]["why"], "Tidy up.");
}

/// The three states of hostile mail of message 1 (issue 112): the SQL that
/// sets each one, and the SQL that clears it.
const HOSTILE: [(&str, &str); 3] = [
    (
        "UPDATE message SET hostile = '[\"lookalike\"]' WHERE id = 1",
        "UPDATE message SET hostile = '[]' WHERE id = 1",
    ),
    (
        "INSERT INTO message_label(message_id, label_id)
           SELECT 1, id FROM label WHERE provider_id = 'SPAM'",
        "DELETE FROM message_label WHERE message_id = 1
           AND label_id IN (SELECT id FROM label WHERE provider_id = 'SPAM')",
    ),
    (
        "UPDATE header SET from_addr = 'bad@example.com' WHERE message_id = 1;
         INSERT INTO sender(address, kind) VALUES ('bad@example.com', 'hostile')
           ON CONFLICT(address) DO UPDATE SET kind = 'hostile'",
        "UPDATE sender SET kind = 'person' WHERE address = 'bad@example.com'",
    ),
];
