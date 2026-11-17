//! The routes of matters (card R6-01): the list, one matter, the confirm and
//! the decline. Each matter document passes `matter.schema.json`, and the
//! `matter` field of each item comes from the store.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Method, Request, StatusCode};
use iris_api::Auth;
use iris_store::{NewMessage, Store};
use rusqlite::params;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn call(store: &Arc<Store>, method: Method, uri: &str, body: &str) -> (StatusCode, Value) {
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let req = Request::builder()
        .method(method)
        .uri(uri)
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

/// Items 1 to 5: record, conversation, commitment, record, commitment, in
/// threads 0, 1, 2, 0, 1. Proposal g-1 holds items 1, 2 and 3; g-2 holds 4
/// and 5.
fn setup(name: &str) -> (std::path::PathBuf, Arc<Store>) {
    let dir = std::env::temp_dir().join(format!("iris-api-matters-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Arc::new(Store::open(&dir).unwrap());
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    let msgs: Vec<NewMessage> = (0..3)
        .map(|t| NewMessage {
            provider_id: format!("g{t}"),
            thread_provider_id: format!("gt{t}"),
            ..Default::default()
        })
        .collect();
    let ids = s.put_messages(a, &msgs).unwrap();
    let forms = [
        "record",
        "conversation",
        "commitment",
        "record",
        "commitment",
    ];
    s.write(|tx| {
        for (i, form) in forms.iter().enumerate() {
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES (?1, ?2, 'open', ?3, 0, 0)",
                params![form, a, json!({"title": format!("Item {i}")}).to_string()],
            )?;
            tx.execute(
                "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
                params![tx.last_insert_rowid(), ids[i % 3]],
            )?;
        }
        Ok(())
    })
    .unwrap();
    for (title, items) in [("Tokyo trip", vec![1, 2, 3]), ("Lease", vec![4, 5])] {
        iris_matters::write(&s, |tx| {
            iris_matters::insert_proposal(tx, title, "fake-model", None, &items, 1_000)
        })
        .unwrap();
    }
    (dir, s)
}

/// The document and the members of a view name the same items, with the
/// same counts.
fn agree(v: &Value) {
    let mut named: Vec<&str> = Vec::new();
    for form in ["commitment", "conversation", "record"] {
        let list = v["items"][form].as_array().unwrap();
        assert_eq!(v["matter"]["counts"][form], list.len(), "{v}");
        named.extend(list.iter().map(|d| d["id"].as_str().unwrap()));
    }
    let mut doc: Vec<&str> = v["matter"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i.as_str().unwrap())
        .collect();
    named.sort_unstable();
    doc.sort_unstable();
    assert_eq!(named, doc, "{v}");
}

fn ids(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn list_view_confirm_and_decline() {
    let (dir, s) = setup("flow");
    let schema = iris_spec::validator("matter");

    let (code, list) = call(&s, Method::GET, "/api/matters", "").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(ids(&list["proposed"]), vec!["g-2", "g-1"]);
    for m in list["proposed"].as_array().unwrap() {
        assert!(schema.is_valid(m), "{m}");
    }
    let (code, v) = call(&s, Method::GET, "/api/matters/g-1", "").await;
    assert_eq!(code, StatusCode::OK);
    agree(&v);
    assert_eq!(v["matter"]["items"], json!(["r-1", "v-2", "c-3"]));
    assert_eq!(
        v["matter"]["counts"],
        json!({"commitment": 1, "conversation": 1, "record": 1})
    );
    assert_eq!(v["items"]["record"][0]["id"], "r-1");
    assert_eq!(v["items"]["record"][0]["matter"], Value::Null);

    // Bad confirms change nothing.
    for (body, want) in [
        (r#"{"title":"Trip","items":[]}"#, StatusCode::BAD_REQUEST),
        (r#"{"title":"","items":["r-1"]}"#, StatusCode::BAD_REQUEST),
        (
            r#"{"title":"Trip","items":["m-1"]}"#,
            StatusCode::BAD_REQUEST,
        ),
        // The prefix must agree with the form of the row: row 1 is a record.
        (
            r#"{"title":"Trip","items":["v-1"]}"#,
            StatusCode::BAD_REQUEST,
        ),
        (
            r#"{"title":"Trip","items":["r-4"]}"#,
            StatusCode::BAD_REQUEST,
        ),
        (r#"{"title":"Trip"}"#, StatusCode::BAD_REQUEST),
    ] {
        let (code, _) = call(&s, Method::POST, "/api/matters/g-1/confirm", body).await;
        assert_eq!(code, want, "{body}");
    }
    let (code, _) = call(
        &s,
        Method::POST,
        "/api/matters/g-9/confirm",
        r#"{"title":"T","items":["r-1"]}"#,
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    // Confirm with two of three members.
    let (code, v) = call(
        &s,
        Method::POST,
        "/api/matters/g-1/confirm",
        r#"{"title":"Tokyo","items":["r-1","c-3"]}"#,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    agree(&v);
    assert!(schema.is_valid(&v["matter"]), "{}", v["matter"]);
    assert_eq!(v["matter"]["status"], "confirmed");
    assert_eq!(v["matter"]["title"], "Tokyo");
    assert_eq!(v["matter"]["items"], json!(["r-1", "c-3"]));
    assert_eq!(v["items"]["conversation"], json!([]));
    // A second click is a conflict.
    let (code, _) = call(
        &s,
        Method::POST,
        "/api/matters/g-1/confirm",
        r#"{"title":"Tokyo","items":["r-1"]}"#,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);

    // The `matter` field of an item comes from the store: kept members only.
    let (_, item) = call(&s, Method::GET, "/api/items/r-1", "").await;
    assert_eq!(item["matter"], "g-1");
    let (_, item) = call(&s, Method::GET, "/api/items/v-2", "").await;
    assert_eq!(item["matter"], Value::Null);
    let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
    assert_eq!(ids(&list["proposed"]), vec!["g-2"]);
    assert_eq!(ids(&list["confirmed"]), vec!["g-1"]);

    // Decline: the full grouping stays in the document.
    let (code, v) = call(&s, Method::POST, "/api/matters/g-2/decline", "").await;
    assert_eq!(code, StatusCode::OK);
    agree(&v);
    assert!(schema.is_valid(&v["matter"]), "{}", v["matter"]);
    assert_eq!(v["matter"]["items"], json!(["r-4", "c-5"]));
    let (code, _) = call(&s, Method::POST, "/api/matters/g-2/decline", "").await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
    assert_eq!(ids(&list["proposed"]), Vec::<&str>::new());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_stale_proposal_is_not_shown_and_not_decided() {
    let (dir, s) = setup("stale");
    // Item 5 goes: g-2 keeps one live member, so it is not valid.
    s.write(|tx| Ok(tx.execute("UPDATE item SET status = 'gone' WHERE id = 5", [])?))
        .unwrap();
    let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
    assert_eq!(ids(&list["proposed"]), vec!["g-1"]);
    let (code, _) = call(&s, Method::GET, "/api/matters/g-2", "").await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = call(&s, Method::POST, "/api/matters/g-2/decline", "").await;
    assert_eq!(code, StatusCode::CONFLICT);
    // A hidden member of a valid proposal is not in its document.
    s.write(|tx| {
        Ok(tx.execute(
            r#"UPDATE item SET doc = json_set(doc, '$.attention.state', 'hidden') WHERE id = 2"#,
            [],
        )?)
    })
    .unwrap();
    let (_, v) = call(&s, Method::GET, "/api/matters/g-1", "").await;
    agree(&v);
    assert_eq!(v["matter"]["items"], json!(["r-1", "c-3"]));
    let (code, _) = call(&s, Method::GET, "/api/matters/x-1", "").await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let _ = std::fs::remove_dir_all(dir);
}

/// The answer to a confirm comes from the state of the matter first: a
/// matter that is gone answers 404, and a proposal that is not valid now
/// answers 409, for each list of members, with a right or a wrong kind.
#[tokio::test]
async fn a_confirm_answers_the_state_of_the_matter_first() {
    let (dir, s) = setup("first");
    // Item 5 goes from the store: the store deletes proposal g-2.
    s.write(|tx| Ok(tx.execute("DELETE FROM item WHERE id = 5", [])?))
        .unwrap();
    // Items 2 and 3 are gone: g-1 keeps one live member, so it is not valid.
    s.write(|tx| Ok(tx.execute("UPDATE item SET status = 'gone' WHERE id IN (2, 3)", [])?))
        .unwrap();
    let lists = [
        vec!["r-4", "c-5"],
        vec!["c-4", "r-5"],
        vec!["r-4"],
        vec!["v-5"],
        vec!["r-1", "v-2"],
        vec!["r-1", "c-2"],
        vec!["v-1"],
    ];
    for (matter, want) in [
        ("g-2", StatusCode::NOT_FOUND),
        ("g-1", StatusCode::CONFLICT),
    ] {
        for items in &lists {
            let body = json!({"title": "Trip", "items": items}).to_string();
            let (code, _) = call(
                &s,
                Method::POST,
                &format!("/api/matters/{matter}/confirm"),
                &body,
            )
            .await;
            assert_eq!(code, want, "{matter} {items:?}");
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Decisions commit while another thread reads: each list document passes
/// the schema, and each view names the same members in both parts.
#[test]
fn reads_during_decisions_are_consistent() {
    let dir = std::env::temp_dir().join(format!("iris-api-matters-race-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Arc::new(Store::open(&dir).unwrap());
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    let msgs: Vec<NewMessage> = (0..2)
        .map(|t| NewMessage {
            provider_id: format!("g{t}"),
            thread_provider_id: format!("gt{t}"),
            ..Default::default()
        })
        .collect();
    let ids = s.put_messages(a, &msgs).unwrap();
    const N: i64 = 40;
    s.write(|tx| {
        for i in 0..2 * N {
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES ('record', ?1, 'kept', '{}', 0, 0)",
                [a],
            )?;
            tx.execute(
                "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
                params![tx.last_insert_rowid(), ids[(i % 2) as usize]],
            )?;
        }
        Ok(())
    })
    .unwrap();
    for m in 0..N {
        iris_matters::write(&s, |tx| {
            iris_matters::insert_proposal(tx, "T", "fake", None, &[2 * m + 1, 2 * m + 2], 1_000)
        })
        .unwrap();
    }
    let writer = {
        let s = s.clone();
        std::thread::spawn(move || {
            for m in 1..=N {
                let id = format!("g-{m}");
                let keep = [format!("r-{}", 2 * m - 1)];
                if m % 2 == 0 {
                    iris_api::confirm_matter(&s, &id, "Kept", &keep, 2_000).unwrap();
                } else {
                    iris_api::decline_matter(&s, &id, 2_000).unwrap();
                }
            }
        })
    };
    let schema = iris_spec::validator("matter");
    let mut reads = 0;
    while !writer.is_finished() || reads < 20 {
        let list = iris_api::matters(&s).unwrap();
        for key in ["proposed", "confirmed"] {
            for m in list[key].as_array().unwrap() {
                assert!(schema.is_valid(m), "{m}");
            }
        }
        let id = format!("g-{}", 1 + reads % N);
        if let Some(v) = iris_api::matter(&s, &id).unwrap() {
            agree(&v);
            assert!(schema.is_valid(&v["matter"]), "{v}");
        }
        reads += 1;
    }
    writer.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
