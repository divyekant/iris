//! The stop of the web client (docs/specs/2026-11-17-stop.md, section 5):
//! GET and POST /api/stop, POST /api/resume.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_store::Store;
use serde_json::{json, Value};
use tower::ServiceExt;

struct TempDir(std::path::PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("iris-api-stop-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    TempDir(dir)
}

async fn call(store: &Arc<Store>, method: &str, uri: &str, body: &str) -> (StatusCode, Value) {
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

fn stopped(s: &Store) -> Option<i64> {
    s.read(iris_store::stopped).unwrap()
}

/// Stop sets the switch and answers a complete stop; resume clears it.
#[tokio::test]
async fn stop_and_resume() {
    let dir = temp("cycle");
    let s = Arc::new(Store::open(&dir.0).unwrap());
    let (code, v) = call(&s, "GET", "/api/stop", "").await;
    assert_eq!(
        (code, v),
        (
            StatusCode::OK,
            json!({"stopped": false, "since": null, "live": 0, "busy": false})
        )
    );

    let (code, v) = call(&s, "POST", "/api/stop", "{}").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        (&v["stopped"], &v["sessions"], &v["live"], &v["busy"]),
        (&json!(true), &json!(0), &json!(0), &json!(false))
    );
    assert!(stopped(&s).is_some());
    let (_, got) = call(&s, "GET", "/api/stop", "").await;
    assert_eq!(
        (&got["stopped"], &got["since"]),
        (&json!(true), &v["since"])
    );

    let (code, v) = call(&s, "POST", "/api/resume", "").await;
    assert_eq!((code, &v["resumed"]), (StatusCode::OK, &json!(true)));
    assert!(stopped(&s).is_none());
    let (_, v) = call(&s, "POST", "/api/resume", "").await;
    assert_eq!(v["resumed"], json!(false));
}

/// A body or a wrong method changes nothing.
#[tokio::test]
async fn other_input_changes_nothing() {
    let dir = temp("input");
    let s = Arc::new(Store::open(&dir.0).unwrap());
    let (code, _) = call(&s, "POST", "/api/stop", "{\"all\": true}").await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (code, _) = call(&s, "PUT", "/api/stop", "").await;
    assert_eq!(code, StatusCode::METHOD_NOT_ALLOWED);
    assert!(stopped(&s).is_none());
    iris_apply::stop(&s).unwrap();
    let (code, _) = call(&s, "POST", "/api/resume", "[]").await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(stopped(&s).is_some());
}

/// GET /api/stop measures the stop now: a held apply lock is `busy`, and a
/// session that is not reaped is `live`.
#[tokio::test]
async fn the_state_says_when_the_stop_is_not_complete() {
    let dir = temp("state");
    let s = Arc::new(Store::open(&dir.0).unwrap());
    let job = iris_job::create(&s, "Tidy the inbox.", None, 1).unwrap();
    iris_job::register(&s, job, "claude-code", 1).unwrap();
    iris_apply::stop_wait(&s, std::time::Duration::ZERO).unwrap();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(s.dir().join(iris_apply::LOCK_FILE))
        .unwrap();
    file.try_lock().unwrap();
    let (_, v) = call(&s, "GET", "/api/stop", "").await;
    assert_eq!((&v["live"], &v["busy"]), (&json!(1), &json!(true)));
    drop(file);
    let (_, v) = call(&s, "GET", "/api/stop", "").await;
    assert_eq!(v["busy"], json!(false));
}
