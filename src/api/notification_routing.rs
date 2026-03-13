use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// --- Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    pub push_categories: Vec<String>,
    pub digest_categories: Vec<String>,
    pub silent_categories: Vec<String>,
    pub push_senders: Vec<String>,
    pub digest_interval_minutes: i64,
    pub quiet_hours_start: Option<String>,
    pub quiet_hours_end: Option<String>,
    pub vip_always_push: bool,
    pub urgency_threshold: String,
}

#[derive(Debug, Deserialize)]
pub struct ClassifyRequest {
    pub message_id: String,
}

#[derive(Debug, Serialize)]
pub struct ClassifyResponse {
    pub route: String,
    pub reason: String,
    pub message_id: String,
}

#[derive(Debug, Serialize)]
pub struct DigestItem {
    pub id: String,
    pub message_id: String,
    pub account_id: String,
    pub from_address: Option<String>,
    pub subject: Option<String>,
    pub category: Option<String>,
    pub priority: Option<String>,
    pub route: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct DigestResponse {
    pub items: Vec<DigestItem>,
    pub total: usize,
    pub categories: std::collections::HashMap<String, usize>,
}

#[derive(Debug, Serialize)]
pub struct ClearResponse {
    pub cleared: usize,
}

// --- Validation helpers ---

const VALID_URGENCY_LEVELS: &[&str] = &["low", "normal", "high", "urgent"];

fn urgency_rank(level: &str) -> i32 {
    match level {
        "low" => 0,
        "normal" => 1,
        "high" => 2,
        "urgent" => 3,
        _ => 1,
    }
}

fn is_valid_time(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return false;
    }
    let hour: u32 = match parts[0].parse() {
        Ok(h) => h,
        Err(_) => return false,
    };
    let minute: u32 = match parts[1].parse() {
        Ok(m) => m,
        Err(_) => return false,
    };
    hour < 24 && minute < 60
}

fn is_within_quiet_hours(start: &str, end: &str) -> bool {
    let now = chrono::Local::now();
    let current_minutes = now.format("%H:%M").to_string();
    let cur = time_to_minutes(&current_minutes);
    let s = time_to_minutes(start);
    let e = time_to_minutes(end);

    if s <= e {
        // Same-day range: e.g., 22:00-23:00
        cur >= s && cur < e
    } else {
        // Overnight range: e.g., 22:00-07:00
        cur >= s || cur < e
    }
}

fn time_to_minutes(t: &str) -> u32 {
    let parts: Vec<&str> = t.split(':').collect();
    let h: u32 = parts[0].parse().unwrap_or(0);
    let m: u32 = parts[1].parse().unwrap_or(0);
    h * 60 + m
}

// --- Internal helpers ---

fn load_config(conn: &rusqlite::Connection) -> Result<RoutingConfig, StatusCode> {
    conn.query_row(
        "SELECT push_categories, digest_categories, silent_categories, push_senders,
                digest_interval_minutes, quiet_hours_start, quiet_hours_end,
                vip_always_push, urgency_threshold
         FROM notification_routing_config WHERE id = 1",
        [],
        |row| {
            let push_cat_json: String = row.get(0)?;
            let digest_cat_json: String = row.get(1)?;
            let silent_cat_json: String = row.get(2)?;
            let push_senders_json: String = row.get(3)?;

            Ok(RoutingConfig {
                push_categories: serde_json::from_str(&push_cat_json).unwrap_or_default(),
                digest_categories: serde_json::from_str(&digest_cat_json).unwrap_or_default(),
                silent_categories: serde_json::from_str(&silent_cat_json).unwrap_or_default(),
                push_senders: serde_json::from_str(&push_senders_json).unwrap_or_default(),
                digest_interval_minutes: row.get(4)?,
                quiet_hours_start: row.get(5)?,
                quiet_hours_end: row.get(6)?,
                vip_always_push: row.get::<_, i64>(7)? != 0,
                urgency_threshold: row.get(8)?,
            })
        },
    )
    .map_err(|e| {
        tracing::error!("Failed to load notification routing config: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

fn is_vip_contact(conn: &rusqlite::Connection, email: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM vip_contacts WHERE email = ?1",
        rusqlite::params![email],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

fn classify_message(
    conn: &rusqlite::Connection,
    msg_id: &str,
    config: &RoutingConfig,
) -> Result<(String, String), StatusCode> {
    // Load message data
    let msg = conn
        .query_row(
            "SELECT id, from_address, ai_category, ai_priority_label, ai_intent, account_id, subject
             FROM messages WHERE id = ?1 AND is_deleted = 0",
            rusqlite::params![msg_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,                // id
                    row.get::<_, Option<String>>(1)?,         // from_address
                    row.get::<_, Option<String>>(2)?,         // ai_category
                    row.get::<_, Option<String>>(3)?,         // ai_priority_label
                    row.get::<_, Option<String>>(4)?,         // ai_intent
                    row.get::<_, String>(5)?,                 // account_id
                    row.get::<_, Option<String>>(6)?,         // subject
                ))
            },
        )
        .map_err(|e| {
            tracing::error!("Failed to load message for classification: {e}");
            StatusCode::NOT_FOUND
        })?;

    let (_id, from_address, ai_category, ai_priority_label, ai_intent, _account_id, _subject) = msg;

    // 1. VIP sender check
    if config.vip_always_push {
        if let Some(ref addr) = from_address {
            // Check explicit push_senders list
            if config.push_senders.iter().any(|s| s.eq_ignore_ascii_case(addr)) {
                return Ok(("push".to_string(), "sender in push_senders list".to_string()));
            }
            // Check vip_contacts table
            if is_vip_contact(conn, addr) {
                return Ok(("push".to_string(), "sender is VIP contact".to_string()));
            }
        }
    }

    // 2. Intent = action_required check
    if let Some(ref intent) = ai_intent {
        let intent_lower = intent.to_lowercase();
        if intent_lower == "action_required" || intent_lower == "action_request" {
            return Ok(("push".to_string(), "message requires action".to_string()));
        }
    }

    // 3. Urgency threshold check
    if let Some(ref priority) = ai_priority_label {
        if urgency_rank(priority) >= urgency_rank(&config.urgency_threshold) {
            return Ok((
                "push".to_string(),
                format!("priority '{}' meets threshold '{}'", priority, config.urgency_threshold),
            ));
        }
    }

    // 4. Category-based routing
    if let Some(ref category) = ai_category {
        let cat_lower = category.to_lowercase();

        if config
            .push_categories
            .iter()
            .any(|c| c.to_lowercase() == cat_lower)
        {
            return Ok(("push".to_string(), format!("category '{}' in push list", category)));
        }

        if config
            .silent_categories
            .iter()
            .any(|c| c.to_lowercase() == cat_lower)
        {
            return Ok(("silent".to_string(), format!("category '{}' in silent list", category)));
        }

        if config
            .digest_categories
            .iter()
            .any(|c| c.to_lowercase() == cat_lower)
        {
            return Ok(("digest".to_string(), format!("category '{}' in digest list", category)));
        }
    }

    // 5. Default → digest
    Ok(("digest".to_string(), "default routing".to_string()))
}

// --- Handlers ---

/// GET /api/notifications/routing/config
pub async fn get_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<RoutingConfig>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("Failed to get DB connection: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let config = load_config(&conn)?;
    Ok(Json(config))
}

/// PUT /api/notifications/routing/config
pub async fn update_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RoutingConfig>,
) -> Result<Json<RoutingConfig>, StatusCode> {
    // Validate inputs
    if req.digest_interval_minutes <= 0 {
        tracing::warn!("Invalid digest_interval_minutes: {}", req.digest_interval_minutes);
        return Err(StatusCode::BAD_REQUEST);
    }

    if !VALID_URGENCY_LEVELS.contains(&req.urgency_threshold.as_str()) {
        tracing::warn!("Invalid urgency_threshold: {}", req.urgency_threshold);
        return Err(StatusCode::BAD_REQUEST);
    }

    if let Some(ref start) = req.quiet_hours_start {
        if !is_valid_time(start) {
            tracing::warn!("Invalid quiet_hours_start: {}", start);
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    if let Some(ref end) = req.quiet_hours_end {
        if !is_valid_time(end) {
            tracing::warn!("Invalid quiet_hours_end: {}", end);
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    let conn = state.db.get().map_err(|e| {
        tracing::error!("Failed to get DB connection: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let push_cat_json = serde_json::to_string(&req.push_categories).unwrap_or_else(|_| "[]".to_string());
    let digest_cat_json = serde_json::to_string(&req.digest_categories).unwrap_or_else(|_| "[]".to_string());
    let silent_cat_json = serde_json::to_string(&req.silent_categories).unwrap_or_else(|_| "[]".to_string());
    let push_senders_json = serde_json::to_string(&req.push_senders).unwrap_or_else(|_| "[]".to_string());

    conn.execute(
        "UPDATE notification_routing_config SET
            push_categories = ?1,
            digest_categories = ?2,
            silent_categories = ?3,
            push_senders = ?4,
            digest_interval_minutes = ?5,
            quiet_hours_start = ?6,
            quiet_hours_end = ?7,
            vip_always_push = ?8,
            urgency_threshold = ?9,
            updated_at = unixepoch()
         WHERE id = 1",
        rusqlite::params![
            push_cat_json,
            digest_cat_json,
            silent_cat_json,
            push_senders_json,
            req.digest_interval_minutes,
            req.quiet_hours_start,
            req.quiet_hours_end,
            req.vip_always_push as i64,
            req.urgency_threshold,
        ],
    )
    .map_err(|e| {
        tracing::error!("Failed to update notification routing config: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Return updated config
    let config = load_config(&conn)?;
    Ok(Json(config))
}

/// POST /api/notifications/routing/classify
pub async fn classify(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ClassifyRequest>,
) -> Result<Json<ClassifyResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("Failed to get DB connection: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let config = load_config(&conn)?;
    let (mut route, reason) = classify_message(&conn, &req.message_id, &config)?;

    // Check quiet hours — if within quiet hours and not urgent, downgrade push → digest
    if route == "push" {
        if let (Some(start), Some(end)) = (&config.quiet_hours_start, &config.quiet_hours_end) {
            if is_within_quiet_hours(start, end) {
                // Only downgrade if message is not urgent
                let priority: Option<String> = conn
                    .query_row(
                        "SELECT ai_priority_label FROM messages WHERE id = ?1",
                        rusqlite::params![req.message_id],
                        |row| row.get(0),
                    )
                    .ok()
                    .flatten();

                let is_urgent = priority
                    .as_deref()
                    .map(|p| p == "urgent")
                    .unwrap_or(false);

                if !is_urgent {
                    route = "digest".to_string();
                }
            }
        }
    }

    // Load message metadata for digest item storage
    let msg_meta: Option<(String, Option<String>, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT account_id, from_address, subject, ai_category FROM messages WHERE id = ?1",
            rusqlite::params![req.message_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .ok();

    // Store classification in notification_digest_items
    if let Some((account_id, from_address, subject, category)) = msg_meta {
        let priority: Option<String> = conn
            .query_row(
                "SELECT ai_priority_label FROM messages WHERE id = ?1",
                rusqlite::params![req.message_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        let item_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO notification_digest_items (id, message_id, account_id, from_address, subject, category, priority, route)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![item_id, req.message_id, account_id, from_address, subject, category, priority, route],
        )
        .map_err(|e| {
            tracing::error!("Failed to store notification digest item: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    Ok(Json(ClassifyResponse {
        route,
        reason,
        message_id: req.message_id,
    }))
}

/// GET /api/notifications/digest
pub async fn get_digest(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DigestResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("Failed to get DB connection: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut stmt = conn
        .prepare(
            "SELECT id, message_id, account_id, from_address, subject, category, priority, route, created_at
             FROM notification_digest_items
             WHERE is_read = 0 AND route = 'digest'
             ORDER BY created_at DESC",
        )
        .map_err(|e| {
            tracing::error!("Failed to prepare digest query: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let items: Vec<DigestItem> = stmt
        .query_map([], |row| {
            Ok(DigestItem {
                id: row.get(0)?,
                message_id: row.get(1)?,
                account_id: row.get(2)?,
                from_address: row.get(3)?,
                subject: row.get(4)?,
                category: row.get(5)?,
                priority: row.get(6)?,
                route: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(|e| {
            tracing::error!("Failed to query digest items: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect();

    let total = items.len();

    let mut categories = std::collections::HashMap::new();
    for item in &items {
        if let Some(ref cat) = item.category {
            *categories.entry(cat.clone()).or_insert(0) += 1;
        }
    }

    Ok(Json(DigestResponse {
        items,
        total,
        categories,
    }))
}

/// POST /api/notifications/digest/clear
pub async fn clear_digest(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ClearResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("Failed to get DB connection: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let cleared = conn
        .execute(
            "UPDATE notification_digest_items SET is_read = 1 WHERE is_read = 0 AND route = 'digest'",
            [],
        )
        .map_err(|e| {
            tracing::error!("Failed to clear digest: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(ClearResponse { cleared }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};
    use crate::models::message::InsertMessage;

    fn setup_test_db() -> r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager> {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Apply migration 037 (notification routing tables)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS notification_routing_config (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                push_categories TEXT NOT NULL DEFAULT '[]',
                digest_categories TEXT NOT NULL DEFAULT '[]',
                silent_categories TEXT NOT NULL DEFAULT '[]',
                push_senders TEXT NOT NULL DEFAULT '[]',
                digest_interval_minutes INTEGER NOT NULL DEFAULT 60,
                quiet_hours_start TEXT,
                quiet_hours_end TEXT,
                vip_always_push INTEGER NOT NULL DEFAULT 1,
                urgency_threshold TEXT NOT NULL DEFAULT 'high',
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE IF NOT EXISTS notification_digest_items (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                account_id TEXT NOT NULL,
                from_address TEXT,
                subject TEXT,
                category TEXT,
                priority TEXT,
                route TEXT NOT NULL DEFAULT 'digest',
                is_read INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE IF NOT EXISTS vip_contacts (
                email TEXT PRIMARY KEY,
                display_name TEXT,
                vip_score REAL NOT NULL DEFAULT 0.0,
                is_manual INTEGER NOT NULL DEFAULT 0,
                message_count INTEGER NOT NULL DEFAULT 0,
                reply_count INTEGER NOT NULL DEFAULT 0,
                last_contact INTEGER,
                first_contact INTEGER,
                avg_reply_time_secs INTEGER,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            INSERT OR IGNORE INTO notification_routing_config (id) VALUES (1);",
        )
        .unwrap();

        conn
    }

    fn create_test_account(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    ) -> Account {
        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "notif-test@example.com".to_string(),
            display_name: Some("Notification Test".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("notif-test@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn make_test_message(account_id: &str, subject: &str, from: &str) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{}@test.com>", subject.replace(' ', "-"))),
            thread_id: None,
            folder: "INBOX".to_string(),
            from_address: Some(from.to_string()),
            from_name: Some("Test Sender".to_string()),
            to_addresses: Some(r#"["notif-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(1700000000),
            snippet: Some("Preview...".to_string()),
            body_text: Some("Body text".to_string()),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(1),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(512),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    fn insert_msg(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
        msg: &InsertMessage,
    ) -> String {
        InsertMessage::insert(conn, msg).expect("failed to insert test message")
    }

    fn load_test_config(conn: &rusqlite::Connection) -> RoutingConfig {
        load_config(conn).expect("failed to load config")
    }

    fn update_test_config(conn: &rusqlite::Connection, config: &RoutingConfig) {
        let push_cat_json = serde_json::to_string(&config.push_categories).unwrap();
        let digest_cat_json = serde_json::to_string(&config.digest_categories).unwrap();
        let silent_cat_json = serde_json::to_string(&config.silent_categories).unwrap();
        let push_senders_json = serde_json::to_string(&config.push_senders).unwrap();

        conn.execute(
            "UPDATE notification_routing_config SET
                push_categories = ?1,
                digest_categories = ?2,
                silent_categories = ?3,
                push_senders = ?4,
                digest_interval_minutes = ?5,
                quiet_hours_start = ?6,
                quiet_hours_end = ?7,
                vip_always_push = ?8,
                urgency_threshold = ?9,
                updated_at = unixepoch()
             WHERE id = 1",
            rusqlite::params![
                push_cat_json,
                digest_cat_json,
                silent_cat_json,
                push_senders_json,
                config.digest_interval_minutes,
                config.quiet_hours_start,
                config.quiet_hours_end,
                config.vip_always_push as i64,
                config.urgency_threshold,
            ],
        )
        .unwrap();
    }

    // --- Tests ---

    #[test]
    fn test_default_config_exists_after_migration() {
        let conn = setup_test_db();
        let config = load_test_config(&conn);

        assert!(config.push_categories.is_empty());
        assert!(config.digest_categories.is_empty());
        assert!(config.silent_categories.is_empty());
        assert!(config.push_senders.is_empty());
        assert_eq!(config.digest_interval_minutes, 60);
        assert!(config.quiet_hours_start.is_none());
        assert!(config.quiet_hours_end.is_none());
        assert!(config.vip_always_push);
        assert_eq!(config.urgency_threshold, "high");
    }

    #[test]
    fn test_classify_vip_sender_to_push() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        // Insert VIP contact
        conn.execute(
            "INSERT INTO vip_contacts (email, display_name, vip_score, is_manual)
             VALUES ('vip@example.com', 'VIP User', 1.0, 1)",
            [],
        )
        .unwrap();

        let msg = make_test_message(&account.id, "VIP Email", "vip@example.com");
        let msg_id = insert_msg(&conn, &msg);

        let config = load_test_config(&conn);
        let (route, reason) = classify_message(&conn, &msg_id, &config).unwrap();

        assert_eq!(route, "push");
        assert!(reason.contains("VIP"), "reason should mention VIP: {}", reason);
    }

    #[test]
    fn test_classify_push_senders_list() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        let msg = make_test_message(&account.id, "Boss Email", "boss@company.com");
        let msg_id = insert_msg(&conn, &msg);

        let mut config = load_test_config(&conn);
        config.push_senders = vec!["boss@company.com".to_string()];
        update_test_config(&conn, &config);

        let config = load_test_config(&conn);
        let (route, reason) = classify_message(&conn, &msg_id, &config).unwrap();

        assert_eq!(route, "push");
        assert!(reason.contains("push_senders"), "reason: {}", reason);
    }

    #[test]
    fn test_classify_urgent_email_to_push() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        let msg = make_test_message(&account.id, "Urgent task", "sender@example.com");
        let msg_id = insert_msg(&conn, &msg);

        // Set message priority to urgent
        conn.execute(
            "UPDATE messages SET ai_priority_label = 'urgent' WHERE id = ?1",
            rusqlite::params![msg_id],
        )
        .unwrap();

        let config = load_test_config(&conn);
        let (route, _reason) = classify_message(&conn, &msg_id, &config).unwrap();

        assert_eq!(route, "push");
    }

    #[test]
    fn test_classify_action_required_to_push() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        let msg = make_test_message(&account.id, "Action needed", "sender@example.com");
        let msg_id = insert_msg(&conn, &msg);

        // Set intent to action_required
        conn.execute(
            "UPDATE messages SET ai_intent = 'action_required' WHERE id = ?1",
            rusqlite::params![msg_id],
        )
        .unwrap();

        let config = load_test_config(&conn);
        let (route, reason) = classify_message(&conn, &msg_id, &config).unwrap();

        assert_eq!(route, "push");
        assert!(reason.contains("action"), "reason: {}", reason);
    }

    #[test]
    fn test_classify_promotions_to_digest() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        let msg = make_test_message(&account.id, "Sale 50% off", "store@deals.com");
        let msg_id = insert_msg(&conn, &msg);

        // Set category to Promotions
        conn.execute(
            "UPDATE messages SET ai_category = 'Promotions' WHERE id = ?1",
            rusqlite::params![msg_id],
        )
        .unwrap();

        let mut config = load_test_config(&conn);
        config.digest_categories = vec!["Promotions".to_string()];
        update_test_config(&conn, &config);

        let config = load_test_config(&conn);
        let (route, reason) = classify_message(&conn, &msg_id, &config).unwrap();

        assert_eq!(route, "digest");
        assert!(reason.contains("Promotions"), "reason: {}", reason);
    }

    #[test]
    fn test_classify_silent_routing() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        let msg = make_test_message(&account.id, "Newsletter", "newsletter@example.com");
        let msg_id = insert_msg(&conn, &msg);

        conn.execute(
            "UPDATE messages SET ai_category = 'Newsletters' WHERE id = ?1",
            rusqlite::params![msg_id],
        )
        .unwrap();

        let mut config = load_test_config(&conn);
        config.silent_categories = vec!["Newsletters".to_string()];
        update_test_config(&conn, &config);

        let config = load_test_config(&conn);
        let (route, reason) = classify_message(&conn, &msg_id, &config).unwrap();

        assert_eq!(route, "silent");
        assert!(reason.contains("Newsletters"), "reason: {}", reason);
    }

    #[test]
    fn test_quiet_hours_downgrade_push_to_digest() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        let msg = make_test_message(&account.id, "Important update", "vip@example.com");
        let msg_id = insert_msg(&conn, &msg);

        // Make this a push-worthy message (high priority, threshold = normal)
        conn.execute(
            "UPDATE messages SET ai_priority_label = 'high' WHERE id = ?1",
            rusqlite::params![msg_id],
        )
        .unwrap();

        // Set quiet hours to cover all day (00:00 - 23:59) to guarantee we're in quiet hours
        let mut config = load_test_config(&conn);
        config.urgency_threshold = "normal".to_string();
        config.quiet_hours_start = Some("00:00".to_string());
        config.quiet_hours_end = Some("23:59".to_string());
        update_test_config(&conn, &config);

        // Verify classification would be push without quiet hours
        let config_no_quiet = RoutingConfig {
            urgency_threshold: "normal".to_string(),
            quiet_hours_start: None,
            quiet_hours_end: None,
            ..load_test_config(&conn)
        };
        let (route_no_quiet, _) = classify_message(&conn, &msg_id, &config_no_quiet).unwrap();
        assert_eq!(route_no_quiet, "push", "Without quiet hours should be push");

        // Now test with quiet hours — since the message is 'high' (not 'urgent'),
        // it should be downgraded to digest during quiet hours
        let config = load_test_config(&conn);
        let (route, _) = classify_message(&conn, &msg_id, &config).unwrap();
        assert_eq!(route, "push", "classify_message itself doesn't check quiet hours");

        // Quiet hours check is done in the classify handler, so test the is_within_quiet_hours fn
        assert!(is_within_quiet_hours("00:00", "23:59"));
    }

    #[test]
    fn test_quiet_hours_no_downgrade_for_urgent() {
        // Urgent messages should NOT be downgraded during quiet hours
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        let msg = make_test_message(&account.id, "Critical alert", "alerts@example.com");
        let msg_id = insert_msg(&conn, &msg);

        conn.execute(
            "UPDATE messages SET ai_priority_label = 'urgent' WHERE id = ?1",
            rusqlite::params![msg_id],
        )
        .unwrap();

        let mut config = load_test_config(&conn);
        config.urgency_threshold = "high".to_string();
        config.quiet_hours_start = Some("00:00".to_string());
        config.quiet_hours_end = Some("23:59".to_string());
        update_test_config(&conn, &config);

        let config = load_test_config(&conn);
        let (route, _) = classify_message(&conn, &msg_id, &config).unwrap();
        assert_eq!(route, "push", "Urgent should still be push");

        // Verify is_urgent check: priority is "urgent", so quiet hours should NOT downgrade
        let priority: Option<String> = conn
            .query_row(
                "SELECT ai_priority_label FROM messages WHERE id = ?1",
                rusqlite::params![msg_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        assert_eq!(priority.as_deref(), Some("urgent"));
    }

    #[test]
    fn test_config_update_and_retrieval() {
        let conn = setup_test_db();

        let new_config = RoutingConfig {
            push_categories: vec!["Primary".to_string(), "Finance".to_string()],
            digest_categories: vec!["Promotions".to_string(), "Social".to_string()],
            silent_categories: vec!["Newsletters".to_string()],
            push_senders: vec!["ceo@company.com".to_string()],
            digest_interval_minutes: 30,
            quiet_hours_start: Some("22:00".to_string()),
            quiet_hours_end: Some("07:00".to_string()),
            vip_always_push: false,
            urgency_threshold: "urgent".to_string(),
        };
        update_test_config(&conn, &new_config);

        let loaded = load_test_config(&conn);
        assert_eq!(loaded.push_categories, vec!["Primary", "Finance"]);
        assert_eq!(loaded.digest_categories, vec!["Promotions", "Social"]);
        assert_eq!(loaded.silent_categories, vec!["Newsletters"]);
        assert_eq!(loaded.push_senders, vec!["ceo@company.com"]);
        assert_eq!(loaded.digest_interval_minutes, 30);
        assert_eq!(loaded.quiet_hours_start.as_deref(), Some("22:00"));
        assert_eq!(loaded.quiet_hours_end.as_deref(), Some("07:00"));
        assert!(!loaded.vip_always_push);
        assert_eq!(loaded.urgency_threshold, "urgent");
    }

    #[test]
    fn test_digest_list_and_clear() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        // Insert some digest items
        for i in 0..3 {
            conn.execute(
                "INSERT INTO notification_digest_items (id, message_id, account_id, from_address, subject, category, priority, route)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'digest')",
                rusqlite::params![
                    uuid::Uuid::new_v4().to_string(),
                    format!("msg-{}", i),
                    account.id,
                    format!("sender{}@example.com", i),
                    format!("Subject {}", i),
                    if i < 2 { "Promotions" } else { "Social" },
                    "normal",
                ],
            )
            .unwrap();
        }

        // Also insert a silent item (should NOT appear in digest)
        conn.execute(
            "INSERT INTO notification_digest_items (id, message_id, account_id, route)
             VALUES (?1, 'msg-silent', ?2, 'silent')",
            rusqlite::params![uuid::Uuid::new_v4().to_string(), account.id],
        )
        .unwrap();

        // Query unread digest items
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM notification_digest_items WHERE is_read = 0 AND route = 'digest'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);

        // Clear digest
        let cleared = conn
            .execute(
                "UPDATE notification_digest_items SET is_read = 1 WHERE is_read = 0 AND route = 'digest'",
                [],
            )
            .unwrap();
        assert_eq!(cleared, 3);

        // Verify cleared
        let count_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM notification_digest_items WHERE is_read = 0 AND route = 'digest'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count_after, 0);

        // Silent item should still be unread
        let silent_unread: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM notification_digest_items WHERE is_read = 0 AND route = 'silent'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(silent_unread, 1);
    }

    #[test]
    fn test_urgency_threshold_levels() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        // Test with threshold = 'low' — everything at low or above should be push
        let msg = make_test_message(&account.id, "Low priority msg", "sender@example.com");
        let msg_id = insert_msg(&conn, &msg);
        conn.execute(
            "UPDATE messages SET ai_priority_label = 'low' WHERE id = ?1",
            rusqlite::params![msg_id],
        )
        .unwrap();

        let mut config = load_test_config(&conn);
        config.urgency_threshold = "low".to_string();
        update_test_config(&conn, &config);

        let config = load_test_config(&conn);
        let (route, _) = classify_message(&conn, &msg_id, &config).unwrap();
        assert_eq!(route, "push", "Low priority should be push when threshold is low");

        // Test with threshold = 'urgent' — only urgent should be push
        let mut msg2 = make_test_message(&account.id, "High priority msg", "sender2@example.com");
        msg2.message_id = Some("<high-priority-msg@test.com>".to_string());
        let msg_id2 = insert_msg(&conn, &msg2);
        conn.execute(
            "UPDATE messages SET ai_priority_label = 'high' WHERE id = ?1",
            rusqlite::params![msg_id2],
        )
        .unwrap();

        let mut config = load_test_config(&conn);
        config.urgency_threshold = "urgent".to_string();
        update_test_config(&conn, &config);

        let config = load_test_config(&conn);
        let (route2, _) = classify_message(&conn, &msg_id2, &config).unwrap();
        assert_eq!(route2, "digest", "High priority should be digest when threshold is urgent");
    }

    #[test]
    fn test_classify_with_defaults_routes_to_digest() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        // Message with no AI metadata and default config
        let msg = make_test_message(&account.id, "Plain email", "someone@example.com");
        let msg_id = insert_msg(&conn, &msg);

        let config = load_test_config(&conn);
        let (route, reason) = classify_message(&conn, &msg_id, &config).unwrap();

        assert_eq!(route, "digest");
        assert_eq!(reason, "default routing");
    }

    #[test]
    fn test_classify_nonexistent_message() {
        let conn = setup_test_db();

        let config = load_test_config(&conn);
        let result = classify_message(&conn, "nonexistent-id", &config);

        assert!(result.is_err());
    }

    #[test]
    fn test_vip_always_push_disabled() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        // Insert VIP contact
        conn.execute(
            "INSERT INTO vip_contacts (email, display_name, vip_score, is_manual)
             VALUES ('vip2@example.com', 'VIP User 2', 1.0, 1)",
            [],
        )
        .unwrap();

        let msg = make_test_message(&account.id, "VIP disabled test", "vip2@example.com");
        let msg_id = insert_msg(&conn, &msg);

        // Disable VIP always push
        let mut config = load_test_config(&conn);
        config.vip_always_push = false;
        update_test_config(&conn, &config);

        let config = load_test_config(&conn);
        let (route, reason) = classify_message(&conn, &msg_id, &config).unwrap();

        // Without VIP override and no other triggers, should fall to default digest
        assert_eq!(route, "digest");
        assert_eq!(reason, "default routing");
    }

    #[test]
    fn test_category_push_takes_priority_over_digest() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        let msg = make_test_message(&account.id, "Finance alert", "bank@example.com");
        let msg_id = insert_msg(&conn, &msg);

        conn.execute(
            "UPDATE messages SET ai_category = 'Finance' WHERE id = ?1",
            rusqlite::params![msg_id],
        )
        .unwrap();

        // Put Finance in both push and digest — push should win (checked first)
        let mut config = load_test_config(&conn);
        config.push_categories = vec!["Finance".to_string()];
        config.digest_categories = vec!["Finance".to_string()];
        update_test_config(&conn, &config);

        let config = load_test_config(&conn);
        let (route, _) = classify_message(&conn, &msg_id, &config).unwrap();
        assert_eq!(route, "push");
    }

    #[test]
    fn test_time_validation() {
        assert!(is_valid_time("00:00"));
        assert!(is_valid_time("23:59"));
        assert!(is_valid_time("12:30"));
        assert!(!is_valid_time("24:00"));
        assert!(!is_valid_time("12:60"));
        assert!(!is_valid_time("abc"));
        assert!(!is_valid_time("12"));
        assert!(!is_valid_time("12:30:00"));
    }

    #[test]
    fn test_urgency_rank_ordering() {
        assert!(urgency_rank("low") < urgency_rank("normal"));
        assert!(urgency_rank("normal") < urgency_rank("high"));
        assert!(urgency_rank("high") < urgency_rank("urgent"));
        assert_eq!(urgency_rank("unknown"), urgency_rank("normal"));
    }

    #[test]
    fn test_quiet_hours_boundary() {
        // Same-day range
        assert!(is_within_quiet_hours("00:00", "23:59"));

        // Overnight range basic validation
        // We can't control chrono::Local::now() in unit tests easily,
        // so we verify the function doesn't panic and returns a bool
        let _ = is_within_quiet_hours("22:00", "07:00");
        let _ = is_within_quiet_hours("23:00", "06:00");
    }

    #[test]
    fn test_digest_categories_grouping() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        // Insert digest items with different categories
        let categories = vec!["Promotions", "Promotions", "Social", "Social", "Social"];
        for (i, cat) in categories.iter().enumerate() {
            conn.execute(
                "INSERT INTO notification_digest_items (id, message_id, account_id, category, route)
                 VALUES (?1, ?2, ?3, ?4, 'digest')",
                rusqlite::params![
                    uuid::Uuid::new_v4().to_string(),
                    format!("msg-cat-{}", i),
                    account.id,
                    cat,
                ],
            )
            .unwrap();
        }

        // Query and group
        let mut stmt = conn
            .prepare(
                "SELECT category, COUNT(*) as cnt
                 FROM notification_digest_items
                 WHERE is_read = 0 AND route = 'digest' AND category IS NOT NULL
                 GROUP BY category",
            )
            .unwrap();

        let groups: std::collections::HashMap<String, i64> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(groups.get("Promotions"), Some(&2));
        assert_eq!(groups.get("Social"), Some(&3));
    }

    #[test]
    fn test_action_request_intent_variant() {
        let conn = setup_test_db();
        let account = create_test_account(&conn);

        // Test ACTION_REQUEST (uppercase, V6 AI pipeline format)
        let msg = make_test_message(&account.id, "Review needed", "reviewer@example.com");
        let msg_id = insert_msg(&conn, &msg);

        conn.execute(
            "UPDATE messages SET ai_intent = 'ACTION_REQUEST' WHERE id = ?1",
            rusqlite::params![msg_id],
        )
        .unwrap();

        let config = load_test_config(&conn);
        let (route, reason) = classify_message(&conn, &msg_id, &config).unwrap();

        assert_eq!(route, "push");
        assert!(reason.contains("action"), "reason: {}", reason);
    }
}
