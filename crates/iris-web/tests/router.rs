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

/// If the client build exists, the program must serve it, not the fallback.
/// Cargo runs `build.rs` again when `client/dist` appears, so the embedded
/// files always match the folder at test time.
#[tokio::test]
async fn the_client_build_is_embedded_when_it_exists() {
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../client/dist");
    let Ok(index) = std::fs::read_to_string(dist.join("index.html")) else {
        return; // no client build: the fallback page is correct
    };
    let paths: Vec<_> = iris_web::paths().collect();
    assert!(paths.contains(&"/index.html"), "{paths:?}");
    assert!(paths.iter().any(|p| p.ends_with(".js")), "{paths:?}");
    assert_eq!(get("/").await.2, index);
    for path in paths {
        let bytes = std::fs::read(dist.join(&path[1..])).unwrap();
        assert_eq!(get(path).await.2, String::from_utf8_lossy(&bytes), "{path}");
    }
}

#[tokio::test]
async fn only_get_and_head_are_allowed() {
    for method in ["POST", "PUT", "DELETE"] {
        let res = iris_web::router()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED, "{method}");
    }
    let res = iris_web::router()
        .oneshot(Request::head("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
