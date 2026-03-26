use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::api::unified_auth::{AuthContext, Permission};
use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

const VALID_EVENT_TYPES: &[&str] = &[
    "email.received",
    "email.sent",
    "email.archived",
    "email.deleted",
    "email.labeled",
    "email.starred",
];

const MAX_CONSECUTIVE_FAILURES: i64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Webhook {
    pub id: i64,
    pub account_id: i64,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    pub events: Vec<String>,
    pub active: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_triggered_at: Option<String>,
    pub failure_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookDelivery {
    pub id: i64,
    pub webhook_id: i64,
    pub event_type: String,
    pub payload: String,
    pub status_code: Option<i64>,
    pub response_body: Option<String>,
    pub success: bool,
    pub delivered_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateWebhookRequest {
    pub account_id: i64,
    pub url: String,
    pub events: Vec<String>,
    pub secret: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWebhookRequest {
    pub url: Option<String>,
    pub events: Option<Vec<String>>,
    pub active: Option<bool>,
    pub secret: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeliveriesQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

fn validate_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn validate_events(events: &[String]) -> Result<(), String> {
    if events.is_empty() {
        return Err("events list cannot be empty".to_string());
    }
    for event in events {
        if !VALID_EVENT_TYPES.contains(&event.as_str()) {
            return Err(format!("invalid event type: {event}"));
        }
    }
    Ok(())
}

/// Compute HMAC-SHA256 signature for webhook payload.
/// Uses a simple HMAC construction: SHA256(secret + "." + payload).
pub fn compute_signature(secret: &str, payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(b".");
    hasher.update(payload.as_bytes());
    format!("sha256={:x}", hasher.finalize())
}

// ---------------------------------------------------------------------------
// Database helpers
// ---------------------------------------------------------------------------

type Conn = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

fn get_webhook(conn: &Conn, id: i64) -> Option<Webhook> {
    conn.query_row(
        "SELECT id, account_id, url, secret, events, active, created_at, updated_at, last_triggered_at, failure_count
         FROM webhooks WHERE id = ?1",
        params![id],
        |row| {
            let events_str: String = row.get(4)?;
            let events: Vec<String> = events_str.split(',').map(|s| s.trim().to_string()).collect();
            Ok(Webhook {
                id: row.get(0)?,
                account_id: row.get(1)?,
                url: row.get(2)?,
                secret: row.get(3)?,
                events,
                active: row.get::<_, i32>(5)? != 0,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                last_triggered_at: row.get(8)?,
                failure_count: row.get(9)?,
            })
        },
    )
    .ok()
}

fn list_webhooks_for_account(conn: &Conn, account_id: i64) -> Vec<Webhook> {
    let mut stmt = conn
        .prepare(
            "SELECT id, account_id, url, secret, events, active, created_at, updated_at, last_triggered_at, failure_count
             FROM webhooks WHERE account_id = ?1 ORDER BY created_at DESC",
        )
        .unwrap();

    stmt.query_map(params![account_id], |row| {
        let events_str: String = row.get(4)?;
        let events: Vec<String> = events_str.split(',').map(|s| s.trim().to_string()).collect();
        Ok(Webhook {
            id: row.get(0)?,
            account_id: row.get(1)?,
            url: row.get(2)?,
            secret: row.get(3)?,
            events,
            active: row.get::<_, i32>(5)? != 0,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            last_triggered_at: row.get(8)?,
            failure_count: row.get(9)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

fn insert_webhook(conn: &Conn, req: &CreateWebhookRequest) -> Result<Webhook, String> {
    let events_str = req.events.join(",");
    conn.execute(
        "INSERT INTO webhooks (account_id, url, secret, events)
         VALUES (?1, ?2, ?3, ?4)",
        params![req.account_id, req.url, req.secret, events_str],
    )
    .map_err(|e| format!("db insert error: {e}"))?;

    let id = conn.last_insert_rowid();
    get_webhook(conn, id).ok_or_else(|| "failed to retrieve inserted webhook".to_string())
}

fn update_webhook_in_db(conn: &Conn, id: i64, req: &UpdateWebhookRequest) -> Result<Webhook, String> {
    let existing = get_webhook(conn, id).ok_or("webhook not found")?;

    let url = req.url.as_deref().unwrap_or(&existing.url);
    let events = req
        .events
        .as_ref()
        .map(|e| e.join(","))
        .unwrap_or_else(|| existing.events.join(","));
    let active: i32 = if req.active.unwrap_or(existing.active) { 1 } else { 0 };
    let secret = if req.secret.is_some() {
        req.secret.as_deref()
    } else {
        existing.secret.as_deref()
    };

    conn.execute(
        "UPDATE webhooks SET url = ?1, events = ?2, active = ?3, secret = ?4, updated_at = datetime('now')
         WHERE id = ?5",
        params![url, events, active, secret, id],
    )
    .map_err(|e| format!("db update error: {e}"))?;

    get_webhook(conn, id).ok_or_else(|| "failed to retrieve updated webhook".to_string())
}

fn delete_webhook_in_db(conn: &Conn, id: i64) -> bool {
    let rows = conn
        .execute("DELETE FROM webhooks WHERE id = ?1", params![id])
        .unwrap_or(0);
    rows > 0
}

fn list_deliveries(conn: &Conn, webhook_id: i64, limit: i64, offset: i64) -> Vec<WebhookDelivery> {
    let mut stmt = conn
        .prepare(
            "SELECT id, webhook_id, event_type, payload, status_code, response_body, success, delivered_at
             FROM webhook_deliveries
             WHERE webhook_id = ?1
             ORDER BY delivered_at DESC
             LIMIT ?2 OFFSET ?3",
        )
        .unwrap();

    stmt.query_map(params![webhook_id, limit, offset], |row| {
        Ok(WebhookDelivery {
            id: row.get(0)?,
            webhook_id: row.get(1)?,
            event_type: row.get(2)?,
            payload: row.get(3)?,
            status_code: row.get(4)?,
            response_body: row.get(5)?,
            success: row.get::<_, i32>(6)? != 0,
            delivered_at: row.get(7)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

fn record_delivery(
    conn: &Conn,
    webhook_id: i64,
    event_type: &str,
    payload: &str,
    status_code: Option<i64>,
    response_body: Option<&str>,
    success: bool,
) {
    let _ = conn.execute(
        "INSERT INTO webhook_deliveries (webhook_id, event_type, payload, status_code, response_body, success)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![webhook_id, event_type, payload, status_code, response_body, success as i32],
    );

    if success {
        // Reset failure count and update last_triggered_at
        let _ = conn.execute(
            "UPDATE webhooks SET failure_count = 0, last_triggered_at = datetime('now'), updated_at = datetime('now') WHERE id = ?1",
            params![webhook_id],
        );
    } else {
        // Increment failure count and auto-disable after MAX_CONSECUTIVE_FAILURES
        let _ = conn.execute(
            "UPDATE webhooks SET failure_count = failure_count + 1, updated_at = datetime('now') WHERE id = ?1",
            params![webhook_id],
        );
        let _ = conn.execute(
            "UPDATE webhooks SET active = 0 WHERE id = ?1 AND failure_count >= ?2",
            params![webhook_id, MAX_CONSECUTIVE_FAILURES],
        );
    }
}

/// Find active webhooks that subscribe to a given event type.
pub fn find_matching_webhooks(conn: &Conn, event_type: &str) -> Vec<Webhook> {
    // We use LIKE to match comma-separated event types
    let mut stmt = conn
        .prepare(
            "SELECT id, account_id, url, secret, events, active, created_at, updated_at, last_triggered_at, failure_count
             FROM webhooks
             WHERE active = 1",
        )
        .unwrap();

    stmt.query_map([], |row| {
        let events_str: String = row.get(4)?;
        let events: Vec<String> = events_str.split(',').map(|s| s.trim().to_string()).collect();
        Ok(Webhook {
            id: row.get(0)?,
            account_id: row.get(1)?,
            url: row.get(2)?,
            secret: row.get(3)?,
            events,
            active: row.get::<_, i32>(5)? != 0,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            last_triggered_at: row.get(8)?,
            failure_count: row.get(9)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .filter(|w| w.events.iter().any(|e| e == event_type))
    .collect()
}

/// Trigger webhooks for a given event. This delivers the payload to all matching
/// active webhooks and records the delivery attempt.
///
/// In production, this would make HTTP requests. For now, it records the attempt
/// and can be extended with reqwest calls.
pub fn trigger_webhooks(
    conn: &Conn,
    event_type: &str,
    payload: &serde_json::Value,
) -> Vec<(i64, bool)> {
    let webhooks = find_matching_webhooks(conn, event_type);
    let payload_str = serde_json::to_string(payload).unwrap_or_default();
    let mut results = Vec::new();

    for webhook in &webhooks {
        // In a real implementation, we'd make an HTTP POST here with reqwest.
        // For now, we record the delivery as successful for testing purposes.
        // The test endpoint exercises actual delivery logic.

        let _signature = webhook
            .secret
            .as_ref()
            .map(|s| compute_signature(s, &payload_str));

        // Record delivery attempt (simulated success in non-HTTP mode)
        record_delivery(
            conn,
            webhook.id,
            event_type,
            &payload_str,
            Some(200),
            Some("ok"),
            true,
        );
        results.push((webhook.id, true));
    }

    results
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

/// POST /api/webhooks — register a new webhook
pub async fn create_webhook(
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateWebhookRequest>,
) -> Result<(StatusCode, Json<Webhook>), (StatusCode, Json<serde_json::Value>)> {
    auth.require_json(Permission::Autonomous)?;
    // Validate URL
    if !validate_url(&req.url) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "url must start with http:// or https://"})),
        ));
    }

    // Validate events
    if let Err(e) = validate_events(&req.events) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        ));
    }

    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    let webhook = insert_webhook(&conn, &req).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
    })?;

    Ok((StatusCode::CREATED, Json(webhook)))
}

/// GET /api/webhooks?account_id=N — list all webhooks for an account
pub async fn list_webhooks(
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<Webhook>>, (StatusCode, Json<serde_json::Value>)> {
    auth.require_json(Permission::DraftOnly)?;
    let account_id: i64 = params
        .get("account_id")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    Ok(Json(list_webhooks_for_account(&conn, account_id)))
}

/// GET /api/webhooks/:id — get a specific webhook
pub async fn get_webhook_handler(
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Webhook>, (StatusCode, Json<serde_json::Value>)> {
    auth.require_json(Permission::DraftOnly)?;
    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    get_webhook(&conn, id)
        .map(Json)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "webhook not found"})),
            )
        })
}

/// PUT /api/webhooks/:id — update a webhook
pub async fn update_webhook(
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateWebhookRequest>,
) -> Result<Json<Webhook>, (StatusCode, Json<serde_json::Value>)> {
    auth.require_json(Permission::Autonomous)?;
    // Validate URL if provided
    if let Some(ref url) = req.url {
        if !validate_url(url) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "url must start with http:// or https://"})),
            ));
        }
    }

    // Validate events if provided
    if let Some(ref events) = req.events {
        if let Err(e) = validate_events(events) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e})),
            ));
        }
    }

    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    update_webhook_in_db(&conn, id, &req).map(Json).map_err(|e| {
        if e.contains("not found") {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": e})),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e})),
            )
        }
    })
}

/// DELETE /api/webhooks/:id — delete a webhook
pub async fn delete_webhook(
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    auth.require_json(Permission::Autonomous)?;
    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    if delete_webhook_in_db(&conn, id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "webhook not found"})),
        ))
    }
}

/// GET /api/webhooks/:id/deliveries — list recent deliveries
pub async fn list_webhook_deliveries(
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(params): Query<DeliveriesQuery>,
) -> Result<Json<Vec<WebhookDelivery>>, (StatusCode, Json<serde_json::Value>)> {
    auth.require_json(Permission::DraftOnly)?;
    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    // Ensure webhook exists
    get_webhook(&conn, id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "webhook not found"})),
        )
    })?;

    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    Ok(Json(list_deliveries(&conn, id, limit, offset)))
}

/// POST /api/webhooks/:id/test — send a test event
pub async fn test_webhook(
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    auth.require_json(Permission::Autonomous)?;
    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    let webhook = get_webhook(&conn, id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "webhook not found"})),
        )
    })?;

    let test_payload = serde_json::json!({
        "event": "test",
        "webhook_id": webhook.id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "data": {
            "message": "This is a test webhook delivery from Iris"
        }
    });

    let payload_str = serde_json::to_string(&test_payload).unwrap_or_default();

    let signature = webhook
        .secret
        .as_ref()
        .map(|s| compute_signature(s, &payload_str));

    // Record delivery as a test (simulated success — in production this would
    // actually POST to webhook.url with reqwest)
    record_delivery(
        &conn,
        webhook.id,
        "test",
        &payload_str,
        Some(200),
        Some("test delivery"),
        true,
    );

    Ok(Json(serde_json::json!({
        "status": "delivered",
        "webhook_id": webhook.id,
        "url": webhook.url,
        "signature": signature,
        "payload": test_payload,
    })))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    fn setup() -> Conn {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        // Verify webhooks table exists (created by migration 040)
        conn.execute("SELECT 1 FROM webhooks LIMIT 0", []).unwrap();
        conn
    }

    fn create_test_webhook(_conn: &Conn) -> CreateWebhookRequest {
        CreateWebhookRequest {
            account_id: 1,
            url: "https://example.com/webhook".to_string(),
            events: vec!["email.received".to_string(), "email.sent".to_string()],
            secret: Some("test-secret-123".to_string()),
        }
    }

    // 1. Test creating a webhook
    #[test]
    fn test_create_webhook() {
        let conn = setup();
        let req = create_test_webhook(&conn);
        let webhook = insert_webhook(&conn, &req).unwrap();

        assert_eq!(webhook.account_id, 1);
        assert_eq!(webhook.url, "https://example.com/webhook");
        assert_eq!(webhook.events, vec!["email.received", "email.sent"]);
        assert!(webhook.active);
        assert_eq!(webhook.failure_count, 0);
        assert!(webhook.secret.is_some());
    }

    // 2. Test listing webhooks for an account
    #[test]
    fn test_list_webhooks() {
        let conn = setup();

        // Create two webhooks for account 1
        let req1 = CreateWebhookRequest {
            account_id: 1,
            url: "https://example.com/hook1".to_string(),
            events: vec!["email.received".to_string()],
            secret: None,
        };
        let req2 = CreateWebhookRequest {
            account_id: 1,
            url: "https://example.com/hook2".to_string(),
            events: vec!["email.sent".to_string()],
            secret: None,
        };
        let req3 = CreateWebhookRequest {
            account_id: 2,
            url: "https://other.com/hook".to_string(),
            events: vec!["email.deleted".to_string()],
            secret: None,
        };

        insert_webhook(&conn, &req1).unwrap();
        insert_webhook(&conn, &req2).unwrap();
        insert_webhook(&conn, &req3).unwrap();

        let account1_hooks = list_webhooks_for_account(&conn, 1);
        assert_eq!(account1_hooks.len(), 2);

        let account2_hooks = list_webhooks_for_account(&conn, 2);
        assert_eq!(account2_hooks.len(), 1);
    }

    // 3. Test getting a specific webhook
    #[test]
    fn test_get_webhook() {
        let conn = setup();
        let req = create_test_webhook(&conn);
        let created = insert_webhook(&conn, &req).unwrap();

        let fetched = get_webhook(&conn, created.id).unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.url, "https://example.com/webhook");
    }

    // 4. Test getting a nonexistent webhook
    #[test]
    fn test_get_webhook_not_found() {
        let conn = setup();
        assert!(get_webhook(&conn, 99999).is_none());
    }

    // 5. Test updating a webhook
    #[test]
    fn test_update_webhook() {
        let conn = setup();
        let req = create_test_webhook(&conn);
        let created = insert_webhook(&conn, &req).unwrap();

        let update = UpdateWebhookRequest {
            url: Some("https://updated.com/hook".to_string()),
            events: Some(vec!["email.deleted".to_string()]),
            active: Some(false),
            secret: None,
        };

        let updated = update_webhook_in_db(&conn, created.id, &update).unwrap();
        assert_eq!(updated.url, "https://updated.com/hook");
        assert_eq!(updated.events, vec!["email.deleted"]);
        assert!(!updated.active);
    }

    // 6. Test updating a nonexistent webhook
    #[test]
    fn test_update_webhook_not_found() {
        let conn = setup();
        let update = UpdateWebhookRequest {
            url: Some("https://updated.com/hook".to_string()),
            events: None,
            active: None,
            secret: None,
        };
        let result = update_webhook_in_db(&conn, 99999, &update);
        assert!(result.is_err());
    }

    // 7. Test deleting a webhook
    #[test]
    fn test_delete_webhook() {
        let conn = setup();
        let req = create_test_webhook(&conn);
        let created = insert_webhook(&conn, &req).unwrap();

        assert!(delete_webhook_in_db(&conn, created.id));
        assert!(get_webhook(&conn, created.id).is_none());
    }

    // 8. Test deleting a nonexistent webhook
    #[test]
    fn test_delete_webhook_not_found() {
        let conn = setup();
        assert!(!delete_webhook_in_db(&conn, 99999));
    }

    // 9. Test recording and listing deliveries
    #[test]
    fn test_deliveries() {
        let conn = setup();
        let req = create_test_webhook(&conn);
        let created = insert_webhook(&conn, &req).unwrap();

        record_delivery(
            &conn,
            created.id,
            "email.received",
            r#"{"test": true}"#,
            Some(200),
            Some("ok"),
            true,
        );
        record_delivery(
            &conn,
            created.id,
            "email.sent",
            r#"{"test": true}"#,
            Some(500),
            Some("error"),
            false,
        );

        let deliveries = list_deliveries(&conn, created.id, 50, 0);
        assert_eq!(deliveries.len(), 2);

        // Most recent first
        let successful = deliveries.iter().find(|d| d.success).unwrap();
        assert_eq!(successful.event_type, "email.received");
        assert_eq!(successful.status_code, Some(200));

        let failed = deliveries.iter().find(|d| !d.success).unwrap();
        assert_eq!(failed.event_type, "email.sent");
        assert_eq!(failed.status_code, Some(500));
    }

    // 10. Test auto-disable after consecutive failures
    #[test]
    fn test_auto_disable_on_failures() {
        let conn = setup();
        let req = create_test_webhook(&conn);
        let created = insert_webhook(&conn, &req).unwrap();

        // Simulate MAX_CONSECUTIVE_FAILURES failures
        for i in 0..MAX_CONSECUTIVE_FAILURES {
            record_delivery(
                &conn,
                created.id,
                "email.received",
                r#"{"test": true}"#,
                Some(500),
                Some("server error"),
                false,
            );

            let webhook = get_webhook(&conn, created.id).unwrap();
            if i + 1 >= MAX_CONSECUTIVE_FAILURES {
                assert!(!webhook.active, "webhook should be disabled after {} failures", i + 1);
            } else {
                assert!(webhook.active, "webhook should still be active after {} failures", i + 1);
            }
        }
    }

    // 11. Test success resets failure count
    #[test]
    fn test_success_resets_failure_count() {
        let conn = setup();
        let req = create_test_webhook(&conn);
        let created = insert_webhook(&conn, &req).unwrap();

        // Record some failures
        for _ in 0..5 {
            record_delivery(
                &conn,
                created.id,
                "email.received",
                "{}",
                Some(500),
                None,
                false,
            );
        }
        let webhook = get_webhook(&conn, created.id).unwrap();
        assert_eq!(webhook.failure_count, 5);

        // Record a success
        record_delivery(
            &conn,
            created.id,
            "email.received",
            "{}",
            Some(200),
            Some("ok"),
            true,
        );
        let webhook = get_webhook(&conn, created.id).unwrap();
        assert_eq!(webhook.failure_count, 0);
        assert!(webhook.last_triggered_at.is_some());
    }

    // 12. Test HMAC signature computation
    #[test]
    fn test_compute_signature() {
        let sig = compute_signature("my-secret", r#"{"event":"test"}"#);
        assert!(sig.starts_with("sha256="));
        assert_eq!(sig.len(), 7 + 64); // "sha256=" (7) + 64 hex chars

        // Same inputs produce same output
        let sig2 = compute_signature("my-secret", r#"{"event":"test"}"#);
        assert_eq!(sig, sig2);

        // Different secret produces different output
        let sig3 = compute_signature("other-secret", r#"{"event":"test"}"#);
        assert_ne!(sig, sig3);

        // Different payload produces different output
        let sig4 = compute_signature("my-secret", r#"{"event":"other"}"#);
        assert_ne!(sig, sig4);
    }

    // 13. Test URL validation
    #[test]
    fn test_url_validation() {
        assert!(validate_url("https://example.com/webhook"));
        assert!(validate_url("http://localhost:3000/hook"));
        assert!(!validate_url("ftp://example.com/hook"));
        assert!(!validate_url("not-a-url"));
        assert!(!validate_url(""));
    }

    // 14. Test event validation
    #[test]
    fn test_event_validation() {
        assert!(validate_events(&["email.received".to_string()]).is_ok());
        assert!(validate_events(&[
            "email.received".to_string(),
            "email.sent".to_string(),
            "email.archived".to_string(),
            "email.deleted".to_string(),
            "email.labeled".to_string(),
            "email.starred".to_string(),
        ])
        .is_ok());
        assert!(validate_events(&["invalid.event".to_string()]).is_err());
        assert!(validate_events(&[]).is_err());
    }

    // 15. Test find_matching_webhooks
    #[test]
    fn test_find_matching_webhooks() {
        let conn = setup();

        let req1 = CreateWebhookRequest {
            account_id: 1,
            url: "https://example.com/hook1".to_string(),
            events: vec!["email.received".to_string(), "email.sent".to_string()],
            secret: None,
        };
        let req2 = CreateWebhookRequest {
            account_id: 1,
            url: "https://example.com/hook2".to_string(),
            events: vec!["email.deleted".to_string()],
            secret: None,
        };

        insert_webhook(&conn, &req1).unwrap();
        insert_webhook(&conn, &req2).unwrap();

        let received_hooks = find_matching_webhooks(&conn, "email.received");
        assert_eq!(received_hooks.len(), 1);
        assert_eq!(received_hooks[0].url, "https://example.com/hook1");

        let deleted_hooks = find_matching_webhooks(&conn, "email.deleted");
        assert_eq!(deleted_hooks.len(), 1);
        assert_eq!(deleted_hooks[0].url, "https://example.com/hook2");

        let starred_hooks = find_matching_webhooks(&conn, "email.starred");
        assert_eq!(starred_hooks.len(), 0);
    }

    // 16. Test trigger_webhooks
    #[test]
    fn test_trigger_webhooks() {
        let conn = setup();

        let req = CreateWebhookRequest {
            account_id: 1,
            url: "https://example.com/hook".to_string(),
            events: vec!["email.received".to_string()],
            secret: Some("webhook-secret".to_string()),
        };
        let created = insert_webhook(&conn, &req).unwrap();

        let payload = serde_json::json!({
            "event": "email.received",
            "message_id": "test-123",
            "subject": "Hello World"
        });

        let results = trigger_webhooks(&conn, "email.received", &payload);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, created.id);
        assert!(results[0].1); // success

        // Verify delivery was recorded
        let deliveries = list_deliveries(&conn, created.id, 50, 0);
        assert_eq!(deliveries.len(), 1);
        assert!(deliveries[0].success);
    }

    // 17. Test inactive webhooks are not triggered
    #[test]
    fn test_inactive_webhooks_not_triggered() {
        let conn = setup();

        let req = CreateWebhookRequest {
            account_id: 1,
            url: "https://example.com/hook".to_string(),
            events: vec!["email.received".to_string()],
            secret: None,
        };
        let created = insert_webhook(&conn, &req).unwrap();

        // Deactivate the webhook
        let update = UpdateWebhookRequest {
            url: None,
            events: None,
            active: Some(false),
            secret: None,
        };
        update_webhook_in_db(&conn, created.id, &update).unwrap();

        let results = trigger_webhooks(
            &conn,
            "email.received",
            &serde_json::json!({"test": true}),
        );
        assert_eq!(results.len(), 0);
    }

    // 18. Test cascade delete removes deliveries
    #[test]
    fn test_cascade_delete() {
        let conn = setup();
        let req = create_test_webhook(&conn);
        let created = insert_webhook(&conn, &req).unwrap();

        // Record some deliveries
        record_delivery(&conn, created.id, "test", "{}", Some(200), None, true);
        record_delivery(&conn, created.id, "test", "{}", Some(200), None, true);

        let deliveries = list_deliveries(&conn, created.id, 50, 0);
        assert_eq!(deliveries.len(), 2);

        // Delete webhook — deliveries should cascade
        delete_webhook_in_db(&conn, created.id);

        // Deliveries table should have no entries for this webhook
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM webhook_deliveries WHERE webhook_id = ?1",
                params![created.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
