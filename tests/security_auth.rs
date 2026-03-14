// ---------------------------------------------------------------------------
// Security tests: Authentication & Authorization Bypass
// ---------------------------------------------------------------------------
// Tests that session auth, API key auth, CSRF protections, and access
// controls cannot be bypassed with crafted requests.

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

const TEST_TOKEN: &str = "test-session-token-security-auth";

fn create_test_state() -> Arc<AppState> {
    let manager = SqliteConnectionManager::memory().with_init(|conn| {
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )
    });
    // Need at least 2 connections: agent_auth_middleware holds one while the handler
    // acquires another within the same request lifecycle.
    let pool = Pool::builder().max_size(2).build(manager).unwrap();
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

fn create_test_state_with_agent_key() -> (Arc<AppState>, String) {
    let state = create_test_state();
    let raw_key = {
        let conn = state.db.get().unwrap();
        let (raw_key, _) = iris_server::api::agent::create_api_key(
            &conn,
            "Test Agent",
            "read_only",
            None,
        )
        .unwrap();
        raw_key
    };
    (state, raw_key)
}

async fn body_to_json(body: Body) -> serde_json::Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// =========================================================================
// A) Session auth bypass attempts
// =========================================================================

#[tokio::test]
async fn auth_no_token_rejected() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(Request::get("/api/accounts").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_empty_token_rejected() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/accounts")
                .header("x-session-token", "")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_wrong_token_rejected() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/accounts")
                .header("x-session-token", "wrong-token-value")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_token_from_different_session_rejected() {
    let state = create_test_state();
    let app = build_app(state);

    // Use a token that looks real but is from a different session
    let res = app
        .oneshot(
            Request::get("/api/accounts")
                .header("x-session-token", "other-session-token-xyz789")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_valid_token_accepted() {
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

#[tokio::test]
async fn auth_token_in_wrong_header_rejected() {
    let state = create_test_state();
    let app = build_app(state);

    // Token in Authorization header instead of x-session-token
    let res = app
        .oneshot(
            Request::get("/api/accounts")
                .header("authorization", format!("Bearer {}", TEST_TOKEN))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_token_in_query_param_rejected() {
    let state = create_test_state();
    let app = build_app(state);

    // Token as query parameter should not work for REST endpoints
    let res = app
        .oneshot(
            Request::get(&format!("/api/accounts?token={}", TEST_TOKEN))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_protects_all_critical_endpoints() {
    let state = create_test_state();
    let app = build_app(state);

    let endpoints = vec![
        ("GET", "/api/accounts"),
        ("GET", "/api/messages"),
        ("GET", "/api/search?q=test"),
        ("GET", "/api/config"),
        ("GET", "/api/drafts"),
        ("GET", "/api/labels"),
        ("GET", "/api/api-keys"),
        ("GET", "/api/audit-log"),
    ];

    for (method, path) in endpoints {
        let req = match method {
            "GET" => Request::get(path).body(Body::empty()).unwrap(),
            "POST" => Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
            _ => panic!("unexpected method"),
        };

        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "Endpoint {} {} should require auth",
            method,
            path
        );
    }
}

// =========================================================================
// B) API key auth bypass
// =========================================================================

#[tokio::test]
async fn agent_no_bearer_token_rejected() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/agent/search?q=test")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Agent routes are nested under session auth AND agent auth.
    // Without Bearer token in Authorization header, agent_auth_middleware returns 401.
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn agent_invalid_bearer_token_rejected() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/agent/search?q=test")
                .header("x-session-token", TEST_TOKEN)
                .header("authorization", "Bearer iris_invalidkey0000000000000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn agent_revoked_key_rejected() {
    let state = create_test_state();
    let raw_key;
    {
        let conn = state.db.get().unwrap();
        let (key, stored) = iris_server::api::agent::create_api_key(
            &conn,
            "Revoked Agent",
            "read_only",
            None,
        )
        .unwrap();
        raw_key = key;
        iris_server::api::agent::revoke_api_key(&conn, &stored.id);
    }

    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/agent/search?q=test")
                .header("x-session-token", TEST_TOKEN)
                .header("authorization", format!("Bearer {}", raw_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn agent_read_only_key_cannot_create_draft() {
    let (state, raw_key) = create_test_state_with_agent_key();
    let app = build_app(state);

    let body = serde_json::json!({
        "account_id": "acc1",
        "body_text": "Hello"
    });

    let res = app
        .oneshot(
            Request::post("/api/agent/drafts")
                .header("x-session-token", TEST_TOKEN)
                .header("authorization", format!("Bearer {}", raw_key))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    // read_only key lacks "draft" permission
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn agent_read_only_key_cannot_send() {
    let (state, raw_key) = create_test_state_with_agent_key();
    let app = build_app(state);

    let body = serde_json::json!({
        "account_id": "acc1",
        "to": ["victim@example.com"],
        "subject": "test",
        "body_text": "Hello"
    });

    let res = app
        .oneshot(
            Request::post("/api/agent/send")
                .header("x-session-token", TEST_TOKEN)
                .header("authorization", format!("Bearer {}", raw_key))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    // read_only key lacks "send" permission
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn agent_scoped_key_cannot_read_other_accounts_messages() {
    let state = create_test_state();
    let raw_key;
    {
        let conn = state.db.get().unwrap();
        // Create account and message in account "acc1"
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc1', 'imap', 'alice@example.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc2', 'imap', 'bob@example.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, subject, date, is_read, body_text)
             VALUES ('msg-bob', 'acc2', 'INBOX', 'x@y.com', 'Secret', 1710000000, 0, 'Confidential content')",
            [],
        )
        .unwrap();

        // Create a key scoped to acc1 only
        let (key, _) = iris_server::api::agent::create_api_key(
            &conn,
            "Scoped Agent",
            "read_only",
            Some("acc1"),
        )
        .unwrap();
        raw_key = key;
    }

    let app = build_app(state);

    // Try to read a message belonging to acc2 using a key scoped to acc1
    let res = app
        .oneshot(
            Request::get("/api/agent/messages/msg-bob")
                .header("x-session-token", TEST_TOKEN)
                .header("authorization", format!("Bearer {}", raw_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Scoped key should not access other account's messages"
    );
}

// =========================================================================
// C) CSRF / Origin checks
// =========================================================================

#[tokio::test]
async fn bootstrap_rejects_cross_site() {
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

#[tokio::test]
async fn bootstrap_rejects_no_fetch_site_header() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/auth/bootstrap")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn bootstrap_accepts_same_origin() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/auth/bootstrap")
                .header("sec-fetch-site", "same-origin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let json = body_to_json(res.into_body()).await;
    assert_eq!(json["token"], TEST_TOKEN);
}

#[tokio::test]
async fn bootstrap_accepts_none_fetch_site() {
    // "none" = direct navigation, e.g. typing URL in address bar
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/auth/bootstrap")
                .header("sec-fetch-site", "none")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn bootstrap_accepts_same_site() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/auth/bootstrap")
                .header("sec-fetch-site", "same-site")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

// =========================================================================
// D) Path traversal and ID manipulation
// =========================================================================

#[tokio::test]
async fn attachment_download_rejects_nonexistent_id() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/attachments/../../etc/passwd/download")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // The router normalizes paths. Axum does not match traversal paths
    // as valid route parameters. The result is either 404 or the path
    // gets normalized and doesn't find a matching record (returning 200
    // with empty/error body). Either way, no real file content is leaked.
    let status = res.status();
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap_or_default();
    let body_str = String::from_utf8_lossy(&body);
    assert!(
        !body_str.contains("root:") && !body_str.contains("/bin/"),
        "Path traversal must not leak system files"
    );
}

#[tokio::test]
async fn attachment_download_with_safe_nonexistent_id() {
    // Attachment download looks up by ID in the database (parameterized query).
    // Non-existent ID returns 404.
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/attachments/nonexistent-uuid-1234/download")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn message_detail_with_nonexistent_id() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/messages/nonexistent-id")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

// =========================================================================
// E) Batch operation limits
// =========================================================================

#[tokio::test]
async fn batch_update_rejects_empty_ids() {
    let state = create_test_state();
    let app = build_app(state);

    let body = serde_json::json!({ "ids": [], "action": "mark_read" });
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

#[tokio::test]
async fn batch_update_rejects_oversized_ids_list() {
    let state = create_test_state();
    let app = build_app(state);

    // > 1000 IDs should be rejected to prevent DoS
    let ids: Vec<String> = (0..1001).map(|i| format!("msg-{}", i)).collect();
    let body = serde_json::json!({ "ids": ids, "action": "mark_read" });

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

#[tokio::test]
async fn batch_update_rejects_invalid_action() {
    let state = create_test_state();
    let app = build_app(state);

    let body = serde_json::json!({ "ids": ["msg1"], "action": "drop_table" });
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

// =========================================================================
// F) Folder allowlist validation
// =========================================================================

#[tokio::test]
async fn messages_folder_injection_defaults_to_inbox() {
    // The list_messages endpoint validates folder against an allowlist.
    // Any invalid folder defaults to INBOX.
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/messages?folder=%27%3B%20DROP%20TABLE%20messages%3B%20--")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should succeed (defaults to INBOX), not error
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_to_json(res.into_body()).await;
    // Should return valid response structure
    assert!(json["messages"].is_array());
}

// =========================================================================
// G) Health endpoint is always public
// =========================================================================

#[tokio::test]
async fn health_accessible_without_auth() {
    let state = create_test_state();
    let app = build_app(state);

    let res = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let json = body_to_json(res.into_body()).await;
    assert_eq!(json["status"], "ok");
}

// =========================================================================
// H) Permission hierarchy tests (unit level)
// =========================================================================

#[test]
fn permission_hierarchy_read_only_is_restricted() {
    use iris_server::api::agent::has_permission;

    assert!(has_permission("read_only", "read"));
    assert!(has_permission("read_only", "search"));
    assert!(!has_permission("read_only", "draft"));
    assert!(!has_permission("read_only", "send"));
    assert!(!has_permission("read_only", "execute"));
    assert!(!has_permission("read_only", "configure"));
}

#[test]
fn permission_hierarchy_draft_only() {
    use iris_server::api::agent::has_permission;

    assert!(has_permission("draft_only", "read"));
    assert!(has_permission("draft_only", "search"));
    assert!(has_permission("draft_only", "draft"));
    assert!(!has_permission("draft_only", "send"));
}

#[test]
fn permission_hierarchy_send_with_approval() {
    use iris_server::api::agent::has_permission;

    assert!(has_permission("send_with_approval", "read"));
    assert!(has_permission("send_with_approval", "search"));
    assert!(has_permission("send_with_approval", "draft"));
    assert!(has_permission("send_with_approval", "send"));
    assert!(!has_permission("send_with_approval", "execute"));
    assert!(!has_permission("send_with_approval", "configure"));
}

#[test]
fn permission_hierarchy_invalid_permission_denied() {
    use iris_server::api::agent::has_permission;

    assert!(!has_permission("superadmin", "read"));
    assert!(!has_permission("superadmin", "send"));
    assert!(!has_permission("", "read"));
    assert!(!has_permission("root", "execute"));
}
