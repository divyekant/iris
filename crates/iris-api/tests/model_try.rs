//! "Try now" after a model error (decision D35, issue #136): the status
//! gives the end of the wait, and `POST /api/model/try` ends it. The count
//! of errors stays, so the backoff stays.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_store::Store;
use serde_json::Value;
use tower::ServiceExt;

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
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[tokio::test]
async fn try_now_ends_the_wait_and_keeps_the_count() {
    let dir = std::env::temp_dir().join(format!("iris-api-try-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Arc::new(Store::open(&dir).unwrap());
    // No model: no wait, and a try changes nothing.
    let (code, v) = call(&store, true, "/api/model/try").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v["model_retry_at"], Value::Null);
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO model_setting(id, provider, base_url, model, version, max_tokens,
                   timeout_s, retry_failures, retry_at)
                 VALUES (1, 'local', 'http://127.0.0.1:1/v1', 'm', 1, 100, 5, 3, ?1)",
                [now() + 3_600_000],
            )?;
            Ok(())
        })
        .unwrap();
    let (_, v) = call(&store, false, "/api/status").await;
    assert!(v["model_retry_at"].is_string(), "{v}");

    let (code, v) = call(&store, true, "/api/model/try").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v["model_retry_at"], Value::Null);
    let (failures, at): (i64, i64) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT retry_failures, retry_at FROM model_setting",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    // The wait ends now: the next run takes it as an ended wait.
    assert_eq!(failures, 3);
    assert!(at <= now());
    // A wait that ended stays as it is.
    let (_, v) = call(&store, true, "/api/model/try").await;
    assert_eq!(v["model_retry_at"], Value::Null);
    let after: i64 = store
        .read(|c| Ok(c.query_row("SELECT retry_at FROM model_setting", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(after, at);
    // GET is not a try.
    let (code, _) = call(&store, false, "/api/model/try").await;
    assert_eq!(code, StatusCode::METHOD_NOT_ALLOWED);
    let _ = std::fs::remove_dir_all(&dir);
}
