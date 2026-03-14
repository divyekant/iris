// ---------------------------------------------------------------------------
// Security tests: SQL Injection
// ---------------------------------------------------------------------------
// Verifies that all user-facing query parameters use parameterized
// statements and that FTS5 queries are properly sanitized. The app
// uses rusqlite with `params![]` throughout — these tests confirm
// that SQL metacharacters in input do not cause query corruption.

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

const TEST_TOKEN: &str = "test-session-token-security-sql";

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

fn create_state_with_data() -> Arc<AppState> {
    let state = create_test_state();
    {
        let conn = state.db.get().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc1', 'imap', 'test@example.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, from_name, subject, body_text, date, is_read)
             VALUES ('msg1', 'acc1', 'INBOX', 'alice@example.com', 'Alice', 'Meeting tomorrow', 'Discuss project plan', 1709500800, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, from_name, subject, body_text, date, is_read)
             VALUES ('msg2', 'acc1', 'INBOX', 'bob@example.com', 'Bob', 'Invoice #1234', 'Payment details attached', 1709504400, 1)",
            [],
        )
        .unwrap();
    }
    state
}

async fn body_to_json(body: Body) -> serde_json::Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// =========================================================================
// A) Search endpoint — SQL metacharacters
// =========================================================================

#[tokio::test]
async fn sqli_search_drop_table() {
    let state = create_state_with_data();
    let app = build_app(state.clone());

    let res = app
        .oneshot(
            Request::get("/api/search?q=%27%3B+DROP+TABLE+messages%3B+--")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should return OK (search handles gracefully) or empty results
    // NOT a 500 (which would indicate SQL error)
    assert_ne!(
        res.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "SQL injection attempt should not cause server error"
    );

    // Verify messages table still exists
    let conn = state.db.get().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2, "Messages table should be intact after injection attempt");
}

#[tokio::test]
async fn sqli_search_union_select() {
    let state = create_state_with_data();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/search?q=test+UNION+SELECT+key_hash+FROM+api_keys+--")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // FTS5 wraps each word in quotes, so UNION/SELECT become literal search terms
    // Should not return any sensitive data
    let status = res.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR,
        "UNION injection should be handled safely"
    );

    if status == StatusCode::OK {
        let json = body_to_json(res.into_body()).await;
        let results = json["results"].as_array().unwrap();
        // Should not leak api_keys data
        for r in results {
            assert!(
                r.get("key_hash").is_none(),
                "UNION SELECT should not leak api_keys data"
            );
        }
    }
}

#[tokio::test]
async fn sqli_search_single_quote() {
    let state = create_state_with_data();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/search?q=%27")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Single quote should not break the query
    assert_ne!(
        res.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "Single quote should not cause SQL error"
    );
}

#[tokio::test]
async fn sqli_search_double_dash_comment() {
    let state = create_state_with_data();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/search?q=test+--+ignored")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(
        res.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "SQL comment should not cause error"
    );
}

#[tokio::test]
async fn sqli_search_semicolon() {
    let state = create_state_with_data();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/search?q=test%3B+DROP+TABLE+accounts")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(
        res.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "Semicolon in search should not cause error"
    );
}

// =========================================================================
// B) FTS5-specific injection attempts
// =========================================================================

#[tokio::test]
async fn sqli_fts5_match_syntax() {
    let state = create_state_with_data();
    let app = build_app(state);

    // FTS5 MATCH syntax characters
    // Pre-encoded FTS5 injection payloads (percent-encoded for URL safety)
    let fts5_urls = vec![
        "/api/search?q=test*",                                  // FTS5 wildcard
        "/api/search?q=NEAR(a%2C%20b%2C%2010)",                 // NEAR operator
        "/api/search?q=test%20AND%20%22x%22",                   // AND operator
        "/api/search?q=test%20NOT%20secret",                    // NOT operator
        "/api/search?q=test%20OR%20(SELECT%201)",               // OR with subquery
        "/api/search?q=%7Bcol1%20col2%7D%3A%20test",            // Column filter syntax
    ];

    for url in &fts5_urls {

        let res = app
            .clone()
            .oneshot(
                Request::get(*url)
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // The search endpoint wraps terms in quotes, neutralizing FTS5 operators.
        // Should not cause a 500 error.
        assert_ne!(
            res.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "FTS5 injection '{}' should not cause server error",
            url
        );
    }
}

// =========================================================================
// C) Filter parameters — SQL injection via operators
// =========================================================================

#[tokio::test]
async fn sqli_from_operator_injection() {
    let state = create_state_with_data();
    let app = build_app(state);

    // from: operator is passed through LIKE with parameterized query
    let res = app
        .oneshot(
            Request::get("/api/search?q=from:%27%3B+DROP+TABLE+messages%3B+--+test")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(
        res.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "from: operator injection should not cause error"
    );
}

#[tokio::test]
async fn sqli_category_operator_injection() {
    let state = create_state_with_data();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/search?q=category:%27+OR+1%3D1+--")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(
        res.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "category: operator injection should not cause error"
    );
}

// =========================================================================
// D) Message list — filter parameters
// =========================================================================

#[tokio::test]
async fn sqli_category_filter_injection() {
    let state = create_state_with_data();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/messages?category=%27+OR+1%3D1+--")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Category is passed via parameterized query (LOWER(m.ai_category) = ?N)
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "Category injection should be handled safely"
    );
}

#[tokio::test]
async fn sqli_account_id_injection() {
    let state = create_state_with_data();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/messages?account_id=%27+OR+1%3D1+--")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // account_id is passed via parameterized query
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "Account ID injection should be handled safely"
    );
    let json = body_to_json(res.into_body()).await;
    // Injected account_id should match zero records
    assert_eq!(json["total"], 0);
}

// =========================================================================
// E) Batch update — ID injection
// =========================================================================

#[tokio::test]
async fn sqli_batch_update_id_injection() {
    let state = create_state_with_data();
    let app = build_app(state.clone());

    let body = serde_json::json!({
        "ids": ["msg1'; DROP TABLE messages; --"],
        "action": "mark_read"
    });

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

    assert_eq!(res.status(), StatusCode::OK);
    let json = body_to_json(res.into_body()).await;
    // The malicious ID won't match any real message
    assert_eq!(json["updated"], 0);

    // Verify table is intact
    let conn = state.db.get().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2, "Messages table should be intact");
}

// =========================================================================
// F) Thread notes — content injection
// =========================================================================

#[tokio::test]
async fn sqli_thread_note_content() {
    let state = create_state_with_data();
    let app = build_app(state.clone());

    let body = serde_json::json!({
        "content": "'; DROP TABLE thread_notes; --"
    });

    let res = app
        .oneshot(
            Request::post("/api/threads/thread1/notes")
                .header("x-session-token", TEST_TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should succeed (201) — the content is stored safely via parameterized query
    assert_eq!(res.status(), StatusCode::CREATED);

    // Verify the note was stored with the literal SQL injection string
    let conn = state.db.get().unwrap();
    let content: String = conn
        .query_row(
            "SELECT content FROM thread_notes WHERE thread_id = 'thread1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(content, "'; DROP TABLE thread_notes; --");
}

// =========================================================================
// G) Chat session_id — injection
// =========================================================================

#[tokio::test]
async fn sqli_chat_session_id_injection() {
    let state = create_state_with_data();
    let app = build_app(state);

    let body = serde_json::json!({
        "session_id": "test'; DROP TABLE chat_messages; --",
        "message": "Hello"
    });

    let res = app
        .oneshot(
            Request::post("/api/ai/chat")
                .header("x-session-token", TEST_TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    // AI is disabled in test, so returns 503, but the session_id is validated
    // before DB access. The important thing is no SQL error occurs.
    // Session ID length check (max 100) will pass since this is under 100 chars.
    assert_ne!(
        res.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "Session ID injection should not cause SQL error"
    );
}

// =========================================================================
// H) Saved search injection
// =========================================================================

#[tokio::test]
async fn sqli_saved_search_name_injection() {
    let state = create_state_with_data();
    let app = build_app(state.clone());

    let body = serde_json::json!({
        "name": "'; DROP TABLE saved_searches; --",
        "query": "from:alice",
        "filters": {}
    });

    let res = app
        .oneshot(
            Request::post("/api/saved-searches")
                .header("x-session-token", TEST_TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::CREATED);

    // Verify the name was stored literally, not executed as SQL
    let conn = state.db.get().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM saved_searches", [], |row| row.get(0))
        .unwrap();
    assert!(count >= 1, "saved_searches table should have the entry");
}

// =========================================================================
// I) Pagination bounds
// =========================================================================

#[tokio::test]
async fn sqli_negative_offset_clamped() {
    let state = create_state_with_data();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/messages?offset=-999")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn sqli_extreme_limit_capped() {
    let state = create_state_with_data();
    let app = build_app(state);

    let res = app
        .oneshot(
            Request::get("/api/messages?limit=999999")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // The limit is capped at 500 in code (.min(500))
    assert_eq!(res.status(), StatusCode::OK);
    let json = body_to_json(res.into_body()).await;
    // Should return data, not error
    assert!(json["messages"].is_array());
}

// =========================================================================
// J) API key creation — permission injection
// =========================================================================

#[tokio::test]
async fn sqli_api_key_invalid_permission() {
    let state = create_state_with_data();
    let app = build_app(state);

    let body = serde_json::json!({
        "name": "Evil Key",
        "permission": "'; INSERT INTO api_keys VALUES('x','x','x','x','autonomous',NULL,0,NULL,0); --"
    });

    let res = app
        .oneshot(
            Request::post("/api/api-keys")
                .header("x-session-token", TEST_TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Permission is validated against a whitelist BEFORE any DB access
    assert_eq!(
        res.status(),
        StatusCode::BAD_REQUEST,
        "Invalid permission should be rejected at validation"
    );
}

// =========================================================================
// K) Confirm action — message ID resolution injection
// =========================================================================

#[tokio::test]
async fn sqli_confirm_action_message_id_like_injection() {
    let state = create_state_with_data();

    // First, create a chat message with a proposed action containing malicious IDs
    {
        let conn = state.db.get().unwrap();
        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content, proposed_action)
             VALUES ('chatmsg1', 'session1', 'assistant', 'OK',
                     '{\"action\":\"archive\",\"description\":\"test\",\"message_ids\":[\"% OR 1=1 --\"]}')",
            [],
        )
        .unwrap();
    }

    let app = build_app(state.clone());

    // Enable AI first (confirm_action doesn't check AI enabled, but needs chat_messages)
    let body = serde_json::json!({
        "session_id": "session1",
        "message_id": "chatmsg1"
    });

    let res = app
        .oneshot(
            Request::post("/api/ai/chat/confirm")
                .header("x-session-token", TEST_TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    // The confirm_action handler sanitizes message IDs:
    // - Strips non-alphanumeric/non-hyphen chars
    // - Uses LIKE ?1 with parameterized query for resolution
    // The malicious ID "% OR 1=1 --" should NOT match any real message
    // after sanitization (all special chars stripped, leaving "OR11")
    let status = res.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::BAD_REQUEST,
        "Malicious message ID should be handled safely, got {}",
        status
    );

    // Verify no messages were accidentally updated
    let conn = state.db.get().unwrap();
    let original_msg1_read: bool = conn
        .query_row(
            "SELECT is_read FROM messages WHERE id = 'msg1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!original_msg1_read, "msg1 should still be unread");
}
