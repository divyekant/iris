//! Get the pages of the web client from the router, with no network.

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use tower::ServiceExt;

async fn get(path: &str) -> (StatusCode, String, String) {
    let res = iris_web::router()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let kind = res
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, kind, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn root_shows_the_iris_page() {
    let (status, kind, body) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(kind, "text/html; charset=utf-8");
    assert!(body.contains("<title>Iris</title>"), "{body}");
}

#[tokio::test]
async fn every_embedded_file_is_served_with_its_type() {
    for path in iris_web::paths() {
        let (status, kind, _) = get(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(!kind.is_empty(), "{path}");
    }
}

#[tokio::test]
async fn an_unknown_path_is_not_found() {
    let (status, _, _) = get("/no-such-file.js").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
