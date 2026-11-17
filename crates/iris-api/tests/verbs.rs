//! Tests of the state verbs of the web client (specification 2, section 4):
//! `POST /api/items/{id}/close`, `/decline`, `/defer`, `/skip`, `/mute` and
//! `/dismiss`.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_store::{NewMessage, Store};
use serde_json::{json, Value};
use tower::ServiceExt;

/// A store with the commitment `c-1` of the status `status`, in the mode
/// `observe` (no policy).
fn store(name: &str, status: &str) -> Arc<Store> {
    store_of(name, "commitment", "commitment-you-owe.json", "c-1", status)
}

/// A store with one item of the form `form` (row 1, ID `id`), from the
/// schema example `example`, with the status `status`.
fn store_of(name: &str, form: &str, example: &str, id: &str, status: &str) -> Arc<Store> {
    let dir = std::env::temp_dir().join(format!("iris-api-verbs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Store::open(&dir).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    let m = NewMessage {
        provider_id: "g1".into(),
        thread_provider_id: "gt1".into(),
        ..Default::default()
    };
    s.put_messages(a, &[m]).unwrap();
    let mut doc = iris_spec::read_json(&iris_spec::schema_dir().join("examples").join(example));
    doc["id"] = json!(id);
    doc["status"] = json!(status);
    s.write(|tx| {
        tx.execute(
            "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
             VALUES (?4, ?1, ?2, ?3, 0, 0)",
            rusqlite::params![a, status, doc.to_string(), form],
        )?;
        tx.execute(
            "INSERT INTO item_source(item_id, message_id, role) VALUES (1, 1, 'origin')",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    Arc::new(s)
}

async fn post(store: &Arc<Store>, uri: &str, body: &str) -> (StatusCode, Value) {
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let req = Request::post(uri)
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth.bearer())
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = iris_api::router(store.clone(), auth)
        .oneshot(req)
        .await
        .unwrap();
    let code = res.status();
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn one(s: &Store, sql: &str) -> String {
    s.read(|c| Ok(c.query_row(sql, [], |r| r.get::<_, String>(0))?))
        .unwrap()
}

fn count(s: &Store, sql: &str) -> i64 {
    s.read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

/// Each verb makes a person changeset with one action, approved by the
/// person, and applies it through the ledger, also in the mode `observe`.
#[tokio::test]
async fn each_verb_changes_the_commitment() {
    for (verb, body, status, until) in [
        ("close", "{}", "done", Value::Null),
        (
            "decline",
            r#"{"reason":"Not mine."}"#,
            "declined",
            Value::Null,
        ),
        (
            "defer",
            r#"{"date":"2099-01-05"}"#,
            "deferred",
            json!("2099-01-05"),
        ),
    ] {
        let s = store(verb, "open");
        let (code, item) = post(&s, &format!("/api/items/c-1/{verb}"), body).await;
        assert_eq!(code, StatusCode::OK, "{verb}: {item}");
        assert_eq!(item["status"], json!(status), "{verb}");
        assert_eq!(item["deferred_until"], until, "{verb}");
        assert_eq!(
            one(
                &s,
                "SELECT c.author_kind || ' ' || a.decided_by || ' ' || a.state || ' ' || l.state
                 FROM changeset c JOIN changeset_action a ON a.changeset_id = c.id
                 JOIN ledger l ON l.action_id = a.id"
            ),
            "person person applied applied",
            "{verb}"
        );
    }
    // The reason of the person is the reason of the action and the
    // changeset. With no reason, a fixed one.
    let s = store("reason", "open");
    post(&s, "/api/items/c-1/decline", r#"{"reason":"Not mine."}"#).await;
    assert_eq!(
        one(
            &s,
            "SELECT json_extract(args, '$.reason') FROM changeset_action"
        ),
        "Not mine."
    );
    let s = store("default", "open");
    post(&s, "/api/items/c-1/close", "{}").await;
    assert_eq!(
        one(
            &s,
            "SELECT json_extract(args, '$.reason') FROM changeset_action"
        ),
        "Done."
    );
}

/// Each request acts on its own new action, also in a store with earlier
/// changesets: defer, then close the same commitment.
#[tokio::test]
async fn each_request_applies_its_own_action() {
    let s = store("two", "open");
    let (code, item) = post(&s, "/api/items/c-1/defer", r#"{"date":"2099-01-05"}"#).await;
    assert_eq!(code, StatusCode::OK, "{item}");
    let (code, item) = post(&s, "/api/items/c-1/close", "{}").await;
    assert_eq!(code, StatusCode::OK, "{item}");
    assert_eq!(item["status"], json!("done"));
    assert_eq!(
        one(
            &s,
            "SELECT group_concat(a.verb || ' ' || l.state, ', ') FROM changeset_action a
             JOIN ledger l ON l.action_id = a.id"
        ),
        "defer applied, close applied"
    );
}

/// A commitment that the verb cannot change gives 409 with the reason, and
/// a failed ledger entry. The item does not change.
#[tokio::test]
async fn a_closed_commitment_does_not_change() {
    let s = store("done", "done");
    let (code, body) = post(&s, "/api/items/c-1/defer", r#"{"date":"2099-01-05"}"#).await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert!(
        body["error"].as_str().unwrap().contains("c-1 is done"),
        "{body}"
    );
    assert_eq!(one(&s, "SELECT state FROM ledger"), "failed");
    assert_eq!(one(&s, "SELECT status FROM item"), "done");
}

/// A request that is not valid writes nothing: no changeset stays.
#[tokio::test]
async fn a_bad_request_writes_nothing() {
    let s = store("bad", "open");
    for (uri, body, want) in [
        ("/api/items/c-1/defer", "{}", StatusCode::BAD_REQUEST),
        (
            "/api/items/c-1/defer",
            r#"{"date":"soon"}"#,
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/items/c-1/close",
            r#"{"date":"2099-01-05"}"#,
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/items/c-1/close",
            r#"{"reason":"  "}"#,
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/items/c-1/close",
            r#"{"why":"x"}"#,
            StatusCode::BAD_REQUEST,
        ),
        ("/api/items/c-1/close", "not json", StatusCode::BAD_REQUEST),
        ("/api/items/c-9/close", "{}", StatusCode::NOT_FOUND),
        ("/api/items/c-1/trash", "{}", StatusCode::NOT_FOUND),
    ] {
        let (code, b) = post(&s, uri, body).await;
        assert_eq!(code, want, "{uri} {body}: {b}");
    }
    let long = format!(r#"{{"reason":"{}"}}"#, "x".repeat(iris_api::MAX_REASON + 1));
    let (code, _) = post(&s, "/api/items/c-1/close", &long).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert_eq!(count(&s, "SELECT count(*) FROM changeset"), 0);
    assert_eq!(one(&s, "SELECT status FROM item"), "open");
}

/// The verbs of R5-02 (docs/specs/2026-11-17-preference-rules.md): each
/// moves its form from each allowed status to its own status, through a
/// person changeset and the ledger. Each other status gives 409 and no
/// change. Another form gives 400 and writes nothing.
#[tokio::test]
async fn skip_mute_and_dismiss_change_their_form_only() {
    let cases = [
        (
            "skip",
            "feed",
            "feed.json",
            "f-1",
            "skipped",
            &["new", "read"][..],
            &["skipped", "gone"][..],
        ),
        (
            "mute",
            "conversation",
            "conversation.json",
            "v-1",
            "muted",
            &["active", "quiet"][..],
            &["muted", "gone"][..],
        ),
        (
            "dismiss",
            "signal",
            "signal-code.json",
            "s-1",
            "dismissed",
            &["active"][..],
            &["expired", "used", "dismissed", "gone"][..],
        ),
    ];
    for (verb, form, example, id, to, from, not) in cases {
        for status in from {
            let s = store_of(&format!("{verb}-{status}"), form, example, id, status);
            let (code, item) = post(&s, &format!("/api/items/{id}/{verb}"), "{}").await;
            assert_eq!(code, StatusCode::OK, "{verb} {status}: {item}");
            assert_eq!(item["status"], json!(to), "{verb} {status}");
            assert_eq!(one(&s, "SELECT status FROM item"), to);
            assert_eq!(
                one(
                    &s,
                    "SELECT c.author_kind || ' ' || a.decided_by || ' ' || l.state || ' ' || l.pre || ' ' || l.post
                     FROM changeset c JOIN changeset_action a ON a.changeset_id = c.id
                     JOIN ledger l ON l.action_id = a.id"
                ),
                format!(
                    r#"person person applied {{"{id}":{{"status":"{status}"}}}} {{"{id}":{{"status":"{to}"}}}}"#
                ),
                "{verb} {status}"
            );
        }
        for status in not {
            let s = store_of(&format!("{verb}-no-{status}"), form, example, id, status);
            let (code, body) = post(&s, &format!("/api/items/{id}/{verb}"), "{}").await;
            assert_eq!(code, StatusCode::CONFLICT, "{verb} {status}: {body}");
            assert_eq!(one(&s, "SELECT state FROM ledger"), "failed");
            assert_eq!(one(&s, "SELECT status FROM item"), *status);
        }
    }
    // Another form: the submit denies the action, and nothing stays.
    let s = store("skip-commitment", "open");
    for verb in ["skip", "mute", "dismiss"] {
        let (code, body) = post(&s, &format!("/api/items/c-1/{verb}"), "{}").await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{verb}: {body}");
    }
    assert_eq!(count(&s, "SELECT count(*) FROM changeset"), 0);
    assert_eq!(one(&s, "SELECT status FROM item"), "open");
}
