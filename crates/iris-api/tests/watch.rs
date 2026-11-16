//! The routes of watches (card R5-07): `GET /api/watches`,
//! `POST /api/watches` and `POST /api/watches/{id}/remove`.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Method, Request, StatusCode};
use iris_api::Auth;
use iris_store::{NewMessage, Store};
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

#[tokio::test]
async fn add_list_and_remove_a_watch() {
    let dir = std::env::temp_dir().join(format!("iris-api-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Arc::new(Store::open(&dir).unwrap());
    let a = s.add_account("gmail", "owner@example.com").unwrap();

    let (code, w) = call(&s, Method::POST, "/api/watches", r#"{"text":"visa"}"#).await;
    assert_eq!(code, StatusCode::CREATED);
    assert_eq!(
        (w["text"].as_str(), w["state"].as_str()),
        (Some("visa"), Some("waits"))
    );
    let id = w["id"].as_str().unwrap().to_string();
    for bad in [r#"{"text":"  "}"#, r#"{"text":"form:email"}"#, "{}"] {
        let (code, _) = call(&s, Method::POST, "/api/watches", bad).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{bad}");
    }

    // A date after the watch: the route used the time of now.
    let now = 4_102_444_800_000; // 2100-01-01
    let m = NewMessage {
        provider_id: "g1".into(),
        thread_provider_id: "gt1".into(),
        internal_date: Some(now),
        body: Some(iris_store::Body {
            subject: "Visa".into(),
            sender: "embassy@example.com".into(),
            text: "Ready.".into(),
        }),
        ..Default::default()
    };
    s.put_messages(a, &[m]).unwrap();
    assert_eq!(s.check_watches(now).unwrap().len(), 1);
    let (code, list) = call(&s, Method::GET, "/api/watches", "").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(list[0]["state"], "arrived");
    assert_eq!(list[0]["message"]["id"], json!("m-1"));

    let uri = format!("/api/watches/{id}/remove");
    assert_eq!(call(&s, Method::POST, &uri, "").await.0, StatusCode::OK);
    assert_eq!(
        call(&s, Method::POST, &uri, "").await.0,
        StatusCode::NOT_FOUND
    );
    let (code, _) = call(&s, Method::POST, "/api/watches/m-1/remove", "").await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert_eq!(call(&s, Method::GET, "/api/watches", "").await.1, json!([]));
    let _ = std::fs::remove_dir_all(&dir);
}
