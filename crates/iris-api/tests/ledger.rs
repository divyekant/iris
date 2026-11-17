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
             VALUES (?1, 1, 1, 'archive', ?2, '{}', 'Old mail.')",
            params![cs, format!("{{\"messages\":[{}]}}", ids.join(","))],
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

/// The entry of a state verb names its item, and has no undo yet (issue #226).
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
    assert_eq!(e["messages"], 0);
    assert_eq!(e["can_undo"], false);
    let (code, _) = call(
        &s,
        true,
        &format!("/api/ledger/{}/undo", e["id"].as_str().unwrap()),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);
}
