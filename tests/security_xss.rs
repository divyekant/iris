// ---------------------------------------------------------------------------
// Security tests: XSS Prevention
// ---------------------------------------------------------------------------
// Tests the HTML sanitization layer used for markdown preview and email
// content rendering. The regex-based sanitizer in src/api/markdown.rs
// strips script tags, event handlers, and javascript: URLs.
//
// The email HTML viewer uses a sandboxed iframe as the primary security
// boundary. This sanitizer is defense-in-depth for markdown-to-HTML.

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

const TEST_TOKEN: &str = "test-session-token-security-xss";

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

async fn get_sanitized_html(app: axum::Router, markdown: &str) -> String {
    let body = serde_json::json!({ "markdown": markdown });
    let res = app
        .oneshot(
            Request::post("/api/compose/markdown-preview")
                .header("x-session-token", TEST_TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    json["html"].as_str().unwrap().to_string()
}

// =========================================================================
// Basic script injection
// =========================================================================

#[tokio::test]
async fn xss_basic_script_tag() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, "<script>alert('xss')</script>").await;
    assert!(!html.contains("<script>"), "script tag not stripped");
    assert!(!html.contains("alert("), "alert() not stripped");
}

#[tokio::test]
async fn xss_script_tag_uppercase() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, "<SCRIPT>alert('xss')</SCRIPT>").await;
    assert!(!html.to_lowercase().contains("<script>"), "uppercase script tag not stripped");
}

#[tokio::test]
async fn xss_script_tag_mixed_case() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, "<ScRiPt>alert('xss')</sCrIpT>").await;
    assert!(!html.to_lowercase().contains("<script>"), "mixed-case script tag not stripped");
}

// =========================================================================
// Event handler injection
// =========================================================================

#[tokio::test]
async fn xss_img_onerror() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, r#"<img src=x onerror="alert('xss')">"#).await;
    assert!(!html.contains("onerror"), "onerror handler not stripped");
}

#[tokio::test]
async fn xss_onload_handler() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, r#"<body onload="alert('xss')">"#).await;
    assert!(!html.contains("onload"), "onload handler not stripped");
}

#[tokio::test]
async fn xss_onclick_handler() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, r#"<div onclick="alert('xss')">Click me</div>"#).await;
    assert!(!html.contains("onclick"), "onclick handler not stripped");
}

#[tokio::test]
async fn xss_onmouseover_handler() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, r#"<a onmouseover="alert(1)">hover</a>"#).await;
    assert!(!html.contains("onmouseover"), "onmouseover handler not stripped");
}

#[tokio::test]
async fn xss_onfocus_handler() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, r#"<input onfocus="alert(1)" autofocus>"#).await;
    assert!(!html.contains("onfocus"), "onfocus handler not stripped");
}

// =========================================================================
// SVG injection
// =========================================================================

#[tokio::test]
async fn xss_svg_onload() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, r#"<svg onload="alert('xss')">"#).await;
    assert!(!html.contains("onload"), "svg onload not stripped");
}

// =========================================================================
// javascript: protocol
// =========================================================================

#[tokio::test]
async fn xss_javascript_href() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, r#"[click](javascript:alert('xss'))"#).await;
    assert!(!html.to_lowercase().contains("javascript:alert"), "javascript: protocol not neutralized");
}

#[tokio::test]
async fn xss_javascript_href_uppercase() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, r#"<a href="JAVASCRIPT:alert(1)">click</a>"#).await;
    assert!(!html.to_lowercase().contains("javascript:alert"), "uppercase javascript: not neutralized");
}

#[tokio::test]
async fn xss_javascript_href_with_whitespace() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, r#"<a href="  javascript:alert(1)">click</a>"#).await;
    assert!(!html.to_lowercase().contains("javascript:alert"), "whitespace-padded javascript: not neutralized");
}

// =========================================================================
// iframe injection
// =========================================================================

#[tokio::test]
async fn xss_iframe_tag() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(
        app,
        r#"<iframe src="data:text/html,<script>alert(1)</script>"></iframe>"#,
    )
    .await;
    assert!(!html.to_lowercase().contains("<iframe"), "iframe tag not stripped");
}

#[tokio::test]
async fn xss_iframe_self_closing() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(
        app,
        r#"<iframe src="https://evil.com" />"#,
    )
    .await;
    assert!(!html.to_lowercase().contains("<iframe"), "self-closing iframe not stripped");
}

// =========================================================================
// Multiline and complex payloads
// =========================================================================

#[tokio::test]
async fn xss_script_multiline() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(
        app,
        "<script>\n  var x = 1;\n  alert(x);\n</script>",
    )
    .await;
    assert!(!html.contains("<script>"), "multiline script tag not stripped");
    assert!(!html.contains("alert("), "multiline alert() not stripped");
}

#[tokio::test]
async fn xss_script_with_attributes() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(
        app,
        r#"<script type="text/javascript" src="https://evil.com/xss.js"></script>"#,
    )
    .await;
    assert!(!html.to_lowercase().contains("<script"), "script with attributes not stripped");
}

// =========================================================================
// Event handlers with various quoting styles
// =========================================================================

#[tokio::test]
async fn xss_event_handler_single_quotes() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, r#"<img src=x onerror='alert(1)'>"#).await;
    assert!(!html.contains("onerror"), "single-quoted onerror not stripped");
}

#[tokio::test]
async fn xss_event_handler_no_quotes() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, r#"<img src=x onerror=alert(1)>"#).await;
    assert!(!html.contains("onerror"), "unquoted onerror not stripped");
}

// =========================================================================
// Safe content preservation
// =========================================================================

#[tokio::test]
async fn xss_safe_html_preserved() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(
        app,
        "**bold** and *italic* and [link](https://example.com)",
    )
    .await;
    assert!(html.contains("<strong>bold</strong>"), "bold not preserved");
    assert!(html.contains("<em>italic</em>"), "italic not preserved");
    assert!(html.contains(r#"href="https://example.com""#), "safe link not preserved");
}

#[tokio::test]
async fn xss_safe_images_preserved() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(
        app,
        r#"![alt text](https://example.com/image.png)"#,
    )
    .await;
    assert!(html.contains("https://example.com/image.png"), "safe image src not preserved");
}

// =========================================================================
// Edge cases
// =========================================================================

#[tokio::test]
async fn xss_empty_input() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, "").await;
    assert!(html.is_empty() || html.trim().is_empty());
}

#[tokio::test]
async fn xss_plain_text_safe() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(app, "Just plain text, no HTML here.").await;
    assert!(html.contains("Just plain text"));
    assert!(!html.contains("<script>"));
}

#[tokio::test]
async fn xss_nested_tags() {
    let state = create_test_state();
    let app = build_app(state);

    // Nested dangerous tags
    let html = get_sanitized_html(
        app,
        r#"<div><script>alert(1)</script><p>Safe</p></div>"#,
    )
    .await;
    assert!(!html.contains("<script>"), "nested script not stripped");
    // The safe paragraph content should survive in some form
}

#[tokio::test]
async fn xss_multiple_event_handlers() {
    let state = create_test_state();
    let app = build_app(state);

    let html = get_sanitized_html(
        app,
        r#"<div onclick="alert(1)" onmouseover="alert(2)" onfocus="alert(3)">test</div>"#,
    )
    .await;
    assert!(!html.contains("onclick"), "onclick not stripped");
    assert!(!html.contains("onmouseover"), "onmouseover not stripped");
    assert!(!html.contains("onfocus"), "onfocus not stripped");
}

// =========================================================================
// Content-Disposition header injection (attachment filename)
// =========================================================================

#[tokio::test]
async fn xss_attachment_filename_sanitized() {
    // Test that attachment download sanitizes filename in Content-Disposition.
    // The download handler replaces double quotes with single quotes.
    let state = create_test_state();
    {
        let conn = state.db.get().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc1', 'imap', 'test@example.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, subject, date, is_read)
             VALUES ('msg1', 'acc1', 'INBOX', 'a@b.com', 'test', 1710000000, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attachments (id, message_id, filename, content_type, size, data)
             VALUES ('att1', 'msg1', '\"malicious\".html', 'text/html', 10, X'48656C6C6F')",
            [],
        )
        .unwrap();
    }

    let app = build_app(state);
    let res = app
        .oneshot(
            Request::get("/api/attachments/att1/download")
                .header("x-session-token", TEST_TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let disposition = res
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap();

    // The handler replaces " with ' to prevent header injection
    assert!(
        !disposition.contains(r#""malicious""#),
        "double quotes in filename should be replaced: {}",
        disposition
    );
    assert!(disposition.contains("attachment;"), "should be attachment disposition");
}
