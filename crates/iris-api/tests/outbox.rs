//! The outbox of the web client (docs/specs/2026-11-17-held-outbox.md,
//! section 6): GET /api/outbox and POST /api/outbox/{d-N}/cancel.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_changeset::{add_action, create, submit, Author, AuthorKind, Verb};
use iris_store::Store;
use serde_json::{json, Value};
use tower::ServiceExt;

const OWNER: &str = "owner@example.com";

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("iris-api-outbox-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    TempDir(dir)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Account 1 and draft d-1: a new mail to priya that the apply made.
fn store(dir: &TempDir) -> Arc<Store> {
    let s = Store::open(&dir.0).unwrap();
    s.add_account("gmail", OWNER).unwrap();
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO changeset(author_kind, created_at) VALUES ('person', 0);
             INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
               VALUES (1, 1, 1, 'draft', '{\"accounts\":[\"a-1\"]}', '{}');",
        )?;
        let c = iris_compose::Content {
            to: vec!["priya@example.com".into()],
            cc: vec!["dana@example.com".into()],
            subject: "Q4 plan".into(),
            body: "Hello".into(),
        }
        .check()
        .unwrap();
        let raw = iris_compose::compose(OWNER, "iris.1@example.com", &c, None, 1);
        tx.execute(
            "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                               action_id, created_at, updated_at)
             VALUES (1, 'r1', 'iris.1@example.com', '{}', ?1, ?2, 1, 0, 0)",
            rusqlite::params![iris_compose::hash(&c, None), raw],
        )?;
        Ok(())
    })
    .unwrap();
    Arc::new(s)
}

/// Submit a send of d-1 as the person and approve it at `at`. Gives its ID.
fn approved_send(s: &Store, at: i64) -> i64 {
    approved_send_of(s, "d-1", at)
}

/// Drafts d-2 to d-`last`: copies of d-1, each from its own draft action.
fn more_drafts(s: &Store, last: i64) {
    s.write(|tx| {
        for n in 2..=last {
            tx.execute(
                "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                 VALUES (1, 1, ?1, 'draft', '{\"accounts\":[\"a-1\"]}', '{}')",
                [n],
            )?;
            let action = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                                   action_id, created_at, updated_at)
                 SELECT account_id, 'r' || ?1, 'iris.' || ?1 || '@example.com', content, hash,
                        raw, ?2, 0, 0 FROM draft WHERE id = 1",
                rusqlite::params![n, action],
            )?;
        }
        Ok(())
    })
    .unwrap();
}

/// Submit a send of `draft` as the person and approve it at `at`. Gives its ID.
fn approved_send_of(s: &Store, draft: &str, at: i64) -> i64 {
    let who = Author {
        kind: AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    };
    let cs = create(s, &who, at).unwrap();
    add_action(s, cs, &who, Verb::Send, &[draft], &Default::default(), None).unwrap();
    submit(s, cs, &who, "Send.", at).unwrap();
    s.write(|tx| {
        let id: i64 = tx.query_row(
            "SELECT id FROM changeset_action WHERE changeset_id = ?1",
            [cs],
            |r| r.get(0),
        )?;
        tx.execute(
            "UPDATE changeset_action SET state = 'approved', decided_by = 'person',
               decided_at = ?2 WHERE id = ?1",
            rusqlite::params![id, at],
        )?;
        Ok(id)
    })
    .unwrap()
}

async fn call(store: &Arc<Store>, method: &str, uri: &str) -> (StatusCode, Value) {
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth.bearer())
        .body(Body::empty())
        .unwrap();
    let res = iris_api::router(store.clone(), auth)
        .oneshot(req)
        .await
        .unwrap();
    let code = res.status();
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn state(s: &Store, action: i64) -> (String, String) {
    s.read(|c| {
        Ok(c.query_row(
            "SELECT state, coalesce(decided_by, '') FROM changeset_action WHERE id = ?1",
            [action],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    })
    .unwrap()
}

/// The outbox lists an approved send with its recipients and subject from
/// the stored bytes. While the switch is off, no send has a time to leave.
/// When the switch is on, a send leaves 60 seconds after the later of its
/// approval and the switch.
#[tokio::test]
async fn the_outbox_lists_each_send_that_waits() {
    let dir = temp("list");
    let s = store(&dir);
    let (code, empty) = call(&s, "GET", "/api/outbox").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        empty,
        json!({"sending": {"on": false, "since": null}, "count": 0, "sends": [], "next": null})
    );
    let at = now() - 10_000;
    let a = approved_send(&s, at);
    let (_, v) = call(&s, "GET", "/api/outbox").await;
    let send = &v["sends"][0];
    assert_eq!(send["draft"], "d-1", "{v}");
    assert_eq!(send["state"], "approved");
    assert_eq!(send["readable"], true);
    assert_eq!(send["to"], json!(["priya@example.com"]));
    assert_eq!(send["cc"], json!(["dana@example.com"]));
    assert_eq!(send["subject"], "Q4 plan");
    assert_eq!(send["leaves"], Value::Null);
    assert!(
        send["flags"]
            .as_array()
            .unwrap()
            .contains(&json!("new_recipient")),
        "{send}"
    );
    // The engine moves it to held; the switch goes on after the approval.
    iris_changeset::hold(&s, 1, now()).unwrap();
    iris_apply::set_sending(&s, true).unwrap();
    let on = iris_apply::sending(&s).unwrap().unwrap();
    let (_, v) = call(&s, "GET", "/api/outbox").await;
    assert_eq!(v["sending"]["on"], true);
    assert_eq!(v["sends"][0]["state"], "held");
    let leaves = v["sends"][0]["leaves"].as_str().unwrap();
    let want = iris_api::rfc3339(on.max(at) + iris_apply::HOLD_MS);
    assert_eq!(leaves, want);
    assert_eq!(state(&s, a).0, "held");
}

/// Each send that waits is reachable: the pages, from `next` to `next`,
/// give `count` sends, each one time, for each page size. Red if a page
/// drops a send, as a fixed limit with no next page did.
#[tokio::test]
async fn each_send_that_waits_is_on_a_page() {
    let dir = temp("pages");
    let s = store(&dir);
    // Seven open sends, one for each draft.
    more_drafts(&s, 7);
    let want: Vec<Value> = (1..=7)
        .map(|n| {
            approved_send_of(&s, &format!("d-{n}"), now() - 60_000 + n);
            json!(format!("d-{n}"))
        })
        .collect();
    for limit in 1..=8 {
        let mut got = Vec::new();
        let mut after = String::new();
        loop {
            let (code, v) = call(
                &s,
                "GET",
                &format!("/api/outbox?limit={limit}&after={after}"),
            )
            .await;
            assert_eq!(code, StatusCode::OK, "{v}");
            assert_eq!(v["count"], 7, "{v}");
            let page = v["sends"].as_array().unwrap();
            assert!(page.len() <= limit, "{v}");
            got.extend(page.iter().map(|x| x["draft"].clone()));
            match v["next"].as_str() {
                Some(n) => after = n.to_string(),
                None => break,
            }
        }
        assert_eq!(got, want, "limit {limit}");
    }
    // A page of 200 is the most; a bad cursor is an error, not an empty page.
    assert_eq!(
        call(&s, "GET", "/api/outbox?limit=201").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(&s, "GET", "/api/outbox?after=x").await.0,
        StatusCode::BAD_REQUEST
    );
    let (_, v) = call(&s, "GET", "/api/outbox").await;
    assert_eq!(
        (v["sends"].as_array().unwrap().len(), v["next"].clone()),
        (7, Value::Null)
    );
}

/// A cancel in the outbox denies the send by the person. A second cancel,
/// and a cancel of an ID that is not a draft, find nothing.
#[tokio::test]
async fn a_cancel_in_the_outbox_removes_the_send() {
    let dir = temp("cancel");
    let s = store(&dir);
    let a = approved_send(&s, now());
    iris_changeset::hold(&s, 1, now()).unwrap();
    let (code, v) = call(&s, "POST", "/api/outbox/d-1/cancel").await;
    assert_eq!((code, v), (StatusCode::OK, json!({"cancelled": "d-1"})));
    assert_eq!(state(&s, a), ("denied".into(), "person".into()));
    let (_, v) = call(&s, "GET", "/api/outbox").await;
    assert_eq!(v["sends"], json!([]));
    for uri in [
        "/api/outbox/d-1/cancel",
        "/api/outbox/d-9/cancel",
        "/api/outbox/t-1/cancel",
    ] {
        assert_eq!(
            call(&s, "POST", uri).await.0,
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
}

/// A send that the release claimed cannot be cancelled: 409, and no change.
#[tokio::test]
async fn a_claimed_send_cannot_be_cancelled() {
    let dir = temp("claimed");
    let s = store(&dir);
    let a = approved_send(&s, now());
    iris_changeset::hold(&s, 1, now()).unwrap();
    s.write(|tx| {
        tx.execute(
            "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
             VALUES (?1, 1, '{\"call\":\"send\"}', 'writing', 'none', 1)",
            [a],
        )?;
        Ok(())
    })
    .unwrap();
    let (code, _) = call(&s, "POST", "/api/outbox/d-1/cancel").await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert_eq!(state(&s, a).0, "held");
}

/// The API cannot turn sending on: no route changes the switch.
#[tokio::test]
async fn the_api_cannot_change_the_switch() {
    let dir = temp("switch");
    let s = store(&dir);
    for uri in [
        "/api/outbox",
        "/api/outbox/sending",
        "/api/sending",
        "/api/sending/on",
    ] {
        let (code, _) = call(&s, "POST", uri).await;
        assert!(code.is_client_error(), "{uri}: {code}");
    }
    assert_eq!(iris_apply::sending(&s).unwrap(), None);
}

/// One outbox answer is one snapshot of the store: while sends are approved
/// at the same time, the count always equals the sends of a last page.
/// Red if the count and the page come from two reads (a send approved
/// between them makes the page one longer than the count).
#[test]
fn one_outbox_answer_is_one_snapshot() {
    let dir = temp("snapshot");
    let s = store(&dir);
    const N: i64 = 120;
    more_drafts(&s, N);
    let writer = {
        let s = s.clone();
        std::thread::spawn(move || {
            for n in 1..=N {
                approved_send_of(&s, &format!("d-{n}"), now() - 60_000);
            }
        })
    };
    let mut reads = 0;
    while !writer.is_finished() || reads == 0 {
        let v = iris_api::outbox(&s, iris_api::MAX_SENDS, None).unwrap();
        assert_eq!(v["next"], Value::Null, "{v}");
        assert_eq!(
            v["count"].as_u64().unwrap() as usize,
            v["sends"].as_array().unwrap().len(),
            "{v}"
        );
        reads += 1;
    }
    writer.join().unwrap();
}
