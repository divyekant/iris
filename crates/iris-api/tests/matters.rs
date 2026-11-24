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
    for form in ["commitment", "conversation", "record", "signal"] {
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
        json!({"commitment": 1, "conversation": 1, "record": 1, "signal": 0})
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

/// A route that takes no body refuses each other body with 400, before it
/// changes anything: the decline of a valid proposal stays undone. No
/// body, and the JSON object `{}` of the web client, pass.
#[tokio::test]
async fn a_route_with_no_body_refuses_a_body() {
    let (dir, s) = setup("nobody");
    let bad = [
        "{",
        "x",
        "[]",
        "null",
        "1",
        "\"\"",
        "{\"a\": 1}",
        "{} {}",
        " ",
        "\n",
    ];
    for body in bad {
        let (code, _) = call(&s, Method::POST, "/api/matters/g-1/decline", body).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{body}");
        // The other routes with no body: the check comes before the lookup of the ID.
        for uri in [
            "/api/senders/p-999/accept",
            "/api/changesets/x-999/withdraw",
            "/api/policy/discard",
            "/api/notices/n-999/done",
            "/api/watches/w-999/remove",
        ] {
            let (code, _) = call(&s, Method::POST, uri, body).await;
            assert_eq!(code, StatusCode::BAD_REQUEST, "{uri} {body}");
        }
    }
    let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
    assert_eq!(ids(&list["proposed"]), vec!["g-2", "g-1"]);
    let (code, _) = call(&s, Method::POST, "/api/matters/g-1/decline", " {} ").await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = call(&s, Method::POST, "/api/matters/g-2/decline", "").await;
    assert_eq!(code, StatusCode::OK);
    let _ = std::fs::remove_dir_all(dir);
}

/// The rule of a body on a route that takes none, written apart from the
/// code: zero bytes, or `{` and `}` with only JSON whitespace around and
/// between them, in 64 bytes or fewer.
fn no_body(b: &[u8]) -> bool {
    let ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r');
    let inner: Vec<u8> = b.iter().copied().filter(|c| !ws(c)).collect();
    b.is_empty() || (inner == b"{}" && b.len() <= 64)
}

/// Generated bodies: each gets 400 unless it follows the rule, and a
/// refused body leaves a valid proposal undone.
#[tokio::test]
async fn generated_bodies_follow_the_no_body_rule() {
    let (dir, s) = setup("nobody-gen");
    let pieces: [&[u8]; 14] = [
        b"{", b"}", b" ", b"\n", b"\t", b"\r", b"\x0c", b"\"", b"a", b":", b"1", b"[", b"]",
        b"null",
    ];
    let mut seed: u64 = 17;
    let mut next = |n: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    let (mut passed, mut refused) = (0, 0);
    for i in 0..600 {
        let len = next(7);
        let mut body: Vec<u8> = (0..len)
            .flat_map(|_| pieces[next(pieces.len() as u64) as usize].to_vec())
            .collect();
        // Each fourth body gets white space up to near the 64-byte limit.
        if i % 4 == 0 {
            let pad = 58 + next(10) as usize;
            body.splice(
                body.len() / 2..body.len() / 2,
                std::iter::repeat_n(b' ', pad.saturating_sub(body.len())),
            );
        }
        let text = String::from_utf8(body.clone()).unwrap();
        let want = no_body(&body);
        // g-9 does not exist: a body that passes reaches the route and gets 404.
        let (code, _) = call(&s, Method::POST, "/api/matters/g-9/decline", &text).await;
        let expect = if want {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        };
        assert_eq!(code, expect, "{text:?}");
        if !want {
            let (code, _) = call(&s, Method::POST, "/api/matters/g-1/decline", &text).await;
            assert_eq!(code, StatusCode::BAD_REQUEST, "{text:?}");
            refused += 1;
        } else {
            passed += 1;
        }
    }
    assert!(passed > 20 && refused > 20, "{passed} {refused}");
    let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
    assert_eq!(ids(&list["proposed"]), vec!["g-2", "g-1"]);
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

/// A route that takes no body stops the read of a large body after a few
/// bytes (issue 361): an endless body gives 400, and the route reads only
/// the first chunks of it.
#[tokio::test]
async fn a_route_with_no_body_stops_the_read_early() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (dir, s) = setup("nobody-early");
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    for uri in [
        "/api/matters/g-1/decline",
        "/api/senders/p-999/accept",
        "/api/changesets/x-999/withdraw",
        "/api/policy/discard",
        "/api/notices/n-999/done",
        "/api/watches/w-999/remove",
    ] {
        let read = Arc::new(AtomicUsize::new(0));
        let r = read.clone();
        let chunks = futures_util::stream::repeat_with(move || {
            r.fetch_add(1024, Ordering::SeqCst);
            Ok::<_, std::io::Error>(axum::body::Bytes::from(vec![b' '; 1024]))
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("host", "127.0.0.1:4800")
            .header("authorization", auth.bearer())
            .header("content-type", "application/json")
            .body(Body::from_stream(chunks))
            .unwrap();
        let res = iris_api::router(s.clone(), auth.clone())
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{uri}");
        let n = read.load(Ordering::SeqCst);
        assert!(n <= 4 * 1024, "{uri} read {n} bytes");
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// The 64-byte limit of a route with no body: `{}` in 64 bytes passes and
/// changes the matter. In 65 bytes it gets 400 and changes nothing.
#[tokio::test]
async fn the_no_body_limit_is_64_bytes() {
    let (dir, s) = setup("nobody-64");
    let body = |n: usize| format!("{{{}}}", " ".repeat(n - 2));
    let (code, _) = call(&s, Method::POST, "/api/matters/g-1/decline", &body(65)).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
    assert_eq!(ids(&list["proposed"]), vec!["g-2", "g-1"]);
    let (code, _) = call(&s, Method::POST, "/api/matters/g-1/decline", &body(64)).await;
    assert_eq!(code, StatusCode::OK);
    let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
    assert_eq!(ids(&list["proposed"]), vec!["g-2"]);
    let _ = std::fs::remove_dir_all(dir);
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

/// Issue 112: a member whose origin is hostile mail is in no matter read,
/// also when a confirm pins it, and a proposal does not count it. The
/// member shows again when the state clears.
#[tokio::test]
async fn a_member_of_hostile_mail_is_hidden_until_the_state_clears() {
    let (dir, s) = setup("hostile");
    s.put_labels(1, &[("SPAM", "SPAM", "system")]).unwrap();
    let (code, _) = call(
        &s,
        Method::POST,
        "/api/matters/g-1/confirm",
        r#"{"title":"Tokyo","items":["r-1","v-2","c-3"]}"#,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    // Items 1 and 4 come from message 1.
    for (set, clear) in HOSTILE {
        s.write(|tx| Ok(tx.execute_batch(set)?)).unwrap();
        let (_, v) = call(&s, Method::GET, "/api/matters/g-1", "").await;
        agree(&v);
        assert_eq!(v["matter"]["items"], json!(["v-2", "c-3"]), "{set}");
        let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
        assert_eq!(ids(&list["proposed"]), Vec::<&str>::new(), "{set}");
        assert_eq!(list["confirmed"][0]["counts"]["record"], 0, "{set}");
        // g-2 keeps one live member, so it is not valid.
        let (code, _) = call(&s, Method::GET, "/api/matters/g-2", "").await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{set}");
        let (code, _) = call(&s, Method::POST, "/api/matters/g-2/decline", "").await;
        assert_eq!(code, StatusCode::CONFLICT, "{set}");
        s.write(|tx| Ok(tx.execute_batch(clear)?)).unwrap();
        let (_, v) = call(&s, Method::GET, "/api/matters/g-1", "").await;
        assert_eq!(
            v["matter"]["items"],
            json!(["r-1", "v-2", "c-3"]),
            "{clear}"
        );
        let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
        assert_eq!(ids(&list["proposed"]), vec!["g-2"], "{clear}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Issue 112: a matter with too few members that are not hostile mail (one
/// for a confirmed matter, two for a declined matter) is not read, so each
/// matter document that a route gives passes its schema. It shows again
/// when the state clears.
#[tokio::test]
async fn a_matter_with_too_few_members_to_show_is_not_read() {
    let (dir, s) = setup("hostile-few");
    s.put_labels(1, &[("SPAM", "SPAM", "system")]).unwrap();
    let schema = iris_spec::validator("matter");
    // g-1 keeps only r-1 (message 1). g-2 is declined: r-4 (message 1) and c-5.
    let (code, _) = call(
        &s,
        Method::POST,
        "/api/matters/g-1/confirm",
        r#"{"title":"Tokyo","items":["r-1"]}"#,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, _) = call(&s, Method::POST, "/api/matters/g-2/decline", "").await;
    assert_eq!(code, StatusCode::OK);
    for (set, clear) in HOSTILE {
        s.write(|tx| Ok(tx.execute_batch(set)?)).unwrap();
        for g in ["g-1", "g-2"] {
            let (code, v) = call(&s, Method::GET, &format!("/api/matters/{g}"), "").await;
            assert_eq!(code, StatusCode::NOT_FOUND, "{set}: {g} {v}");
        }
        let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
        assert_eq!(ids(&list["confirmed"]), Vec::<&str>::new(), "{set}");
        s.write(|tx| Ok(tx.execute_batch(clear)?)).unwrap();
        for g in ["g-1", "g-2"] {
            let (code, v) = call(&s, Method::GET, &format!("/api/matters/{g}"), "").await;
            assert_eq!(code, StatusCode::OK, "{clear}: {g}");
            assert!(schema.is_valid(&v["matter"]), "{}", v["matter"]);
        }
        let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
        assert_eq!(ids(&list["confirmed"]), vec!["g-1"], "{clear}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Decisions D19 and D20 (`docs/specs/2026-11-24-matters-edit.md`): a
/// matter by hand, an edit, an addition, and the delete. Each document
/// passes `matter.schema.json`.
#[tokio::test]
async fn the_person_makes_edits_and_deletes_a_matter() {
    let (dir, s) = setup("edit");
    let schema = iris_spec::validator("matter");
    // Decline g-2 so items 4 and 5 are free.
    let (code, _) = call(&s, Method::POST, "/api/matters/g-2/decline", "").await;
    assert_eq!(code, StatusCode::OK);

    let (code, v) = call(
        &s,
        Method::POST,
        "/api/matters",
        r#"{"title":"Lease","items":["r-4"]}"#,
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    assert!(schema.is_valid(&v["matter"]), "{v}");
    assert_eq!(v["matter"]["derived"], json!({"by": "person"}));
    assert_eq!(v["matter"]["status"], "confirmed");
    let id = v["matter"]["id"].as_str().unwrap().to_string();
    let edit = format!("/api/matters/{id}/edit");

    // An item of an open proposal, a feed ID, a bad body.
    for (body, want) in [
        (r#"{"add":["r-1"]}"#, StatusCode::CONFLICT),
        (r#"{"add":["f-1"]}"#, StatusCode::BAD_REQUEST),
        (r#"{"remove":["r-4"]}"#, StatusCode::CONFLICT),
        (r#"{}"#, StatusCode::BAD_REQUEST),
        (r#"{"title":"x","other":1}"#, StatusCode::BAD_REQUEST),
    ] {
        let (code, v) = call(&s, Method::POST, &edit, body).await;
        assert_eq!(code, want, "{body}: {v}");
    }
    let (code, v) = call(&s, Method::POST, &edit, r#"{"title":"Flat","add":["c-5"]}"#).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert!(schema.is_valid(&v["matter"]), "{v}");
    agree(&v);
    assert_eq!(v["matter"]["title"], "Flat");
    let (_, item) = call(&s, Method::GET, "/api/items/c-5", "").await;
    assert_eq!(item["matter"], id.as_str(), "{item}");

    // An addition shows on the list and in the view, and Not this ends it.
    let row: i64 = id[2..].parse().unwrap();
    s.write(|tx| {
        tx.execute(
            "INSERT INTO matter_add(item_id, matter_id, created_at) VALUES (1, ?1, 5)",
            [row],
        )
        .map(|_| ())
        .map_err(Into::into)
    })
    .unwrap_err(); // item 1 is a kept member of the proposal g-1
    let (code, _) = call(&s, Method::POST, "/api/matters/g-1/decline", "").await;
    assert_eq!(code, StatusCode::OK);
    s.write(|tx| {
        tx.execute(
            "INSERT INTO matter_add(item_id, matter_id, created_at) VALUES (1, ?1, 5)",
            [row],
        )?;
        Ok(())
    })
    .unwrap();
    let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
    let doc = &list["confirmed"][0];
    assert_eq!(doc["additions"], json!(["r-1"]), "{list}");
    assert!(schema.is_valid(doc), "{doc}");
    let (_, v) = call(&s, Method::GET, &format!("/api/matters/{id}"), "").await;
    assert_eq!(v["additions"][0]["id"], "r-1", "{v}");
    let (code, v) = call(&s, Method::POST, &edit, r#"{"remove":["r-1"]}"#).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert!(v["matter"].get("additions").is_none(), "{v}");

    // The delete: 204, then the matter is not in the list.
    let (code, _) = call(&s, Method::POST, &format!("/api/matters/{id}/delete"), "").await;
    assert_eq!(code, StatusCode::NO_CONTENT);
    let (_, list) = call(&s, Method::GET, "/api/matters", "").await;
    assert_eq!(list["confirmed"], json!([]));
    let (code, _) = call(&s, Method::POST, &format!("/api/matters/{id}/delete"), "").await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, _) = call(&s, Method::POST, "/api/matters/g-99/delete", "").await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = call(
        &s,
        Method::POST,
        &format!("/api/matters/{id}/delete"),
        "{\"x\":1}",
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let _ = std::fs::remove_dir_all(dir);
}
