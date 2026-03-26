use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::sync::Arc;
use tower::ServiceExt;

use iris_server::ai::memories::MemoriesClient;
use iris_server::ai::provider::ProviderPool;
use iris_server::config::Config;
use iris_server::db::migrations;
use iris_server::ws::hub::WsHub;
use iris_server::{build_app, AppState};

const TEST_TOKEN: &str = "test-session-token-abc123";

fn create_test_state() -> Arc<AppState> {
    let manager = SqliteConnectionManager::memory().with_init(|conn| {
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )
    });
    let pool = Pool::builder().max_size(1).build(manager).unwrap();
    {
        let conn = pool.get().unwrap();
        migrations::run(&conn).unwrap();
    }

    Arc::new(AppState {
        db: pool,
        config: Config {
            port: 3000,
            database_url: ":memory:".to_string(),
            ollama_url: "http://localhost:11434".to_string(),
            memories_url: "http://localhost:8900".to_string(),
            memories_api_key: None,
            anthropic_api_key: None,
            openai_api_key: None,
            gmail_client_id: None,
            gmail_client_secret: None,
            outlook_client_id: None,
            outlook_client_secret: None,
            app_password_hash: None,
            public_url: "http://localhost:3000".to_string(),
            job_poll_interval_ms: 2000,
            job_max_concurrency: 4,
            job_cleanup_days: 7,
        },
        ws_hub: WsHub::new(),
        providers: ProviderPool::new(vec![]),
        memories: MemoriesClient::new("http://localhost:8900", None),
        session_token: TEST_TOKEN.to_string(),
    })
}

async fn body_to_json(body: Body) -> serde_json::Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ---------------------------------------------------------------------------
// Health (public, no auth required)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_no_auth_required() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let json = body_to_json(res.into_body()).await;
    assert_eq!(json["status"], "ok");
    assert_eq!(json["db"], true);
}

// ---------------------------------------------------------------------------
// Bootstrap token
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bootstrap_returns_token_for_same_origin() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/auth/bootstrap")
                .header("sec-fetch-site", "same-origin")
                .header("host", "localhost:3000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let set_cookie = res
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(set_cookie.contains("iris_session="));
    assert!(set_cookie.contains("HttpOnly"));
    let json = body_to_json(res.into_body()).await;
    assert_eq!(json["authenticated"], true);
}

#[tokio::test]
async fn bootstrap_rejects_cross_origin() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/auth/bootstrap")
                .header("sec-fetch-site", "cross-site")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Session auth enforcement
// ---------------------------------------------------------------------------

#[tokio::test]
async fn protected_route_rejects_no_token() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(Request::get("/api/accounts").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_route_rejects_wrong_token() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/accounts")
                .header("x-session-token", "wrong-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_route_accepts_valid_token() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/accounts")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Account CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_and_list_accounts() {
    let state = create_test_state();
    let app = build_app(state);

    // Create account
    let create_body = serde_json::json!({
        "provider": "imap",
        "email": "test@example.com",
        "imap_host": "mail.example.com",
        "imap_port": 993,
        "smtp_host": "mail.example.com",
        "smtp_port": 587,
        "username": "test@example.com",
        "password": "secret"
    });

    let res = app
        .clone()
        .oneshot(
            Request::post("/api/accounts")
                .header("x-session-token", TEST_TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(create_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::CREATED);
    let created = body_to_json(res.into_body()).await;
    assert_eq!(created["email"], "test@example.com");

    // List accounts
    let res = app
        .oneshot(
            Request::get("/api/accounts")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let list = body_to_json(res.into_body()).await;
    let accounts = list.as_array().unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["email"], "test@example.com");
}

// ---------------------------------------------------------------------------
// Messages endpoint (empty, but valid)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn messages_returns_ok_with_token() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/messages")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Batch update validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn batch_update_rejects_empty_ids() {
    let state = create_test_state();
    let app = build_app(state);

    let body = serde_json::json!({ "ids": [], "action": "read" });

    let res = app
        .oneshot(
            Request::patch("/api/messages/batch")
                .header("x-session-token", TEST_TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Search endpoint
// ---------------------------------------------------------------------------

#[tokio::test]
async fn search_empty_query_returns_ok() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/search?q=")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let json = body_to_json(res.into_body()).await;
    assert_eq!(json["total"], 0);
    assert_eq!(json["results"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn search_with_data_returns_results() {
    let state = create_test_state();

    // Insert a test account + message so FTS5 has data to search
    {
        let conn = state.db.get().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc1', 'imap', 'test@example.com')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, from_name, subject, body_text, date, is_read)
             VALUES ('msg1', 'acc1', 'INBOX', 'alice@example.com', 'Alice', 'Meeting tomorrow', 'Lets discuss the project plan', 1709500800, 0)",
            [],
        ).unwrap();
    }

    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/search?q=meeting")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let json = body_to_json(res.into_body()).await;
    assert_eq!(json["total"], 1);
    assert_eq!(json["results"].as_array().unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// CC Suggestions route registration check
// ---------------------------------------------------------------------------

#[tokio::test]
async fn suggest_cc_route_is_registered() {
    let state = create_test_state();
    let app = build_app(state);

    let body = r#"{"to":["alice@example.com"],"cc":[],"subject":"test","body_preview":"test"}"#;
    let res = app
        .oneshot(
            Request::post("/api/ai/suggest-cc")
                .header("content-type", "application/json")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should NOT be 405 - the route must be registered
    // Expected: 503 (AI not available in test) or 200, but NOT 405
    assert_ne!(
        res.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "Route POST /api/ai/suggest-cc must be registered in the router"
    );
}

// ---------------------------------------------------------------------------
// Reply endpoint — 404 for nonexistent message
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reply_returns_404_for_nonexistent_message() {
    let state = create_test_state();
    let app = build_app(state);

    let body = serde_json::json!({
        "message_id": "nonexistent-id",
        "body": "Thanks for the update!"
    });

    let res = app
        .oneshot(
            Request::post("/api/reply")
                .header("x-session-token", TEST_TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let json = body_to_json(res.into_body()).await;
    assert_eq!(json["error"], "message not found");
}

// ---------------------------------------------------------------------------
// Forward endpoint — 404 for nonexistent message
// ---------------------------------------------------------------------------

#[tokio::test]
async fn forward_returns_404_for_nonexistent_message() {
    let state = create_test_state();
    let app = build_app(state);

    let body = serde_json::json!({
        "message_id": "nonexistent-id",
        "to": ["someone@example.com"],
        "body": "FYI"
    });

    let res = app
        .oneshot(
            Request::post("/api/forward")
                .header("x-session-token", TEST_TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let json = body_to_json(res.into_body()).await;
    assert_eq!(json["error"], "message not found");
}

// ---------------------------------------------------------------------------
// Draft reply endpoint — 404 for nonexistent message
// ---------------------------------------------------------------------------

#[tokio::test]
async fn draft_reply_returns_404_for_nonexistent_message() {
    let state = create_test_state();
    let app = build_app(state);

    let body = serde_json::json!({
        "message_id": "nonexistent-id",
        "body": "Draft reply"
    });

    let res = app
        .oneshot(
            Request::post("/api/drafts/reply")
                .header("x-session-token", TEST_TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Draft forward endpoint — 404 for nonexistent message
// ---------------------------------------------------------------------------

#[tokio::test]
async fn draft_forward_returns_404_for_nonexistent_message() {
    let state = create_test_state();
    let app = build_app(state);

    let body = serde_json::json!({
        "message_id": "nonexistent-id",
        "to": ["someone@example.com"],
        "body": "Draft forward"
    });

    let res = app
        .oneshot(
            Request::post("/api/drafts/forward")
                .header("x-session-token", TEST_TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
