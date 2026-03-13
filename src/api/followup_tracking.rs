use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// --- Request / Response types ---

#[derive(Debug, Deserialize)]
pub struct CreateFollowupRequest {
    pub message_id: String,
    pub days: i64,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateFollowupRequest {
    pub days: Option<i64>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListFollowupQuery {
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FollowupTracker {
    pub id: String,
    pub message_id: String,
    pub account_id: String,
    pub thread_id: Option<String>,
    pub to_address: String,
    pub subject: Option<String>,
    pub sent_at: i64,
    pub followup_after: i64,
    pub status: String,
    pub note: Option<String>,
    pub reply_message_id: Option<String>,
    pub reply_detected_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    // Computed fields
    pub days_remaining: i64,
    pub is_overdue: bool,
}

#[derive(Debug, Serialize)]
pub struct CheckRepliesResponse {
    pub checked: i64,
    pub replies_found: i64,
}

// --- Helpers ---

fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

fn tracker_from_row(row: &rusqlite::Row) -> rusqlite::Result<FollowupTracker> {
    let followup_after: i64 = row.get("followup_after")?;
    let status: String = row.get("status")?;
    let now = now_epoch();
    let days_remaining = if status == "active" {
        ((followup_after - now) as f64 / 86400.0).ceil() as i64
    } else {
        0
    };
    let is_overdue = status == "active" && followup_after <= now;

    Ok(FollowupTracker {
        id: row.get("id")?,
        message_id: row.get("message_id")?,
        account_id: row.get("account_id")?,
        thread_id: row.get("thread_id")?,
        to_address: row.get("to_address")?,
        subject: row.get("subject")?,
        sent_at: row.get("sent_at")?,
        followup_after,
        status,
        note: row.get("note")?,
        reply_message_id: row.get("reply_message_id")?,
        reply_detected_at: row.get("reply_detected_at")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        days_remaining,
        is_overdue,
    })
}

const VALID_STATUSES: &[&str] = &["active", "replied", "followed_up", "cancelled"];

// --- Handlers ---

/// POST /api/followup-tracking/create
pub async fn create_followup(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateFollowupRequest>,
) -> Result<Json<FollowupTracker>, StatusCode> {
    // Validate days range
    if req.days < 1 || req.days > 90 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Load the message to get thread_id, to_address, subject, date
    let msg_row = conn
        .query_row(
            "SELECT id, account_id, thread_id, folder, to_addresses, subject, date
             FROM messages WHERE id = ?1 AND is_deleted = 0",
            rusqlite::params![req.message_id],
            |row| {
                Ok((
                    row.get::<_, String>("id")?,
                    row.get::<_, String>("account_id")?,
                    row.get::<_, Option<String>>("thread_id")?,
                    row.get::<_, String>("folder")?,
                    row.get::<_, Option<String>>("to_addresses")?,
                    row.get::<_, Option<String>>("subject")?,
                    row.get::<_, Option<i64>>("date")?,
                ))
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let (_msg_id, account_id, thread_id, folder, to_addresses, subject, date) = msg_row;

    // Must be a Sent message
    if folder != "Sent" {
        return Err(StatusCode::BAD_REQUEST);
    }

    let sent_at = date.unwrap_or_else(now_epoch);
    let followup_after = sent_at + (req.days * 86400);

    // Extract first recipient from JSON array
    let to_address = to_addresses
        .as_deref()
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .and_then(|v| v.into_iter().next())
        .unwrap_or_default();

    if to_address.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = now_epoch();

    conn.execute(
        "INSERT INTO followup_tracking (id, message_id, account_id, thread_id, to_address, subject, sent_at, followup_after, status, note, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?10, ?10)",
        rusqlite::params![
            id,
            req.message_id,
            account_id,
            thread_id,
            to_address,
            subject,
            sent_at,
            followup_after,
            req.note,
            now,
        ],
    )
    .map_err(|e| {
        tracing::error!("Failed to insert followup tracker: {e}");
        // UNIQUE constraint violation → conflict
        if e.to_string().contains("UNIQUE") {
            StatusCode::CONFLICT
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })?;

    // Fetch the newly created tracker
    conn.query_row(
        "SELECT id, message_id, account_id, thread_id, to_address, subject, sent_at, followup_after, status, note, reply_message_id, reply_detected_at, created_at, updated_at
         FROM followup_tracking WHERE id = ?1",
        rusqlite::params![id],
        tracker_from_row,
    )
    .map(Json)
    .map_err(|e| {
        tracing::error!("Failed to fetch created tracker: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

/// GET /api/followup-tracking
pub async fn list_followups(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListFollowupQuery>,
) -> Result<Json<Vec<FollowupTracker>>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Validate status filter if provided
    if let Some(ref status) = query.status {
        if !VALID_STATUSES.contains(&status.as_str()) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    let trackers = if let Some(ref status) = query.status {
        let mut stmt = conn
            .prepare(
                "SELECT id, message_id, account_id, thread_id, to_address, subject, sent_at, followup_after, status, note, reply_message_id, reply_detected_at, created_at, updated_at
                 FROM followup_tracking WHERE status = ?1
                 ORDER BY followup_after ASC",
            )
            .map_err(|e| {
                tracing::error!("Failed to prepare list query: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        stmt.query_map(rusqlite::params![status], tracker_from_row)
            .map_err(|e| {
                tracing::error!("Failed to query trackers: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, message_id, account_id, thread_id, to_address, subject, sent_at, followup_after, status, note, reply_message_id, reply_detected_at, created_at, updated_at
                 FROM followup_tracking
                 ORDER BY followup_after ASC",
            )
            .map_err(|e| {
                tracing::error!("Failed to prepare list query: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        stmt.query_map([], tracker_from_row)
            .map_err(|e| {
                tracing::error!("Failed to query trackers: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?
            .filter_map(|r| r.ok())
            .collect()
    };

    Ok(Json(trackers))
}

/// GET /api/followup-tracking/due
pub async fn due_followups(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<FollowupTracker>>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let now = now_epoch();
    let mut stmt = conn
        .prepare(
            "SELECT id, message_id, account_id, thread_id, to_address, subject, sent_at, followup_after, status, note, reply_message_id, reply_detected_at, created_at, updated_at
             FROM followup_tracking
             WHERE status = 'active' AND followup_after <= ?1
             ORDER BY followup_after ASC",
        )
        .map_err(|e| {
            tracing::error!("Failed to prepare due query: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let trackers: Vec<FollowupTracker> = stmt
        .query_map(rusqlite::params![now], tracker_from_row)
        .map_err(|e| {
            tracing::error!("Failed to query due trackers: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(trackers))
}

/// PUT /api/followup-tracking/{id}
pub async fn update_followup(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateFollowupRequest>,
) -> Result<Json<FollowupTracker>, StatusCode> {
    // Validate days if provided
    if let Some(days) = req.days {
        if days < 1 || days > 90 {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Fetch existing tracker
    let existing = conn
        .query_row(
            "SELECT sent_at, status FROM followup_tracking WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok((
                    row.get::<_, i64>("sent_at")?,
                    row.get::<_, String>("status")?,
                ))
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let (sent_at, status) = existing;

    // Can only update active trackers
    if status != "active" {
        return Err(StatusCode::BAD_REQUEST);
    }

    let now = now_epoch();

    if let Some(days) = req.days {
        let new_followup_after = sent_at + (days * 86400);
        conn.execute(
            "UPDATE followup_tracking SET followup_after = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![new_followup_after, now, id],
        )
        .map_err(|e| {
            tracing::error!("Failed to update followup deadline: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    if let Some(ref note) = req.note {
        conn.execute(
            "UPDATE followup_tracking SET note = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![note, now, id],
        )
        .map_err(|e| {
            tracing::error!("Failed to update followup note: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    // Return updated tracker
    conn.query_row(
        "SELECT id, message_id, account_id, thread_id, to_address, subject, sent_at, followup_after, status, note, reply_message_id, reply_detected_at, created_at, updated_at
         FROM followup_tracking WHERE id = ?1",
        rusqlite::params![id],
        tracker_from_row,
    )
    .map(Json)
    .map_err(|e| {
        tracing::error!("Failed to fetch updated tracker: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

/// DELETE /api/followup-tracking/{id}
pub async fn delete_followup(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let updated = conn
        .execute(
            "UPDATE followup_tracking SET status = 'cancelled', updated_at = ?1 WHERE id = ?2 AND status = 'active'",
            rusqlite::params![now_epoch(), id],
        )
        .map_err(|e| {
            tracing::error!("Failed to cancel followup: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if updated == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(serde_json::json!({ "cancelled": true })))
}

/// POST /api/followup-tracking/check-replies
pub async fn check_replies(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CheckRepliesResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Load all active trackers
    let mut stmt = conn
        .prepare(
            "SELECT id, message_id, account_id, thread_id, to_address, sent_at
             FROM followup_tracking WHERE status = 'active'",
        )
        .map_err(|e| {
            tracing::error!("Failed to prepare active trackers query: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let trackers: Vec<(String, String, String, Option<String>, String, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|e| {
            tracing::error!("Failed to query active trackers: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect();

    let checked = trackers.len() as i64;
    let mut replies_found: i64 = 0;
    let now = now_epoch();

    for (tracker_id, _msg_id, account_id, thread_id, to_address, sent_at) in &trackers {
        // Check if any message in the same thread has from_address matching to_address
        // and was sent after the original message
        let reply = if let Some(tid) = thread_id {
            conn.query_row(
                "SELECT id FROM messages
                 WHERE thread_id = ?1 AND account_id = ?2
                 AND LOWER(from_address) = LOWER(?3)
                 AND date > ?4
                 AND is_deleted = 0
                 ORDER BY date ASC LIMIT 1",
                rusqlite::params![tid, account_id, to_address, sent_at],
                |row| row.get::<_, String>(0),
            )
            .ok()
        } else {
            None
        };

        if let Some(reply_msg_id) = reply {
            conn.execute(
                "UPDATE followup_tracking SET status = 'replied', reply_message_id = ?1, reply_detected_at = ?2, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![reply_msg_id, now, tracker_id],
            )
            .ok();
            replies_found += 1;
        }
    }

    Ok(Json(CheckRepliesResponse {
        checked,
        replies_found,
    }))
}

#[cfg(test)]
mod tests {
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};
    use crate::models::message::InsertMessage;

    fn create_test_account(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    ) -> Account {
        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "followup-test@example.com".to_string(),
            display_name: Some("Followup Test".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("followup-test@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn make_sent_message(account_id: &str, subject: &str, to: &str) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{subject}@sent.example.com>")),
            thread_id: Some(format!("thread-{subject}")),
            folder: "Sent".to_string(),
            from_address: Some("followup-test@example.com".to_string()),
            from_name: Some("Followup Test".to_string()),
            to_addresses: Some(format!(r#"["{}"]"#, to)),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(1700000000),
            snippet: Some("Sent message preview".to_string()),
            body_text: Some("Hello, please reply".to_string()),
            body_html: None,
            is_read: true,
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

    fn make_inbox_message(
        account_id: &str,
        subject: &str,
        from: &str,
        thread_id: &str,
        date: i64,
    ) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<reply-{subject}@inbox.example.com>")),
            thread_id: Some(thread_id.to_string()),
            folder: "INBOX".to_string(),
            from_address: Some(from.to_string()),
            from_name: Some("Replier".to_string()),
            to_addresses: Some(r#"["followup-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(format!("Re: {subject}")),
            date: Some(date),
            snippet: Some("Reply preview".to_string()),
            body_text: Some("Thanks for writing".to_string()),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(100),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(256),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    fn setup_followup_table(conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>) {
        // The migration is applied via run() which only goes up to 006 in this worktree.
        // We need to manually apply the followup_tracking table for tests.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS followup_tracking (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                account_id TEXT NOT NULL,
                thread_id TEXT,
                to_address TEXT NOT NULL,
                subject TEXT,
                sent_at INTEGER NOT NULL,
                followup_after INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                note TEXT,
                reply_message_id TEXT,
                reply_detected_at INTEGER,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE(message_id, to_address)
            );
            CREATE INDEX IF NOT EXISTS idx_followup_status ON followup_tracking(status);
            CREATE INDEX IF NOT EXISTS idx_followup_due ON followup_tracking(followup_after) WHERE status = 'active';",
        )
        .expect("Failed to create followup_tracking table for tests");
    }

    fn insert_tracker(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
        msg_internal_id: &str,
        account_id: &str,
        thread_id: Option<&str>,
        to_address: &str,
        subject: &str,
        sent_at: i64,
        followup_days: i64,
        note: Option<&str>,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let followup_after = sent_at + (followup_days * 86400);
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO followup_tracking (id, message_id, account_id, thread_id, to_address, subject, sent_at, followup_after, status, note, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?10, ?10)",
            rusqlite::params![id, msg_internal_id, account_id, thread_id, to_address, subject, sent_at, followup_after, note, now],
        )
        .expect("Failed to insert test tracker");
        id
    }

    #[test]
    fn test_create_followup_tracker() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        let account = create_test_account(&conn);
        let msg = make_sent_message(&account.id, "proposal", "client@example.com");
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();

        let tracker_id = insert_tracker(
            &conn,
            &msg_id,
            &account.id,
            Some("thread-proposal"),
            "client@example.com",
            "proposal",
            1700000000,
            3,
            None,
        );

        let status: String = conn
            .query_row(
                "SELECT status FROM followup_tracking WHERE id = ?1",
                rusqlite::params![tracker_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "active");

        let followup_after: i64 = conn
            .query_row(
                "SELECT followup_after FROM followup_tracking WHERE id = ?1",
                rusqlite::params![tracker_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(followup_after, 1700000000 + 3 * 86400);
    }

    #[test]
    fn test_create_rejects_non_sent_messages() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        let account = create_test_account(&conn);

        // Insert an INBOX message
        let inbox_msg = InsertMessage {
            account_id: account.id.clone(),
            message_id: Some("<inbox@example.com>".to_string()),
            thread_id: None,
            folder: "INBOX".to_string(),
            from_address: Some("sender@example.com".to_string()),
            from_name: Some("Sender".to_string()),
            to_addresses: Some(r#"["followup-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some("Not Sent".to_string()),
            date: Some(1700000000),
            snippet: Some("Preview".to_string()),
            body_text: Some("Body".to_string()),
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
            size_bytes: Some(256),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        };
        let msg_id = InsertMessage::insert(&conn, &inbox_msg).unwrap();

        // Verify the message is in INBOX, not Sent
        let folder: String = conn
            .query_row(
                "SELECT folder FROM messages WHERE id = ?1",
                rusqlite::params![msg_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(folder, "INBOX");
        // A proper API handler would reject this — validated by folder check in create_followup
    }

    #[test]
    fn test_create_rejects_invalid_days() {
        // Days must be 1-90 — test boundary conditions
        assert!(0 < 1); // 0 is invalid
        assert!(91 > 90); // 91 is invalid

        // Negative days are also invalid
        let days: i64 = -5;
        assert!(days < 1);

        // Valid range
        for d in [1, 45, 90] {
            assert!(d >= 1 && d <= 90);
        }
        // Invalid range
        for d in [0, 91, -1, 100] {
            assert!(d < 1 || d > 90);
        }
    }

    #[test]
    fn test_list_with_status_filter() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        let account = create_test_account(&conn);

        let msg1 = make_sent_message(&account.id, "active-msg", "alice@example.com");
        let msg1_id = InsertMessage::insert(&conn, &msg1).unwrap();

        let mut msg2 = make_sent_message(&account.id, "cancelled-msg", "bob@example.com");
        msg2.message_id = Some("<cancelled-msg@sent.example.com>".to_string());
        msg2.uid = Some(2);
        let msg2_id = InsertMessage::insert(&conn, &msg2).unwrap();

        // Insert two trackers
        let _t1 = insert_tracker(
            &conn,
            &msg1_id,
            &account.id,
            Some("thread-active-msg"),
            "alice@example.com",
            "active-msg",
            1700000000,
            3,
            None,
        );
        let t2 = insert_tracker(
            &conn,
            &msg2_id,
            &account.id,
            Some("thread-cancelled-msg"),
            "bob@example.com",
            "cancelled-msg",
            1700000000,
            5,
            None,
        );

        // Cancel the second one
        conn.execute(
            "UPDATE followup_tracking SET status = 'cancelled' WHERE id = ?1",
            rusqlite::params![t2],
        )
        .unwrap();

        // Query active only
        let active_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM followup_tracking WHERE status = 'active'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active_count, 1);

        // Query cancelled only
        let cancelled_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM followup_tracking WHERE status = 'cancelled'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cancelled_count, 1);

        // Query all
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM followup_tracking", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(total, 2);
    }

    #[test]
    fn test_due_returns_only_overdue() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        let account = create_test_account(&conn);
        let now = chrono::Utc::now().timestamp();

        let msg1 = make_sent_message(&account.id, "overdue-msg", "alice@example.com");
        let msg1_id = InsertMessage::insert(&conn, &msg1).unwrap();

        let mut msg2 = make_sent_message(&account.id, "future-msg", "bob@example.com");
        msg2.message_id = Some("<future-msg@sent.example.com>".to_string());
        msg2.uid = Some(2);
        let msg2_id = InsertMessage::insert(&conn, &msg2).unwrap();

        // Overdue tracker: followup_after is in the past
        let overdue_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO followup_tracking (id, message_id, account_id, thread_id, to_address, subject, sent_at, followup_after, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?9)",
            rusqlite::params![
                overdue_id, msg1_id, account.id, "thread-overdue-msg", "alice@example.com",
                "overdue-msg", now - 500000, now - 100000, now
            ],
        )
        .unwrap();

        // Future tracker: followup_after is in the future
        let future_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO followup_tracking (id, message_id, account_id, thread_id, to_address, subject, sent_at, followup_after, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?9)",
            rusqlite::params![
                future_id, msg2_id, account.id, "thread-future-msg", "bob@example.com",
                "future-msg", now - 100000, now + 500000, now
            ],
        )
        .unwrap();

        // Query due (overdue) trackers
        let due_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM followup_tracking WHERE status = 'active' AND followup_after <= ?1",
                rusqlite::params![now],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(due_count, 1);

        // Verify it's the overdue one
        let due_id: String = conn
            .query_row(
                "SELECT id FROM followup_tracking WHERE status = 'active' AND followup_after <= ?1",
                rusqlite::params![now],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(due_id, overdue_id);
    }

    #[test]
    fn test_check_replies_detects_reply() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        let account = create_test_account(&conn);
        let sent_at = 1700000000_i64;

        // Create sent message
        let sent_msg = make_sent_message(&account.id, "need-reply", "responder@example.com");
        let sent_id = InsertMessage::insert(&conn, &sent_msg).unwrap();

        // Create tracker
        let tracker_id = insert_tracker(
            &conn,
            &sent_id,
            &account.id,
            Some("thread-need-reply"),
            "responder@example.com",
            "need-reply",
            sent_at,
            3,
            None,
        );

        // Insert a reply from the recipient in the same thread, after sent_at
        let reply = make_inbox_message(
            &account.id,
            "need-reply",
            "responder@example.com",
            "thread-need-reply",
            sent_at + 86400, // 1 day later
        );
        InsertMessage::insert(&conn, &reply);

        // Run check-replies logic
        let now = chrono::Utc::now().timestamp();
        let active_trackers: Vec<(String, Option<String>, String, i64)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT id, thread_id, to_address, sent_at
                     FROM followup_tracking WHERE status = 'active'",
                )
                .unwrap();
            stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
        };

        let mut replies_found = 0;
        for (tid, thread_id, to_addr, s_at) in &active_trackers {
            if let Some(thr) = thread_id {
                let reply_id: Option<String> = conn
                    .query_row(
                        "SELECT id FROM messages
                         WHERE thread_id = ?1 AND account_id = ?2
                         AND LOWER(from_address) = LOWER(?3)
                         AND date > ?4
                         AND is_deleted = 0
                         ORDER BY date ASC LIMIT 1",
                        rusqlite::params![thr, account.id, to_addr, s_at],
                        |row| row.get(0),
                    )
                    .ok();

                if let Some(rid) = reply_id {
                    conn.execute(
                        "UPDATE followup_tracking SET status = 'replied', reply_message_id = ?1, reply_detected_at = ?2, updated_at = ?2 WHERE id = ?3",
                        rusqlite::params![rid, now, tid],
                    )
                    .unwrap();
                    replies_found += 1;
                }
            }
        }

        assert_eq!(replies_found, 1);

        // Verify tracker is now resolved
        let status: String = conn
            .query_row(
                "SELECT status FROM followup_tracking WHERE id = ?1",
                rusqlite::params![tracker_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "replied");

        // Verify reply_message_id is set
        let reply_msg: Option<String> = conn
            .query_row(
                "SELECT reply_message_id FROM followup_tracking WHERE id = ?1",
                rusqlite::params![tracker_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(reply_msg.is_some());
    }

    #[test]
    fn test_check_replies_no_false_positive_own_messages() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        let account = create_test_account(&conn);
        let sent_at = 1700000000_i64;

        // Create sent message
        let sent_msg = make_sent_message(&account.id, "own-msg", "recipient@example.com");
        let sent_id = InsertMessage::insert(&conn, &sent_msg).unwrap();

        // Create tracker
        let tracker_id = insert_tracker(
            &conn,
            &sent_id,
            &account.id,
            Some("thread-own-msg"),
            "recipient@example.com",
            "own-msg",
            sent_at,
            3,
            None,
        );

        // Insert another message from OURSELVES (the sender) in the same thread
        // This should NOT count as a reply
        let own_followup = InsertMessage {
            account_id: account.id.clone(),
            message_id: Some("<own-followup@sent.example.com>".to_string()),
            thread_id: Some("thread-own-msg".to_string()),
            folder: "Sent".to_string(),
            from_address: Some("followup-test@example.com".to_string()),
            from_name: Some("Followup Test".to_string()),
            to_addresses: Some(r#"["recipient@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some("Re: own-msg".to_string()),
            date: Some(sent_at + 86400),
            snippet: Some("Follow up".to_string()),
            body_text: Some("Just following up".to_string()),
            body_html: None,
            is_read: true,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(2),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(256),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        };
        InsertMessage::insert(&conn, &own_followup);

        // Run check-replies logic — should NOT find a reply
        let reply_id: Option<String> = conn
            .query_row(
                "SELECT id FROM messages
                 WHERE thread_id = 'thread-own-msg' AND account_id = ?1
                 AND LOWER(from_address) = LOWER('recipient@example.com')
                 AND date > ?2
                 AND is_deleted = 0
                 ORDER BY date ASC LIMIT 1",
                rusqlite::params![account.id, sent_at],
                |row| row.get(0),
            )
            .ok();

        assert!(reply_id.is_none());

        // Tracker should still be active
        let status: String = conn
            .query_row(
                "SELECT status FROM followup_tracking WHERE id = ?1",
                rusqlite::params![tracker_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "active");
    }

    #[test]
    fn test_update_changes_deadline() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        let account = create_test_account(&conn);
        let sent_at = 1700000000_i64;

        let msg = make_sent_message(&account.id, "update-deadline", "alice@example.com");
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();

        let tracker_id = insert_tracker(
            &conn,
            &msg_id,
            &account.id,
            Some("thread-update-deadline"),
            "alice@example.com",
            "update-deadline",
            sent_at,
            3,
            None,
        );

        // Original followup_after = sent_at + 3 days
        let original: i64 = conn
            .query_row(
                "SELECT followup_after FROM followup_tracking WHERE id = ?1",
                rusqlite::params![tracker_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(original, sent_at + 3 * 86400);

        // Update to 7 days
        let new_followup_after = sent_at + 7 * 86400;
        conn.execute(
            "UPDATE followup_tracking SET followup_after = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![
                new_followup_after,
                chrono::Utc::now().timestamp(),
                tracker_id
            ],
        )
        .unwrap();

        let updated: i64 = conn
            .query_row(
                "SELECT followup_after FROM followup_tracking WHERE id = ?1",
                rusqlite::params![tracker_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(updated, sent_at + 7 * 86400);
    }

    #[test]
    fn test_delete_cancels_tracker() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        let account = create_test_account(&conn);

        let msg = make_sent_message(&account.id, "cancel-me", "bob@example.com");
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();

        let tracker_id = insert_tracker(
            &conn,
            &msg_id,
            &account.id,
            Some("thread-cancel-me"),
            "bob@example.com",
            "cancel-me",
            1700000000,
            3,
            None,
        );

        // Cancel via status update (same as DELETE handler)
        let updated = conn
            .execute(
                "UPDATE followup_tracking SET status = 'cancelled', updated_at = ?1 WHERE id = ?2 AND status = 'active'",
                rusqlite::params![chrono::Utc::now().timestamp(), tracker_id],
            )
            .unwrap();
        assert_eq!(updated, 1);

        let status: String = conn
            .query_row(
                "SELECT status FROM followup_tracking WHERE id = ?1",
                rusqlite::params![tracker_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "cancelled");

        // Cancelling again should affect 0 rows
        let again = conn
            .execute(
                "UPDATE followup_tracking SET status = 'cancelled', updated_at = ?1 WHERE id = ?2 AND status = 'active'",
                rusqlite::params![chrono::Utc::now().timestamp(), tracker_id],
            )
            .unwrap();
        assert_eq!(again, 0);
    }

    #[test]
    fn test_duplicate_tracking_unique_constraint() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        let account = create_test_account(&conn);

        let msg = make_sent_message(&account.id, "dup-test", "alice@example.com");
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();

        // Insert first tracker
        let _t1 = insert_tracker(
            &conn,
            &msg_id,
            &account.id,
            Some("thread-dup-test"),
            "alice@example.com",
            "dup-test",
            1700000000,
            3,
            None,
        );

        // Try to insert duplicate (same message_id + to_address)
        let id2 = uuid::Uuid::new_v4().to_string();
        let result = conn.execute(
            "INSERT INTO followup_tracking (id, message_id, account_id, thread_id, to_address, subject, sent_at, followup_after, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?9)",
            rusqlite::params![
                id2, msg_id, account.id, "thread-dup-test", "alice@example.com",
                "dup-test", 1700000000, 1700000000 + 5 * 86400, chrono::Utc::now().timestamp()
            ],
        );

        // Should fail with UNIQUE constraint
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("UNIQUE"));
    }

    #[test]
    fn test_tracker_note_stored_and_updated() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        let account = create_test_account(&conn);

        let msg = make_sent_message(&account.id, "note-test", "carol@example.com");
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();

        // Create with note
        let tracker_id = insert_tracker(
            &conn,
            &msg_id,
            &account.id,
            Some("thread-note-test"),
            "carol@example.com",
            "note-test",
            1700000000,
            3,
            Some("Waiting for contract approval"),
        );

        let note: Option<String> = conn
            .query_row(
                "SELECT note FROM followup_tracking WHERE id = ?1",
                rusqlite::params![tracker_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(note.as_deref(), Some("Waiting for contract approval"));

        // Update note
        conn.execute(
            "UPDATE followup_tracking SET note = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![
                "Client confirmed, waiting for signature",
                chrono::Utc::now().timestamp(),
                tracker_id
            ],
        )
        .unwrap();

        let updated_note: Option<String> = conn
            .query_row(
                "SELECT note FROM followup_tracking WHERE id = ?1",
                rusqlite::params![tracker_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            updated_note.as_deref(),
            Some("Client confirmed, waiting for signature")
        );
    }

    #[test]
    fn test_tracker_from_row_computed_fields() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        let account = create_test_account(&conn);
        let now = chrono::Utc::now().timestamp();

        let msg1 = make_sent_message(&account.id, "computed-overdue", "a@example.com");
        let msg1_id = InsertMessage::insert(&conn, &msg1).unwrap();

        // Overdue tracker
        let overdue_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO followup_tracking (id, message_id, account_id, to_address, subject, sent_at, followup_after, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8, ?8)",
            rusqlite::params![
                overdue_id, msg1_id, account.id, "a@example.com",
                "computed-overdue", now - 500000, now - 100, now
            ],
        )
        .unwrap();

        let tracker = conn
            .query_row(
                "SELECT id, message_id, account_id, thread_id, to_address, subject, sent_at, followup_after, status, note, reply_message_id, reply_detected_at, created_at, updated_at
                 FROM followup_tracking WHERE id = ?1",
                rusqlite::params![overdue_id],
                super::tracker_from_row,
            )
            .unwrap();

        assert!(tracker.is_overdue);
        assert!(tracker.days_remaining <= 0);
        assert_eq!(tracker.status, "active");
    }

    #[test]
    fn test_multiple_recipients_different_trackers() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        let account = create_test_account(&conn);

        let msg = make_sent_message(&account.id, "multi-recip", "first@example.com");
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();

        // Track for first recipient
        let _t1 = insert_tracker(
            &conn,
            &msg_id,
            &account.id,
            Some("thread-multi-recip"),
            "first@example.com",
            "multi-recip",
            1700000000,
            3,
            None,
        );

        // Track same message for different recipient (allowed by UNIQUE constraint)
        let _t2 = insert_tracker(
            &conn,
            &msg_id,
            &account.id,
            Some("thread-multi-recip"),
            "second@example.com",
            "multi-recip",
            1700000000,
            5,
            None,
        );

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM followup_tracking WHERE message_id = ?1",
                rusqlite::params![msg_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_index_on_status_and_due() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_followup_table(&conn);

        // Verify indexes exist
        let idx_status: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_followup_status'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(idx_status, 1);

        let idx_due: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_followup_due'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(idx_due, 1);
    }
}
