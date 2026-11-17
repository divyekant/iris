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
        json!({"intent": "say yes"}),
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
    let (code, _) = post(&s, fake(), "/api/threads/t-1/draft", json!({})).await;
    assert_eq!(code, StatusCode::OK);
}

/// Each refusal saves no draft and names what to do.
#[tokio::test]
async fn a_refused_draft_saves_nothing() {
    // No model client, and no budget.
    let s = store("refused", None);
    let (code, _) = post(&s, None, "/api/threads/t-1/draft", json!({"intent": "yes"})).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    let (code, v) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": "yes"}),
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("budget"), "{v}");
    // A thread that does not exist, and an ID that is not a thread.
    let s = store("refused-2", Some(1_000_000));
    for uri in ["/api/threads/t-9/draft", "/api/threads/m-1/draft"] {
        let (code, _) = post(&s, fake(), uri, json!({"intent": "yes"})).await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{uri}");
    }
    // A long intent, and a field that the route does not know.
    let long = "x".repeat(iris_voice::MAX_INTENT + 1);
    let (code, _) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": long}),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (code, _) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": "yes", "to": "x"}),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    // A mail that understanding did not read, then a hostile mail.
    s.write(|tx| Ok(tx.execute("UPDATE message SET understood_at = NULL", [])?))
        .unwrap();
    let (code, v) = post(
        &s,
        fake(),
        "/api/threads/t-1/draft",
        json!({"intent": "yes"}),
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
        json!({"intent": "yes"}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("hostile"), "{v}");
    assert!(drafts(&s).is_empty());
}
