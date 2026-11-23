//! `POST /api/threads/{t}/draft` (R3-04 intent path): a draft of the reply
//! in the voice of the person, with a fake model. No key, no network.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::{Auth, Connect};
use iris_model::{Reply, Stop, Usage};
use iris_store::{Body as Text, Header, NewMessage, Store};
use iris_understand::Model;
use serde_json::{json, Value};
use tower::ServiceExt;

struct Fake;

impl Model for Fake {
    fn name(&self) -> &str {
        "fake-1"
    }

    fn request_bytes(&self, _: &str, _: &str) -> u64 {
        1000
    }

    fn ask(&self, _: &str, _: &str) -> Result<Reply, iris_model::Error> {
        Ok(Reply {
            stop: Stop::End,
            text: r#"{"body": "Hi Ana,\n\nYes, Thursday works.\n"}"#.into(),
            usage: Usage {
                input_tokens: 100,
                output_tokens: 10,
                cost_micro_usd: None,
                known: true,
            },
        })
    }
}

/// Thread t-1 with one mail from ana (`m-1`), read by understanding, and a
/// priced model with `budget`.
fn store(name: &str, budget: Option<i64>) -> Arc<Store> {
    let dir = std::env::temp_dir().join(format!("iris-api-voice-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Store::open(&dir).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    let m = NewMessage {
        provider_id: "g1".into(),
        thread_provider_id: "gt1".into(),
        internal_date: Some(1_794_000_000_000),
        header: Header {
            from_addr: Some("ana@example.org".into()),
            from_name: Some("Ana Lee".into()),
            to_addrs: Some("owner@example.com".into()),
            subject: Some("Lunch".into()),
            ..Default::default()
        },
        body: Some(Text {
            subject: "Lunch".into(),
            sender: "ana@example.org".into(),
            text: "Lunch on Thursday?".into(),
        }),
        ..Default::default()
    };
    s.put_messages(a, &[m]).unwrap();
    s.write(|tx| {
        tx.execute("UPDATE message SET understood_at = 1", [])?;
        tx.execute(
            "INSERT INTO model_setting(id, provider, base_url, model, version, price_in,
               price_out, max_tokens, timeout_s, budget_micro_usd)
             VALUES (1, 'anthropic', 'https://api.anthropic.com', 'fake-1', 1, 1000000,
                     1000000, 100, 30, ?1)",
            [budget],
        )?;
        Ok(())
    })
    .unwrap();
    Arc::new(s)
}

fn fake() -> Option<Connect> {
    Some(Arc::new(|_| Ok(Box::new(Fake) as Box<dyn Model>)))
}

async fn post(
    store: &Arc<Store>,
    connect: Option<Connect>,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let req = Request::post(uri)
        .header("host", "127.0.0.1:4800")
        .header("content-type", "application/json")
        .header("authorization", auth.bearer())
        .body(Body::from(body.to_string()))
        .unwrap();
    let router = match connect {
        Some(c) => iris_api::router_with_model(store.clone(), auth, c),
        None => iris_api::router(store.clone(), auth),
    };
    let res = router.oneshot(req).await.unwrap();
    let code = res.status();
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn drafts(s: &Store) -> Vec<(String, Option<String>)> {
    s.read(|c| {
        let mut st = c.prepare("SELECT body, intent FROM reply_draft ORDER BY thread_id")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })
    .unwrap()
}

/// The draft comes back for the form and stays in the store, with the
/// intent. The call goes through the budget: its cost is taken.
#[tokio::test]
async fn an_intent_gives_a_draft_in_your_voice() {
    let s = store("ok", Some(1_000_000));
    let (code, v) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": "say yes", "parent": "m-1"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["body"], "Hi Ana,\n\nYes, Thursday works.");
    assert_eq!(v["based_on"], "m-1");
    assert_eq!(v["unknown_cost"], false);
    assert_eq!(
        drafts(&s),
        [(
            "Hi Ana,\n\nYes, Thursday works.".into(),
            Some("say yes".into())
        )]
    );
    let left: i64 = s
        .read(|c| {
            Ok(
                c.query_row("SELECT budget_micro_usd FROM model_setting", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert!(left < 1_000_000, "{left}");
    // An empty intent: the draft comes from the thread alone.
    let (code, _) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"parent": "m-1"}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    // A body with no parent.
    let (code, _) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": "yes"}),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
}

/// Each refusal saves no draft and names what to do.
#[tokio::test]
async fn a_refused_draft_saves_nothing() {
    // No model client, and no budget.
    let s = store("refused", None);
    let (code, _) = post(
        &s,
        None,
        "/api/threads/t-1/draft",
        json!({"intent": "yes", "parent": "m-1"}),
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    let (code, v) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": "yes", "parent": "m-1"}),
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("budget"), "{v}");
    // A thread that does not exist, and an ID that is not a thread.
    let s = store("refused-2", Some(1_000_000));
    for uri in ["/api/threads/t-9/draft", "/api/threads/m-1/draft"] {
        let (code, _) = post(&s, fake(), uri, json!({"intent": "yes", "parent": "m-1"})).await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{uri}");
    }
    // A long intent, and a field that the route does not know.
    let long = "x".repeat(iris_voice::MAX_INTENT + 1);
    let (code, _) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": long, "parent": "m-1"}),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (code, _) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": "yes", "parent": "m-1", "cc": "x"}),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    // A recipient of the form that fails the address check (issue 535).
    let (code, v) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": "yes", "parent": "m-1", "to": ["not an address"]}),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{v}");
    assert!(drafts(&s).is_empty());
    // A valid recipient of the form is the person of the reply.
    let (code, v) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": "yes", "parent": "m-1", "to": ["bo@example.org"]}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    s.write(|tx| Ok(tx.execute("DELETE FROM reply_draft", [])?))
        .unwrap();
    // A mail that understanding did not read, then a hostile mail.
    s.write(|tx| Ok(tx.execute("UPDATE message SET understood_at = NULL", [])?))
        .unwrap();
    let (code, v) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": "yes", "parent": "m-1"}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    s.write(|tx| {
        Ok(tx.execute(
            r#"UPDATE message SET understood_at = 1, hostile = '["link_mismatch"]'"#,
            [],
        )?)
    })
    .unwrap();
    let (code, v) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": "yes", "parent": "m-1"}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("hostile"), "{v}");
    assert!(drafts(&s).is_empty());
}

/// For random threads of mail with the labels `TRASH`, `DRAFT` and `SENT`
/// and random dates, a draft answers the parent of the reply form: its
/// `based_on` is the `parent` of `GET /api/threads/{t}/reply`. A thread with
/// no parent gets no draft.
#[tokio::test]
async fn property_a_draft_answers_the_parent_of_the_form() {
    let mut x: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    for case in 0..60 {
        let s = store(&format!("prop-{case}"), Some(100_000_000));
        let n = 1 + next(5);
        s.write(|tx| {
            tx.execute_batch(
                "INSERT INTO label(account_id, provider_id, name, kind) VALUES
                   (1, 'TRASH', 'TRASH', 'system'), (1, 'DRAFT', 'DRAFT', 'system'),
                   (1, 'SENT', 'SENT', 'system');",
            )?;
            for i in 0..n {
                let id: i64 = {
                    tx.execute(
                        "INSERT INTO message(account_id, thread_id, provider_id, internal_date, understood_at)
                           VALUES (1, 1, ?1, ?2, 1)",
                        rusqlite::params![format!("p{case}-{i}"), 1_794_000_000_000 + next(4) as i64 * 1000],
                    )?;
                    tx.last_insert_rowid()
                };
                tx.execute(
                    "INSERT INTO header(message_id, from_addr, to_addrs, subject)
                       VALUES (?1, 'ana@example.org', 'owner@example.com', 'Lunch')",
                    [id],
                )?;
                tx.execute(
                    "INSERT INTO body(message_id, subject, sender, text) VALUES (?1, 'Lunch', 'ana', 'Lunch?')",
                    [id],
                )?;
                let label = ["", "TRASH", "DRAFT", "SENT"][next(4) as usize];
                if !label.is_empty() {
                    tx.execute(
                        "INSERT INTO message_label(message_id, label_id)
                           SELECT ?1, id FROM label WHERE provider_id = ?2",
                        rusqlite::params![id, label],
                    )?;
                    // SENT and DRAFT clear the understanding (migration 37):
                    // the message is understood after its labels.
                    tx.execute("UPDATE message SET understood_at = 1 WHERE id = ?1", [id])?;
                }
            }
            Ok(())
        })
        .unwrap();
        let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
        let req = Request::get("/api/threads/t-1/reply")
            .header("host", "127.0.0.1:4800")
            .header("authorization", auth.bearer())
            .body(Body::empty())
            .unwrap();
        let res = iris_api::router(s.clone(), auth)
            .oneshot(req)
            .await
            .unwrap();
        let start_code = res.status();
        let start: Value =
            serde_json::from_slice(&to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
        let parent = start["parent"]["id"].as_str().unwrap_or("m-1").to_string();
        let (code, v) = post(
            &s,
            fake(),
            "/api/threads/t-1/draft",
            json!({"intent": "yes", "parent": parent}),
        )
        .await;
        if start_code == StatusCode::OK {
            assert_eq!(code, StatusCode::OK, "case {case}: {v}");
            assert_eq!(v["based_on"], start["parent"]["id"], "case {case}");
        } else {
            assert_eq!(start_code, StatusCode::CONFLICT, "case {case}");
            assert_ne!(code, StatusCode::OK, "case {case}: {v}");
        }
    }
}

/// A mail that comes into the thread after the form opened: the draft for
/// the old parent is a 409 before the call, so it costs nothing and saves
/// nothing.
#[tokio::test]
async fn a_newer_mail_stops_the_draft_before_the_call() {
    let s = store("newer", Some(1_000_000));
    s.write(|tx| {
        tx.execute(
            "INSERT INTO message(account_id, thread_id, provider_id, internal_date, understood_at)
               VALUES (1, 1, 'g2', 1794000009000, 1)",
            [],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO header(message_id, from_addr, to_addrs, subject)
               VALUES (?1, 'ana@example.org', 'owner@example.com', 'Lunch')",
            [id],
        )?;
        tx.execute(
            "INSERT INTO body(message_id, subject, sender, text)
               VALUES (?1, 'Lunch', 'ana', 'Or Friday?')",
            [id],
        )?;
        Ok(())
    })
    .unwrap();
    let (code, v) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": "yes", "parent": "m-1"}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    assert!(drafts(&s).is_empty());
    let runs: i64 = s
        .read(|c| Ok(c.query_row("SELECT count(*) FROM understand_run", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(runs, 0, "no call");
}
