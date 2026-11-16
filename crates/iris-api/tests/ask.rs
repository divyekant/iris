//! `POST /api/ask` (card R5-09) with a fake model. No key, no network.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::{Auth, Connect};
use iris_model::{Reply, Stop, Usage};
use iris_store::{Body as Text, Header, NewMessage, Store};
use iris_understand::Model;
use serde_json::{json, Value};
use tower::ServiceExt;

/// The answer, and a change of the store during the call.
struct Fake(String, Option<Arc<dyn Fn() + Send + Sync>>);

impl Model for Fake {
    fn name(&self) -> &str {
        "fake-1"
    }

    fn request_bytes(&self, _: &str, _: &str) -> u64 {
        1000
    }

    fn ask(&self, _: &str, _: &str) -> Result<Reply, iris_model::Error> {
        if let Some(f) = &self.1 {
            f();
        }
        Ok(Reply {
            stop: Stop::End,
            text: self.0.clone(),
            usage: Usage {
                input_tokens: 100,
                output_tokens: 10,
                cost_micro_usd: None,
                known: true,
            },
        })
    }
}

/// A store with one booking mail (`m-1`) and a priced model with `budget`.
fn store(name: &str, budget: Option<i64>) -> Arc<Store> {
    let dir = std::env::temp_dir().join(format!("iris-api-ask-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Store::open(&dir).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    let m = NewMessage {
        provider_id: "g1".into(),
        thread_provider_id: "gt1".into(),
        internal_date: Some(1_794_000_000_000),
        header: Header {
            date: Some(1_794_000_000_000),
            from_addr: Some("noreply@airline.example".into()),
            from_name: Some("Example Air".into()),
            subject: Some("Your booking confirmation".into()),
            ..Default::default()
        },
        body: Some(Text {
            subject: "Your booking confirmation".into(),
            sender: "noreply@airline.example".into(),
            text: "Departure 07:40, flight TP 1351 to Lisbon.".into(),
        }),
        ..Default::default()
    };
    s.put_messages(a, &[m]).unwrap();
    s.write(|tx| {
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

fn fake(answer: &str) -> Option<Connect> {
    fake_with(answer, None)
}

fn fake_with(answer: &str, during: Option<Arc<dyn Fn() + Send + Sync>>) -> Option<Connect> {
    let answer = answer.to_string();
    Some(Arc::new(move |_| {
        Ok(Box::new(Fake(answer.clone(), during.clone())) as Box<dyn Model>)
    }))
}

/// `connect: None` is the router with no model client.
async fn ask(
    store: &Arc<Store>,
    connect: Option<Connect>,
    body: &str,
    signed_in: bool,
) -> (StatusCode, Value) {
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let mut req = Request::post("/api/ask")
        .header("host", "127.0.0.1:4800")
        .header("content-type", "application/json");
    if signed_in {
        req = req.header("authorization", auth.bearer());
    }
    let router = match connect {
        Some(c) => iris_api::router_with_model(store.clone(), auth, c),
        None => iris_api::router(store.clone(), auth),
    };
    let res = router
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let code = res.status();
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn answers_with_the_summary_of_each_source() {
    let s = store("answer", Some(1_000_000));
    let reply = r#"{"claims": [{"text": "It leaves at 07:40.", "sources": [1]}],
                    "quotes": [{"n": 1, "quote": "Departure 07:40"}]}"#;
    let (code, v) = ask(
        &s,
        fake(reply),
        r#"{"question": "When does my flight leave?"}"#,
        true,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(
        v,
        json!({
            "answer": "It leaves at 07:40 [1].",
            "sources": [{
                "n": 1,
                "quote": "Departure 07:40",
                "message": {
                    "id": "m-1",
                    "thread": "t-1",
                    "date": "2026-11-06T21:20:00Z",
                    "from": {"name": "Example Air", "address": "noreply@airline.example"},
                    "subject": "Your booking confirmation",
                    "labels": []
                }
            }],
            "read": 1,
            "cost_micro_usd": 110
        })
    );
    // No match: no call and no answer.
    let (code, v) = ask(&s, fake(reply), r#"{"question": "weather in Oslo"}"#, true).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        v,
        json!({"answer": null, "sources": [], "read": 0, "cost_micro_usd": null})
    );
}

#[tokio::test]
async fn refuses_a_bad_request_no_model_and_no_budget() {
    let s = store("refuse", None);
    let ok = fake(r#"{"claims": [], "quotes": []}"#);
    for body in [
        r#"{"question": ""}"#,
        r#"{"q": "flight"}"#,
        r#"{"question": "flight", "x": 1}"#,
        "flight",
    ] {
        let (code, _) = ask(&s, ok.clone(), body, true).await;
        assert!(
            code.is_client_error() && code != StatusCode::UNAUTHORIZED,
            "{body}: {code}"
        );
    }
    // The guard covers the route.
    let (code, _) = ask(&s, ok.clone(), r#"{"question": "flight"}"#, false).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    // No budget is approved.
    let (code, v) = ask(&s, ok, r#"{"question": "flight"}"#, true).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert!(v["error"].as_str().unwrap().contains("budget"), "{v}");
    // The router with no model client.
    let s = store("noclient", Some(1_000_000));
    let (code, v) = ask(&s, None, r#"{"question": "flight"}"#, true).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    // A model answer that is not JSON.
    let (code, _) = ask(&s, fake("no"), r#"{"question": "flight"}"#, true).await;
    assert_eq!(code, StatusCode::BAD_GATEWAY);
}

/// The mail can change during the call: a source that Ask cannot read now
/// goes, with its claims.
#[tokio::test]
async fn a_source_that_goes_during_the_call_takes_its_claims() {
    let s = store("gone", Some(1_000_000));
    let s2 = s.clone();
    let gone: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        s2.write(|tx| {
            Ok(tx
                .execute("UPDATE message SET gone_at = 1", [])
                .map(|_| ())?)
        })
        .unwrap();
    });
    let reply = r#"{"claims": [{"text": "It leaves at 07:40.", "sources": [1]}]}"#;
    let (code, v) = ask(
        &s,
        fake_with(reply, Some(gone)),
        r#"{"question": "When does my flight leave?"}"#,
        true,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(
        v,
        json!({"answer": null, "sources": [], "read": 1, "cost_micro_usd": 110})
    );
}
