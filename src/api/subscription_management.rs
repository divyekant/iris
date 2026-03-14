use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Subscription {
    pub id: i64,
    pub account_id: String,
    pub sender_address: String,
    pub sender_name: Option<String>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub total_count: i64,
    pub read_count: i64,
    pub unsubscribe_url: Option<String>,
    pub status: String,
    pub frequency_days: Option<f64>,
    pub read_rate: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub status: Option<String>,
    pub sort_by: Option<String>,
    pub account_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub subscriptions: Vec<Subscription>,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct ScanResponse {
    pub discovered: i64,
    pub updated: i64,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionDetail {
    #[serde(flatten)]
    pub subscription: Subscription,
    pub recent_messages: Vec<RecentMessage>,
}

#[derive(Debug, Serialize)]
pub struct RecentMessage {
    pub id: String,
    pub subject: Option<String>,
    pub date: Option<i64>,
    pub is_read: bool,
}

#[derive(Debug, Deserialize)]
pub struct StatusUpdateRequest {
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct StatusUpdateResponse {
    pub updated: bool,
}

#[derive(Debug, Deserialize)]
pub struct BulkActionRequest {
    pub ids: Vec<i64>,
    pub action: String,
}

#[derive(Debug, Serialize)]
pub struct BulkActionResponse {
    pub updated: i64,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionStats {
    pub total: i64,
    pub active: i64,
    pub unsubscribed: i64,
    pub archived: i64,
    pub blocked: i64,
    pub avg_read_rate: f64,
    pub top_by_volume: Vec<VolumeSender>,
}

#[derive(Debug, Serialize)]
pub struct VolumeSender {
    pub sender_address: String,
    pub sender_name: Option<String>,
    pub total_count: i64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const VALID_STATUSES: &[&str] = &["active", "unsubscribed", "archived", "blocked"];
const VALID_SORT_BY: &[&str] = &["frequency", "count", "read_rate", "last_seen"];

fn compute_read_rate(read_count: i64, total_count: i64) -> f64 {
    if total_count == 0 {
        0.0
    } else {
        read_count as f64 / total_count as f64
    }
}

fn row_to_subscription(row: &rusqlite::Row) -> rusqlite::Result<Subscription> {
    let total_count: i64 = row.get("total_count")?;
    let read_count: i64 = row.get("read_count")?;
    Ok(Subscription {
        id: row.get("id")?,
        account_id: row.get("account_id")?,
        sender_address: row.get("sender_address")?,
        sender_name: row.get("sender_name")?,
        first_seen_at: row.get("first_seen_at")?,
        last_seen_at: row.get("last_seen_at")?,
        total_count,
        read_count,
        unsubscribe_url: row.get("unsubscribe_url")?,
        status: row.get("status")?,
        frequency_days: row.get("frequency_days")?,
        read_rate: compute_read_rate(read_count, total_count),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

// ---------------------------------------------------------------------------
// GET /api/subscriptions
// ---------------------------------------------------------------------------

pub async fn list_subscriptions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<ListResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Validate status filter
    if let Some(status) = &params.status {
        if !VALID_STATUSES.contains(&status.as_str()) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    // Validate sort_by
    let sort_by = params.sort_by.as_deref().unwrap_or("last_seen");
    if !VALID_SORT_BY.contains(&sort_by) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0).max(0);

    let order_clause = match sort_by {
        "frequency" => "frequency_days ASC NULLS LAST",
        "count" => "total_count DESC",
        "read_rate" => "CAST(read_count AS REAL) / MAX(total_count, 1) ASC",
        _ => "last_seen_at DESC",
    };

    // Build dynamic WHERE clause and params
    let mut conditions: Vec<String> = Vec::new();
    let mut dyn_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(status) = &params.status {
        dyn_params.push(Box::new(status.clone()));
        conditions.push(format!("status = ?{}", dyn_params.len()));
    }
    if let Some(account_id) = &params.account_id {
        dyn_params.push(Box::new(account_id.clone()));
        conditions.push(format!("account_id = ?{}", dyn_params.len()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Add limit and offset as params
    dyn_params.push(Box::new(limit));
    let limit_idx = dyn_params.len();
    dyn_params.push(Box::new(offset));
    let offset_idx = dyn_params.len();

    let query = format!(
        "SELECT * FROM subscriptions {where_clause} ORDER BY {order_clause} LIMIT ?{limit_idx} OFFSET ?{offset_idx}"
    );
    let count_query = format!("SELECT COUNT(*) FROM subscriptions {where_clause}");

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = dyn_params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&query).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let subscriptions: Vec<Subscription> = stmt
        .query_map(param_refs.as_slice(), row_to_subscription)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    // Count query uses only the filter params (not limit/offset)
    let count_params: Vec<&dyn rusqlite::types::ToSql> = param_refs[..param_refs.len() - 2].to_vec();
    let total: i64 = conn
        .query_row(&count_query, count_params.as_slice(), |row| row.get(0))
        .unwrap_or(0);

    Ok(Json(ListResponse {
        subscriptions,
        total,
    }))
}

// ---------------------------------------------------------------------------
// POST /api/subscriptions/scan
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ScanParams {
    pub account_id: Option<String>,
}

pub async fn scan_subscriptions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ScanParams>,
) -> Result<Json<ScanResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Find subscription-like senders: have List-Unsubscribe header OR ai_category is newsletter/promotion
    let base_query = "SELECT
                m.account_id,
                m.from_address AS sender_address,
                MAX(m.from_name) AS sender_name,
                MIN(datetime(m.date, 'unixepoch')) AS first_seen_at,
                MAX(datetime(m.date, 'unixepoch')) AS last_seen_at,
                COUNT(*) AS total_count,
                SUM(CASE WHEN m.is_read = 1 THEN 1 ELSE 0 END) AS read_count,
                MAX(m.list_unsubscribe) AS unsubscribe_url
             FROM messages m
             WHERE m.from_address IS NOT NULL
               AND (m.list_unsubscribe IS NOT NULL OR LOWER(m.ai_category) IN ('newsletters', 'promotions'))";

    let sender_query = if params.account_id.is_some() {
        format!("{base_query} AND m.account_id = ?1 GROUP BY m.account_id, m.from_address")
    } else {
        format!("{base_query} GROUP BY m.account_id, m.from_address")
    };

    struct SenderRow {
        account_id: String,
        sender_address: String,
        sender_name: Option<String>,
        first_seen_at: String,
        last_seen_at: String,
        total_count: i64,
        read_count: i64,
        unsubscribe_url: Option<String>,
    }

    fn parse_sender_row(row: &rusqlite::Row) -> rusqlite::Result<SenderRow> {
        Ok(SenderRow {
            account_id: row.get(0)?,
            sender_address: row.get(1)?,
            sender_name: row.get(2)?,
            first_seen_at: row.get(3)?,
            last_seen_at: row.get(4)?,
            total_count: row.get(5)?,
            read_count: row.get(6)?,
            unsubscribe_url: row.get(7)?,
        })
    }

    let mut stmt = conn.prepare(&sender_query).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows: Vec<SenderRow> = if let Some(account_id) = params.account_id {
        stmt.query_map(rusqlite::params![account_id], parse_sender_row)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map([], parse_sender_row)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    };

    let mut discovered: i64 = 0;
    let mut updated: i64 = 0;

    for row in &rows {
        // Compute frequency_days = (last_seen - first_seen) / (total_count - 1)
        let frequency_days: Option<f64> = if row.total_count > 1 {
            // Parse dates and compute difference in days
            let first = chrono::NaiveDateTime::parse_from_str(&row.first_seen_at, "%Y-%m-%d %H:%M:%S").ok();
            let last = chrono::NaiveDateTime::parse_from_str(&row.last_seen_at, "%Y-%m-%d %H:%M:%S").ok();
            match (first, last) {
                (Some(f), Some(l)) => {
                    let diff = l.signed_duration_since(f);
                    let days = diff.num_seconds() as f64 / 86400.0;
                    Some(days / (row.total_count - 1) as f64)
                }
                _ => None,
            }
        } else {
            None
        };

        // Check if subscription already exists
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM subscriptions WHERE account_id = ?1 AND sender_address = ?2",
                rusqlite::params![row.account_id, row.sender_address],
                |r| r.get(0),
            )
            .ok();

        if existing.is_some() {
            // Update existing
            conn.execute(
                "UPDATE subscriptions SET
                    sender_name = COALESCE(?1, sender_name),
                    first_seen_at = ?2,
                    last_seen_at = ?3,
                    total_count = ?4,
                    read_count = ?5,
                    unsubscribe_url = COALESCE(?6, unsubscribe_url),
                    frequency_days = ?7,
                    updated_at = datetime('now')
                 WHERE account_id = ?8 AND sender_address = ?9",
                rusqlite::params![
                    row.sender_name,
                    row.first_seen_at,
                    row.last_seen_at,
                    row.total_count,
                    row.read_count,
                    row.unsubscribe_url,
                    frequency_days,
                    row.account_id,
                    row.sender_address,
                ],
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            updated += 1;
        } else {
            // Insert new
            conn.execute(
                "INSERT INTO subscriptions (
                    account_id, sender_address, sender_name,
                    first_seen_at, last_seen_at, total_count, read_count,
                    unsubscribe_url, frequency_days, status
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active')",
                rusqlite::params![
                    row.account_id,
                    row.sender_address,
                    row.sender_name,
                    row.first_seen_at,
                    row.last_seen_at,
                    row.total_count,
                    row.read_count,
                    row.unsubscribe_url,
                    frequency_days,
                ],
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            discovered += 1;
        }
    }

    Ok(Json(ScanResponse {
        discovered,
        updated,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/subscriptions/:id
// ---------------------------------------------------------------------------

pub async fn get_subscription(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<SubscriptionDetail>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let subscription = conn
        .query_row(
            "SELECT * FROM subscriptions WHERE id = ?1",
            rusqlite::params![id],
            row_to_subscription,
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    // Get recent messages from this sender
    let mut stmt = conn
        .prepare(
            "SELECT id, subject, date, is_read FROM messages
             WHERE account_id = ?1 AND from_address = ?2
             ORDER BY date DESC LIMIT 10",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let recent_messages: Vec<RecentMessage> = stmt
        .query_map(
            rusqlite::params![subscription.account_id, subscription.sender_address],
            |row| {
                Ok(RecentMessage {
                    id: row.get(0)?,
                    subject: row.get(1)?,
                    date: row.get(2)?,
                    is_read: row.get::<_, i64>(3).map(|v| v != 0)?,
                })
            },
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(SubscriptionDetail {
        subscription,
        recent_messages,
    }))
}

// ---------------------------------------------------------------------------
// PUT /api/subscriptions/:id/status
// ---------------------------------------------------------------------------

pub async fn update_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<StatusUpdateRequest>,
) -> Result<Json<StatusUpdateResponse>, StatusCode> {
    if !VALID_STATUSES.contains(&req.status.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows = conn
        .execute(
            "UPDATE subscriptions SET status = ?1, updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![req.status, id],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if rows == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(StatusUpdateResponse { updated: true }))
}

// ---------------------------------------------------------------------------
// POST /api/subscriptions/bulk-action
// ---------------------------------------------------------------------------

pub async fn bulk_action(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BulkActionRequest>,
) -> Result<Json<BulkActionResponse>, StatusCode> {
    if req.ids.is_empty() || req.ids.len() > 1000 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let new_status = match req.action.as_str() {
        "archive" => "archived",
        "block" => "blocked",
        "unsubscribe" => "unsubscribed",
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build parameterized IN clause
    let placeholders: Vec<String> = (1..=req.ids.len()).map(|i| format!("?{}", i + 1)).collect();
    let in_clause = placeholders.join(", ");

    let query = format!(
        "UPDATE subscriptions SET status = ?1, updated_at = datetime('now') WHERE id IN ({in_clause})"
    );

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(new_status.to_string()));
    for id in &req.ids {
        params.push(Box::new(*id));
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let updated = conn
        .execute(&query, param_refs.as_slice())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(BulkActionResponse {
        updated: updated as i64,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/subscriptions/stats
// ---------------------------------------------------------------------------

pub async fn subscription_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SubscriptionStats>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM subscriptions", [], |row| row.get(0))
        .unwrap_or(0);

    let active: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM subscriptions WHERE status = 'active'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let unsubscribed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM subscriptions WHERE status = 'unsubscribed'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let archived: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM subscriptions WHERE status = 'archived'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let blocked: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM subscriptions WHERE status = 'blocked'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Average read rate across all subscriptions
    let avg_read_rate: f64 = conn
        .query_row(
            "SELECT COALESCE(AVG(CAST(read_count AS REAL) / MAX(total_count, 1)), 0.0)
             FROM subscriptions WHERE total_count > 0",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    // Top 10 by volume
    let mut stmt = conn
        .prepare(
            "SELECT sender_address, sender_name, total_count
             FROM subscriptions
             ORDER BY total_count DESC
             LIMIT 10",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let top_by_volume: Vec<VolumeSender> = stmt
        .query_map([], |row| {
            Ok(VolumeSender {
                sender_address: row.get(0)?,
                sender_name: row.get(1)?,
                total_count: row.get(2)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(SubscriptionStats {
        total,
        active,
        unsubscribed,
        archived,
        blocked,
        avg_read_rate,
        top_by_volume,
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::middleware;
    use axum::routing::{get, post, put};
    use axum::Router;
    use http_body_util::BodyExt;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::sync::Arc;
    use tower::ServiceExt;

    use crate::ai::memories::MemoriesClient;
    use crate::ai::provider::ProviderPool;
    use crate::api::session_auth::session_auth_middleware;
    use crate::config::Config;
    use crate::db::migrations;
    use crate::ws::hub::WsHub;
    use crate::AppState;

    const TEST_TOKEN: &str = "test-session-token-sub";

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

    /// Build a test-only router with subscription routes behind session auth.
    fn build_test_app(state: Arc<AppState>) -> Router {
        let protected = Router::new()
            .route("/subscriptions", get(list_subscriptions))
            .route("/subscriptions/scan", post(scan_subscriptions))
            .route("/subscriptions/stats", get(subscription_stats))
            .route("/subscriptions/{id}", get(get_subscription))
            .route("/subscriptions/{id}/status", put(update_status))
            .route("/subscriptions/bulk-action", post(bulk_action))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                session_auth_middleware,
            ));

        Router::new()
            .nest("/api", protected)
            .with_state(state)
    }

    async fn body_to_json(body: Body) -> serde_json::Value {
        let bytes = body.collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn seed_account_and_messages(state: &Arc<AppState>) {
        let conn = state.db.get().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES (1, 'imap', 'user@example.com')",
            [],
        )
        .unwrap();

        // Newsletter sender with list_unsubscribe
        for i in 1..=5 {
            let date = 1700000000 + (i * 86400); // Each day apart
            conn.execute(
                "INSERT INTO messages (id, account_id, folder, from_address, from_name, subject, body_text, date, is_read, list_unsubscribe, ai_category)
                 VALUES (?1, 1, 'INBOX', 'newsletter@news.com', 'Daily News', ?2, 'Content', ?3, ?4, 'https://unsub.news.com/123', 'Newsletters')",
                rusqlite::params![
                    format!("msg-news-{i}"),
                    format!("Newsletter #{i}"),
                    date,
                    if i <= 3 { 1 } else { 0 }, // 3 read, 2 unread
                ],
            )
            .unwrap();
        }

        // Promotion sender without list_unsubscribe but ai_category = Promotions
        for i in 1..=3 {
            let date = 1700000000 + (i * 172800); // Every 2 days
            conn.execute(
                "INSERT INTO messages (id, account_id, folder, from_address, from_name, subject, body_text, date, is_read, ai_category)
                 VALUES (?1, 1, 'INBOX', 'deals@shop.com', 'Shop Deals', ?2, 'Deal content', ?3, ?4, 'Promotions')",
                rusqlite::params![
                    format!("msg-shop-{i}"),
                    format!("Deal #{i}"),
                    date,
                    if i == 1 { 1 } else { 0 }, // 1 read, 2 unread
                ],
            )
            .unwrap();
        }

        // Regular sender (should NOT be picked up by scan)
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, from_name, subject, body_text, date, is_read, ai_category)
             VALUES ('msg-regular-1', 1, 'INBOX', 'friend@gmail.com', 'Friend', 'Hey there', 'Hi!', 1700000000, 1, 'Primary')",
            [],
        )
        .unwrap();
    }

    /// Helper: run a scan on the test app, returning the app state for further use.
    async fn scan(state: &Arc<AppState>) {
        let app = build_test_app(state.clone());
        let res = app
            .oneshot(
                Request::post("/api/subscriptions/scan")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    // -----------------------------------------------------------------------
    // 1. Scan discovers subscriptions
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_scan_discovers_subscriptions() {
        let state = create_test_state();
        seed_account_and_messages(&state);
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::post("/api/subscriptions/scan")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        assert_eq!(json["discovered"], 2); // newsletter + promotions
        assert_eq!(json["updated"], 0);
    }

    // -----------------------------------------------------------------------
    // 2. Scan updates existing subscriptions
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_scan_updates_existing() {
        let state = create_test_state();
        seed_account_and_messages(&state);

        // First scan
        scan(&state).await;

        // Second scan should update, not discover
        let app = build_test_app(state);
        let res = app
            .oneshot(
                Request::post("/api/subscriptions/scan")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        assert_eq!(json["discovered"], 0);
        assert_eq!(json["updated"], 2);
    }

    // -----------------------------------------------------------------------
    // 3. List all subscriptions
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_list_subscriptions() {
        let state = create_test_state();
        seed_account_and_messages(&state);
        scan(&state).await;

        let app = build_test_app(state);
        let res = app
            .oneshot(
                Request::get("/api/subscriptions")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        assert_eq!(json["total"], 2);
        assert_eq!(json["subscriptions"].as_array().unwrap().len(), 2);
    }

    // -----------------------------------------------------------------------
    // 4. List with status filter
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_list_with_status_filter() {
        let state = create_test_state();
        seed_account_and_messages(&state);
        scan(&state).await;

        // All are active
        let app = build_test_app(state.clone());
        let res = app
            .oneshot(
                Request::get("/api/subscriptions?status=active")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        assert_eq!(json["total"], 2);

        // None are archived
        let app = build_test_app(state);
        let res = app
            .oneshot(
                Request::get("/api/subscriptions?status=archived")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        assert_eq!(json["total"], 0);
    }

    // -----------------------------------------------------------------------
    // 5. List with invalid status returns 400
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_list_invalid_status() {
        let state = create_test_state();
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::get("/api/subscriptions?status=bogus")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    // -----------------------------------------------------------------------
    // 6. List with sort_by=count
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_list_sorted_by_count() {
        let state = create_test_state();
        seed_account_and_messages(&state);
        scan(&state).await;

        let app = build_test_app(state);
        let res = app
            .oneshot(
                Request::get("/api/subscriptions?sort_by=count")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        let subs = json["subscriptions"].as_array().unwrap();
        // Newsletter has 5, shop has 3 — sorted DESC by count
        assert!(subs[0]["total_count"].as_i64().unwrap() >= subs[1]["total_count"].as_i64().unwrap());
    }

    // -----------------------------------------------------------------------
    // 7. Get subscription detail
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_subscription_detail() {
        let state = create_test_state();
        seed_account_and_messages(&state);
        scan(&state).await;

        let app = build_test_app(state);
        let res = app
            .oneshot(
                Request::get("/api/subscriptions/1")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        assert!(json["sender_address"].as_str().is_some());
        assert!(json["recent_messages"].as_array().is_some());
    }

    // -----------------------------------------------------------------------
    // 8. Get subscription not found
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_subscription_not_found() {
        let state = create_test_state();
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::get("/api/subscriptions/9999")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    // -----------------------------------------------------------------------
    // 9. Update subscription status
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_update_subscription_status() {
        let state = create_test_state();
        seed_account_and_messages(&state);
        scan(&state).await;

        let body = serde_json::json!({ "status": "unsubscribed" });
        let app = build_test_app(state);
        let res = app
            .oneshot(
                Request::put("/api/subscriptions/1/status")
                    .header("x-session-token", TEST_TOKEN)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        assert_eq!(json["updated"], true);
    }

    // -----------------------------------------------------------------------
    // 10. Update status with invalid value returns 400
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_update_status_invalid() {
        let state = create_test_state();
        seed_account_and_messages(&state);
        scan(&state).await;

        let body = serde_json::json!({ "status": "deleted" });
        let app = build_test_app(state);
        let res = app
            .oneshot(
                Request::put("/api/subscriptions/1/status")
                    .header("x-session-token", TEST_TOKEN)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    // -----------------------------------------------------------------------
    // 11. Update status for non-existent subscription returns 404
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_update_status_not_found() {
        let state = create_test_state();
        let body = serde_json::json!({ "status": "archived" });
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::put("/api/subscriptions/9999/status")
                    .header("x-session-token", TEST_TOKEN)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    // -----------------------------------------------------------------------
    // 12. Bulk action
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_bulk_action() {
        let state = create_test_state();
        seed_account_and_messages(&state);
        scan(&state).await;

        let body = serde_json::json!({ "ids": [1, 2], "action": "archive" });
        let app = build_test_app(state.clone());
        let res = app
            .oneshot(
                Request::post("/api/subscriptions/bulk-action")
                    .header("x-session-token", TEST_TOKEN)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        assert_eq!(json["updated"], 2);

        // Verify they are now archived
        let app = build_test_app(state);
        let res = app
            .oneshot(
                Request::get("/api/subscriptions?status=archived")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = body_to_json(res.into_body()).await;
        assert_eq!(json["total"], 2);
    }

    // -----------------------------------------------------------------------
    // 13. Bulk action with invalid action returns 400
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_bulk_action_invalid() {
        let state = create_test_state();
        let body = serde_json::json!({ "ids": [1], "action": "delete" });
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::post("/api/subscriptions/bulk-action")
                    .header("x-session-token", TEST_TOKEN)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    // -----------------------------------------------------------------------
    // 14. Bulk action with empty ids returns 400
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_bulk_action_empty_ids() {
        let state = create_test_state();
        let body = serde_json::json!({ "ids": [], "action": "archive" });
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::post("/api/subscriptions/bulk-action")
                    .header("x-session-token", TEST_TOKEN)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    // -----------------------------------------------------------------------
    // 15. Stats endpoint
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_subscription_stats() {
        let state = create_test_state();
        seed_account_and_messages(&state);
        scan(&state).await;

        let app = build_test_app(state);
        let res = app
            .oneshot(
                Request::get("/api/subscriptions/stats")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        assert_eq!(json["total"], 2);
        assert_eq!(json["active"], 2);
        assert_eq!(json["unsubscribed"], 0);
        assert_eq!(json["archived"], 0);
        assert_eq!(json["blocked"], 0);
        assert!(json["avg_read_rate"].as_f64().unwrap() > 0.0);
        assert!(json["top_by_volume"].as_array().unwrap().len() <= 10);
    }

    // -----------------------------------------------------------------------
    // 16. Stats on empty DB
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_subscription_stats_empty() {
        let state = create_test_state();
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::get("/api/subscriptions/stats")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        assert_eq!(json["total"], 0);
        assert_eq!(json["active"], 0);
        assert_eq!(json["avg_read_rate"], 0.0);
    }

    // -----------------------------------------------------------------------
    // 17. Auth required — no token returns 401
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_auth_required_list() {
        let state = create_test_state();
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::get("/api/subscriptions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_required_scan() {
        let state = create_test_state();
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::post("/api/subscriptions/scan")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_required_stats() {
        let state = create_test_state();
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::get("/api/subscriptions/stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    // -----------------------------------------------------------------------
    // 20. Scan with account_id filter
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_scan_with_account_filter() {
        let state = create_test_state();
        seed_account_and_messages(&state);

        // Add second account with no subscription-like messages
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO accounts (id, provider, email) VALUES (2, 'imap', 'other@example.com')",
                [],
            )
            .unwrap();
        }

        let app = build_test_app(state);
        let res = app
            .oneshot(
                Request::post("/api/subscriptions/scan?account_id=2")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        assert_eq!(json["discovered"], 0);
    }

    // -----------------------------------------------------------------------
    // 21. Read rate computation
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_read_rate_computation() {
        let state = create_test_state();
        seed_account_and_messages(&state);
        scan(&state).await;

        let app = build_test_app(state);
        let res = app
            .oneshot(
                Request::get("/api/subscriptions?sort_by=count")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let json = body_to_json(res.into_body()).await;
        let subs = json["subscriptions"].as_array().unwrap();

        // Newsletter: 3/5 = 0.6
        let news = subs.iter().find(|s| s["sender_address"] == "newsletter@news.com").unwrap();
        let rate = news["read_rate"].as_f64().unwrap();
        assert!((rate - 0.6).abs() < 0.01);

        // Shop: 1/3 ≈ 0.333
        let shop = subs.iter().find(|s| s["sender_address"] == "deals@shop.com").unwrap();
        let rate = shop["read_rate"].as_f64().unwrap();
        assert!((rate - 0.333).abs() < 0.01);
    }

    // -----------------------------------------------------------------------
    // 22. Frequency days computation
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_frequency_days_computation() {
        let state = create_test_state();
        seed_account_and_messages(&state);
        scan(&state).await;

        let app = build_test_app(state);
        let res = app
            .oneshot(
                Request::get("/api/subscriptions")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = body_to_json(res.into_body()).await;
        let subs = json["subscriptions"].as_array().unwrap();

        // Newsletter: 5 messages, 1 day apart -> frequency = 4 days / 4 = 1.0
        let news = subs.iter().find(|s| s["sender_address"] == "newsletter@news.com").unwrap();
        let freq = news["frequency_days"].as_f64().unwrap();
        assert!((freq - 1.0).abs() < 0.01);

        // Shop: 3 messages, 2 days apart -> (4 days) / 2 = 2.0
        let shop = subs.iter().find(|s| s["sender_address"] == "deals@shop.com").unwrap();
        let freq = shop["frequency_days"].as_f64().unwrap();
        assert!((freq - 2.0).abs() < 0.01);
    }

}
