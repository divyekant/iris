use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::models::message::MessageSummary;
use crate::AppState;

// ── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipScore {
    pub email: String,
    pub score: f64,
    pub frequency_score: f64,
    pub recency_score: f64,
    pub reply_rate_score: f64,
    pub bidirectional_score: f64,
    pub thread_depth_score: f64,
    pub computed_at: i64,
}

#[derive(Debug, Serialize)]
pub struct ComputeResponse {
    pub scored: usize,
}

#[derive(Debug, Serialize)]
pub struct PrioritizedMessage {
    #[serde(flatten)]
    pub message: MessageSummary,
    pub relationship_score: f64,
    pub blended_score: f64,
}

#[derive(Debug, Serialize)]
pub struct PrioritizedResponse {
    pub messages: Vec<PrioritizedMessage>,
    pub total: i64,
}

#[derive(Debug, Deserialize)]
pub struct PrioritizedParams {
    pub account_id: Option<String>,
    pub folder: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ── Score computation helpers ───────────────────────────────────────────────

/// Contact statistics gathered from the messages table.
#[derive(Debug, Default)]
struct ContactStats {
    /// Total messages received from this contact
    received_count: i64,
    /// Total messages sent to this contact (user is sender)
    sent_count: i64,
    /// Unix timestamp of most recent interaction (sent or received)
    last_interaction: i64,
    /// Number of threads where user replied to this contact
    reply_threads: i64,
    /// Total threads involving this contact
    total_threads: i64,
    /// Sum of thread depths for threads with this contact
    thread_depth_sum: i64,
    /// Number of threads used for depth calculation
    thread_count: i64,
}

/// Compute relationship scores for all contacts.
/// Returns a vec of (email, RelationshipScore) pairs.
pub fn compute_scores(
    conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    now_epoch: i64,
) -> Result<Vec<RelationshipScore>, rusqlite::Error> {
    let mut stats: HashMap<String, ContactStats> = HashMap::new();

    // Get all user account emails for identifying sent messages
    let user_emails: Vec<String> = {
        let mut stmt = conn.prepare("SELECT email FROM accounts WHERE is_active = 1")?;
        stmt.query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .map(|e| e.to_lowercase())
            .collect()
    };

    if user_emails.is_empty() {
        return Ok(Vec::new());
    }

    // Gather received messages (from_address is the contact)
    {
        let mut stmt = conn.prepare(
            "SELECT LOWER(from_address) as email, COUNT(*) as cnt, MAX(date) as latest
             FROM messages
             WHERE from_address IS NOT NULL AND is_deleted = 0 AND is_draft = 0
             GROUP BY LOWER(from_address)",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows.flatten() {
            let (email, count, latest) = row;
            // Skip if this is one of the user's own accounts
            if user_emails.contains(&email) {
                continue;
            }
            let entry = stats.entry(email).or_default();
            entry.received_count = count;
            if latest > entry.last_interaction {
                entry.last_interaction = latest;
            }
        }
    }

    // Gather sent messages — extract recipient emails from to_addresses JSON
    {
        let mut stmt = conn.prepare(
            "SELECT to_addresses, date FROM messages
             WHERE folder = 'Sent' AND is_deleted = 0 AND to_addresses IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
            ))
        })?;
        for row in rows.flatten() {
            let (to_json, date) = row;
            let recipients = parse_recipients(&to_json);
            for recip in recipients {
                let recip_lower = recip.to_lowercase();
                if user_emails.contains(&recip_lower) {
                    continue;
                }
                let entry = stats.entry(recip_lower).or_default();
                entry.sent_count += 1;
                if date > entry.last_interaction {
                    entry.last_interaction = date;
                }
            }
        }
    }

    // Thread analysis: reply rate and depth
    {
        // For each thread, get participants and message count
        let mut stmt = conn.prepare(
            "SELECT thread_id, LOWER(from_address), COUNT(*) as msg_count
             FROM messages
             WHERE thread_id IS NOT NULL AND is_deleted = 0 AND from_address IS NOT NULL
             GROUP BY thread_id, LOWER(from_address)",
        )?;

        // Build thread → participants map
        let mut thread_participants: HashMap<String, Vec<(String, i64)>> = HashMap::new();
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows.flatten() {
            let (thread_id, email, count) = row;
            thread_participants
                .entry(thread_id)
                .or_default()
                .push((email, count));
        }

        // Thread depth (total messages per thread)
        let mut thread_depths: HashMap<String, i64> = HashMap::new();
        {
            let mut depth_stmt = conn.prepare(
                "SELECT thread_id, COUNT(*) FROM messages
                 WHERE thread_id IS NOT NULL AND is_deleted = 0
                 GROUP BY thread_id",
            )?;
            let rows = depth_stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for row in rows.flatten() {
                thread_depths.insert(row.0, row.1);
            }
        }

        for (thread_id, participants) in &thread_participants {
            let has_user = participants.iter().any(|(e, _)| user_emails.contains(e));
            let depth = thread_depths.get(thread_id).copied().unwrap_or(1);

            for (email, _count) in participants {
                if user_emails.contains(email) {
                    continue;
                }
                let entry = stats.entry(email.clone()).or_default();
                entry.total_threads += 1;
                entry.thread_depth_sum += depth;
                entry.thread_count += 1;

                // If the user also participated in this thread, it's a reply
                if has_user {
                    entry.reply_threads += 1;
                }
            }
        }
    }

    // Now compute scores
    // Find the max received count for normalization
    let max_received = stats.values().map(|s| s.received_count).max().unwrap_or(1).max(1);

    let half_life_seconds: f64 = 30.0 * 24.0 * 3600.0; // 30 days in seconds

    let scores: Vec<RelationshipScore> = stats
        .into_iter()
        .map(|(email, s)| {
            // Frequency: normalized against top contact (0-1)
            let frequency_score = (s.received_count as f64) / (max_received as f64);

            // Recency: exponential decay with 30-day half-life
            let age_seconds = (now_epoch - s.last_interaction).max(0) as f64;
            let recency_score = (-age_seconds * (2.0_f64.ln()) / half_life_seconds).exp();

            // Reply rate: fraction of threads with this contact that user replied to
            let reply_rate_score = if s.total_threads > 0 {
                (s.reply_threads as f64) / (s.total_threads as f64)
            } else {
                0.0
            };

            // Bidirectional: does the contact also send to us? (min of sent ratio, received ratio capped at 1)
            let bidirectional_score = if s.received_count > 0 && s.sent_count > 0 {
                let ratio = (s.sent_count as f64) / (s.received_count as f64);
                ratio.min(1.0)
            } else {
                0.0
            };

            // Thread depth: average depth, normalized (cap at 10 for normalization)
            let avg_depth = if s.thread_count > 0 {
                (s.thread_depth_sum as f64) / (s.thread_count as f64)
            } else {
                1.0
            };
            let thread_depth_score = ((avg_depth - 1.0) / 9.0).clamp(0.0, 1.0);

            // Weighted composite
            let score = 0.25 * frequency_score
                + 0.25 * recency_score
                + 0.25 * reply_rate_score
                + 0.15 * bidirectional_score
                + 0.10 * thread_depth_score;

            RelationshipScore {
                email,
                score: (score * 1000.0).round() / 1000.0,
                frequency_score: (frequency_score * 1000.0).round() / 1000.0,
                recency_score: (recency_score * 1000.0).round() / 1000.0,
                reply_rate_score: (reply_rate_score * 1000.0).round() / 1000.0,
                bidirectional_score: (bidirectional_score * 1000.0).round() / 1000.0,
                thread_depth_score: (thread_depth_score * 1000.0).round() / 1000.0,
                computed_at: now_epoch,
            }
        })
        .collect();

    Ok(scores)
}

/// Parse recipient emails from a JSON array string like `["a@b.com","c@d.com"]`
fn parse_recipients(json_str: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(json_str).unwrap_or_default()
}

/// Convert an AI priority label to a numeric score for blending.
pub fn ai_priority_to_score(label: Option<&str>) -> f64 {
    match label {
        Some("urgent") => 1.0,
        Some("high") => 0.75,
        Some("normal") => 0.5,
        Some("low") => 0.25,
        _ => 0.5, // default to normal
    }
}

/// Compute blended priority: 60% AI priority + 40% relationship score.
pub fn blended_priority(ai_label: Option<&str>, relationship_score: f64) -> f64 {
    let ai = ai_priority_to_score(ai_label);
    let raw = 0.6 * ai + 0.4 * relationship_score;
    (raw * 1000.0).round() / 1000.0
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// POST /api/ai/relationship-priority — Compute and store relationship scores
pub async fn compute_relationship_scores(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ComputeResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let now = chrono::Utc::now().timestamp();
    let scores = compute_scores(&conn, now).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let count = scores.len();

    // Store scores in the relationship_scores table
    for score in &scores {
        conn.execute(
            "INSERT OR REPLACE INTO relationship_scores (email, score, frequency_score, recency_score, reply_rate_score, bidirectional_score, thread_depth_score, computed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                score.email,
                score.score,
                score.frequency_score,
                score.recency_score,
                score.reply_rate_score,
                score.bidirectional_score,
                score.thread_depth_score,
                score.computed_at,
            ],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    Ok(Json(ComputeResponse { scored: count }))
}

/// GET /api/contacts/{email}/relationship — Get relationship score for a contact
pub async fn get_contact_relationship(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
) -> Result<Json<RelationshipScore>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let email_lower = email.to_lowercase();

    conn.query_row(
        "SELECT email, score, frequency_score, recency_score, reply_rate_score, bidirectional_score, thread_depth_score, computed_at
         FROM relationship_scores WHERE email = ?1",
        rusqlite::params![email_lower],
        |row| {
            Ok(RelationshipScore {
                email: row.get(0)?,
                score: row.get(1)?,
                frequency_score: row.get(2)?,
                recency_score: row.get(3)?,
                reply_rate_score: row.get(4)?,
                bidirectional_score: row.get(5)?,
                thread_depth_score: row.get(6)?,
                computed_at: row.get(7)?,
            })
        },
    )
    .map(Json)
    .map_err(|_| StatusCode::NOT_FOUND)
}

/// GET /api/messages/prioritized — List inbox messages re-ranked by blended priority
pub async fn get_prioritized_messages(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PrioritizedParams>,
) -> Result<Json<PrioritizedResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0).max(0);

    // Load all relationship scores into a map for fast lookup
    let mut rel_scores: HashMap<String, f64> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT email, score FROM relationship_scores")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for row in rows.flatten() {
            rel_scores.insert(row.0, row.1);
        }
    }

    // Allowed folders
    const ALLOWED_FOLDERS: &[&str] = &["INBOX", "Sent", "Drafts", "Starred", "Archive", "Trash"];
    let folder = params
        .folder
        .as_deref()
        .filter(|f| ALLOWED_FOLDERS.contains(f))
        .unwrap_or("INBOX");

    let folder_where = match folder {
        "Starred" => "m.is_starred = 1 AND m.is_deleted = 0",
        "Trash" => "m.is_deleted = 1",
        "Drafts" => "m.is_draft = 1 AND m.is_deleted = 0",
        "INBOX" => "m.folder = 'INBOX' AND m.is_deleted = 0",
        "Sent" => "m.folder = 'Sent' AND m.is_deleted = 0",
        "Archive" => "m.folder = 'Archive' AND m.is_deleted = 0",
        _ => "m.folder = 'INBOX' AND m.is_deleted = 0",
    };

    let select_cols = "m.id, m.account_id, m.thread_id, m.folder, m.from_address, m.from_name,
                       m.subject, m.snippet, m.date, m.is_read, m.is_starred, m.has_attachments,
                       m.labels, m.ai_priority_label, m.ai_category";

    // Fetch all matching messages (we'll sort in-memory after blending)
    let query = if let Some(ref account_id) = params.account_id {
        format!(
            "WITH threaded AS (
                SELECT m.*, ROW_NUMBER() OVER (
                    PARTITION BY COALESCE(m.thread_id, m.id)
                    ORDER BY m.date DESC
                ) as rn
                FROM messages m
                WHERE m.account_id = ?1 AND {folder_where}
            )
            SELECT {select_cols} FROM threaded m WHERE m.rn = 1"
        )
    } else {
        format!(
            "WITH threaded AS (
                SELECT m.*, ROW_NUMBER() OVER (
                    PARTITION BY COALESCE(m.thread_id, m.id)
                    ORDER BY m.date DESC
                ) as rn
                FROM messages m
                JOIN accounts a ON m.account_id = a.id
                WHERE a.is_active = 1 AND {folder_where}
            )
            SELECT {select_cols} FROM threaded m WHERE m.rn = 1"
        )
    };

    let mut stmt = conn.prepare(&query).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let messages: Vec<MessageSummary> = if let Some(ref account_id) = params.account_id {
        stmt.query_map(rusqlite::params![account_id], MessageSummary::from_row)
    } else {
        stmt.query_map([], MessageSummary::from_row)
    }
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .filter_map(|r| r.ok())
    .collect();

    let total = messages.len() as i64;

    // Blend and sort
    let mut prioritized: Vec<PrioritizedMessage> = messages
        .into_iter()
        .map(|msg| {
            let sender = msg
                .from_address
                .as_deref()
                .map(|e| e.to_lowercase())
                .unwrap_or_default();
            let rel = rel_scores.get(&sender).copied().unwrap_or(0.0);
            let blended = blended_priority(msg.ai_priority_label.as_deref(), rel);
            PrioritizedMessage {
                message: msg,
                relationship_score: rel,
                blended_score: blended,
            }
        })
        .collect();

    // Sort descending by blended score, then by date descending as tiebreaker
    prioritized.sort_by(|a, b| {
        b.blended_score
            .partial_cmp(&a.blended_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let da = a.message.date.unwrap_or(0);
                let db = b.message.date.unwrap_or(0);
                db.cmp(&da)
            })
    });

    // Apply pagination
    let page: Vec<PrioritizedMessage> = prioritized
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect();

    Ok(Json(PrioritizedResponse {
        messages: page,
        total,
    }))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};
    use crate::models::message::InsertMessage;

    fn create_test_account(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    ) -> Account {
        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "user@example.com".to_string(),
            display_name: Some("Test User".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("user@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn make_message(
        account_id: &str,
        from: &str,
        to: &str,
        subject: &str,
        folder: &str,
        date: i64,
        thread_id: Option<&str>,
    ) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{subject}-{date}@test>")),
            thread_id: thread_id.map(|s| s.to_string()),
            folder: folder.to_string(),
            from_address: Some(from.to_string()),
            from_name: Some(from.split('@').next().unwrap_or(from).to_string()),
            to_addresses: Some(format!(r#"["{}"]"#, to)),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(date),
            snippet: Some("test snippet".to_string()),
            body_text: Some("test body".to_string()),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(date), // use date as uid for uniqueness
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(100),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    fn ensure_relationship_table(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    ) {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS relationship_scores (
                email TEXT PRIMARY KEY,
                score REAL NOT NULL DEFAULT 0.0,
                frequency_score REAL NOT NULL DEFAULT 0.0,
                recency_score REAL NOT NULL DEFAULT 0.0,
                reply_rate_score REAL NOT NULL DEFAULT 0.0,
                bidirectional_score REAL NOT NULL DEFAULT 0.0,
                thread_depth_score REAL NOT NULL DEFAULT 0.0,
                computed_at INTEGER NOT NULL DEFAULT (unixepoch())
            );",
        )
        .unwrap();
    }

    // Test 1: ai_priority_to_score mapping
    #[test]
    fn test_ai_priority_to_score() {
        assert_eq!(ai_priority_to_score(Some("urgent")), 1.0);
        assert_eq!(ai_priority_to_score(Some("high")), 0.75);
        assert_eq!(ai_priority_to_score(Some("normal")), 0.5);
        assert_eq!(ai_priority_to_score(Some("low")), 0.25);
        assert_eq!(ai_priority_to_score(None), 0.5); // defaults to normal
        assert_eq!(ai_priority_to_score(Some("unknown")), 0.5);
    }

    // Test 2: blended_priority calculation
    #[test]
    fn test_blended_priority() {
        // urgent (1.0) + high relationship (0.8) = 0.6*1.0 + 0.4*0.8 = 0.92
        assert_eq!(blended_priority(Some("urgent"), 0.8), 0.92);
        // low (0.25) + no relationship (0.0) = 0.6*0.25 + 0.4*0.0 = 0.15
        assert_eq!(blended_priority(Some("low"), 0.0), 0.15);
        // normal (0.5) + medium relationship (0.5) = 0.6*0.5 + 0.4*0.5 = 0.5
        assert_eq!(blended_priority(Some("normal"), 0.5), 0.5);
        // None (0.5) + max relationship (1.0) = 0.6*0.5 + 0.4*1.0 = 0.7
        assert_eq!(blended_priority(None, 1.0), 0.7);
    }

    // Test 3: parse_recipients
    #[test]
    fn test_parse_recipients() {
        let r = parse_recipients(r#"["alice@example.com","bob@example.com"]"#);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], "alice@example.com");
        assert_eq!(r[1], "bob@example.com");

        // Invalid JSON
        let r = parse_recipients("not json");
        assert!(r.is_empty());

        // Empty array
        let r = parse_recipients("[]");
        assert!(r.is_empty());
    }

    // Test 4: compute_scores with no messages returns empty
    #[test]
    fn test_compute_scores_no_messages() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_relationship_table(&conn);
        let _ = create_test_account(&conn);

        let scores = compute_scores(&conn, 1700000000).unwrap();
        assert!(scores.is_empty());
    }

    // Test 5: compute_scores with no accounts returns empty
    #[test]
    fn test_compute_scores_no_accounts() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_relationship_table(&conn);

        let scores = compute_scores(&conn, 1700000000).unwrap();
        assert!(scores.is_empty());
    }

    // Test 6: frequency score — contact with most emails gets 1.0
    #[test]
    fn test_frequency_score() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_relationship_table(&conn);
        let account = create_test_account(&conn);

        // alice sends 10 emails, bob sends 5
        for i in 0..10 {
            let msg = make_message(
                &account.id,
                "alice@example.com",
                "user@example.com",
                &format!("alice-{i}"),
                "INBOX",
                1700000000 + i,
                None,
            );
            InsertMessage::insert(&conn, &msg);
        }
        for i in 0..5 {
            let msg = make_message(
                &account.id,
                "bob@example.com",
                "user@example.com",
                &format!("bob-{i}"),
                "INBOX",
                1700000000 + i,
                None,
            );
            InsertMessage::insert(&conn, &msg);
        }

        let scores = compute_scores(&conn, 1700000000 + 100).unwrap();
        let alice = scores.iter().find(|s| s.email == "alice@example.com").unwrap();
        let bob = scores.iter().find(|s| s.email == "bob@example.com").unwrap();

        assert_eq!(alice.frequency_score, 1.0); // top contact
        assert_eq!(bob.frequency_score, 0.5); // 5/10
    }

    // Test 7: recency score — recent contact scores higher
    #[test]
    fn test_recency_score() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_relationship_table(&conn);
        let account = create_test_account(&conn);

        let now = 1700000000_i64;

        // alice emailed now
        let msg = make_message(
            &account.id,
            "alice@example.com",
            "user@example.com",
            "alice-recent",
            "INBOX",
            now,
            None,
        );
        InsertMessage::insert(&conn, &msg);

        // bob emailed 60 days ago
        let msg = make_message(
            &account.id,
            "bob@example.com",
            "user@example.com",
            "bob-old",
            "INBOX",
            now - 60 * 86400,
            None,
        );
        InsertMessage::insert(&conn, &msg);

        let scores = compute_scores(&conn, now).unwrap();
        let alice = scores.iter().find(|s| s.email == "alice@example.com").unwrap();
        let bob = scores.iter().find(|s| s.email == "bob@example.com").unwrap();

        // alice: recency = exp(0) = 1.0
        assert_eq!(alice.recency_score, 1.0);
        // bob: 60 days = 2 half-lives, recency = exp(-2*ln2) = 0.25
        assert_eq!(bob.recency_score, 0.25);
    }

    // Test 8: bidirectional score — both directions = higher score
    #[test]
    fn test_bidirectional_score() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_relationship_table(&conn);
        let account = create_test_account(&conn);

        let now = 1700000000;

        // alice sends 5 emails to user
        for i in 0..5 {
            let msg = make_message(
                &account.id,
                "alice@example.com",
                "user@example.com",
                &format!("alice-in-{i}"),
                "INBOX",
                now + i,
                None,
            );
            InsertMessage::insert(&conn, &msg);
        }

        // user sends 3 emails to alice (from Sent folder)
        for i in 0..3 {
            let msg = make_message(
                &account.id,
                "user@example.com",
                "alice@example.com",
                &format!("user-out-{i}"),
                "Sent",
                now + 10 + i,
                None,
            );
            InsertMessage::insert(&conn, &msg);
        }

        // bob sends 5 emails, user never replies
        for i in 0..5 {
            let msg = make_message(
                &account.id,
                "bob@example.com",
                "user@example.com",
                &format!("bob-in-{i}"),
                "INBOX",
                now + i,
                None,
            );
            InsertMessage::insert(&conn, &msg);
        }

        let scores = compute_scores(&conn, now + 20).unwrap();
        let alice = scores.iter().find(|s| s.email == "alice@example.com").unwrap();
        let bob = scores.iter().find(|s| s.email == "bob@example.com").unwrap();

        // alice: sent=3, received=5, ratio = 3/5 = 0.6
        assert_eq!(alice.bidirectional_score, 0.6);
        // bob: sent=0, received=5, bidirectional = 0
        assert_eq!(bob.bidirectional_score, 0.0);
    }

    // Test 9: thread depth score
    #[test]
    fn test_thread_depth_score() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_relationship_table(&conn);
        let account = create_test_account(&conn);

        let now = 1700000000;

        // Create a deep thread (10 messages) with alice
        for i in 0..10 {
            let from = if i % 2 == 0 {
                "alice@example.com"
            } else {
                "user@example.com"
            };
            let to = if i % 2 == 0 {
                "user@example.com"
            } else {
                "alice@example.com"
            };
            let folder = if from == "user@example.com" { "Sent" } else { "INBOX" };
            let msg = make_message(
                &account.id,
                from,
                to,
                &format!("deep-thread-{i}"),
                folder,
                now + i,
                Some("deep-thread"),
            );
            InsertMessage::insert(&conn, &msg);
        }

        // Create a shallow thread (2 messages) with bob
        for i in 0..2 {
            let from = if i == 0 {
                "bob@example.com"
            } else {
                "user@example.com"
            };
            let to = if i == 0 {
                "user@example.com"
            } else {
                "bob@example.com"
            };
            let folder = if from == "user@example.com" { "Sent" } else { "INBOX" };
            let msg = make_message(
                &account.id,
                from,
                to,
                &format!("shallow-thread-{i}"),
                folder,
                now + i,
                Some("shallow-thread"),
            );
            InsertMessage::insert(&conn, &msg);
        }

        let scores = compute_scores(&conn, now + 20).unwrap();
        let alice = scores.iter().find(|s| s.email == "alice@example.com").unwrap();
        let bob = scores.iter().find(|s| s.email == "bob@example.com").unwrap();

        // alice: avg depth = 10, normalized = (10-1)/9 = 1.0
        assert_eq!(alice.thread_depth_score, 1.0);
        // bob: avg depth = 2, normalized = (2-1)/9 ≈ 0.111
        assert_eq!(bob.thread_depth_score, 0.111);
    }

    // Test 10: single email — frequency = 1.0 since it's the only contact
    #[test]
    fn test_single_email_contact() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_relationship_table(&conn);
        let account = create_test_account(&conn);

        let now = 1700000000;
        let msg = make_message(
            &account.id,
            "solo@example.com",
            "user@example.com",
            "only-email",
            "INBOX",
            now,
            None,
        );
        InsertMessage::insert(&conn, &msg);

        let scores = compute_scores(&conn, now).unwrap();
        assert_eq!(scores.len(), 1);

        let solo = &scores[0];
        assert_eq!(solo.email, "solo@example.com");
        assert_eq!(solo.frequency_score, 1.0); // only contact = top contact
        assert_eq!(solo.recency_score, 1.0); // emailed at now
        assert_eq!(solo.bidirectional_score, 0.0); // no reply
    }

    // Test 11: all messages from one person
    #[test]
    fn test_all_from_one_person() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_relationship_table(&conn);
        let account = create_test_account(&conn);

        let now = 1700000000;
        for i in 0..20 {
            let msg = make_message(
                &account.id,
                "spammer@example.com",
                "user@example.com",
                &format!("spam-{i}"),
                "INBOX",
                now - i * 86400, // one per day going back
                None,
            );
            InsertMessage::insert(&conn, &msg);
        }

        let scores = compute_scores(&conn, now).unwrap();
        assert_eq!(scores.len(), 1);

        let spammer = &scores[0];
        assert_eq!(spammer.email, "spammer@example.com");
        assert_eq!(spammer.frequency_score, 1.0);
        assert_eq!(spammer.bidirectional_score, 0.0); // user never replies
        assert_eq!(spammer.reply_rate_score, 0.0); // no threads with user participation
    }

    // Test 12: composite score weights add up correctly
    #[test]
    fn test_composite_score_weights() {
        // If all component scores are 1.0, the composite should be 1.0
        // 0.25*1.0 + 0.25*1.0 + 0.25*1.0 + 0.15*1.0 + 0.10*1.0 = 1.0
        let score: f64 = 0.25 * 1.0 + 0.25 * 1.0 + 0.25 * 1.0 + 0.15 * 1.0 + 0.10 * 1.0;
        assert!((score - 1.0_f64).abs() < f64::EPSILON);

        // If all are 0, composite is 0
        let score: f64 = 0.25 * 0.0 + 0.25 * 0.0 + 0.25 * 0.0 + 0.15 * 0.0 + 0.10 * 0.0;
        assert!(score.abs() < f64::EPSILON);

        // Mixed: freq=0.8, recency=0.5, reply=1.0, bidir=0.6, depth=0.3
        let score: f64 = 0.25 * 0.8 + 0.25 * 0.5 + 0.25 * 1.0 + 0.15 * 0.6 + 0.10 * 0.3;
        let expected: f64 = 0.2 + 0.125 + 0.25 + 0.09 + 0.03;
        assert!((score - expected).abs() < 1e-10);
    }

    // Test 13: reply rate score
    #[test]
    fn test_reply_rate_score() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_relationship_table(&conn);
        let account = create_test_account(&conn);

        let now = 1700000000;

        // Thread 1: alice sends, user replies
        let msg = make_message(
            &account.id,
            "alice@example.com",
            "user@example.com",
            "thread1-msg1",
            "INBOX",
            now,
            Some("thread-1"),
        );
        InsertMessage::insert(&conn, &msg);
        let msg = make_message(
            &account.id,
            "user@example.com",
            "alice@example.com",
            "thread1-msg2",
            "Sent",
            now + 1,
            Some("thread-1"),
        );
        InsertMessage::insert(&conn, &msg);

        // Thread 2: alice sends, user does NOT reply
        let msg = make_message(
            &account.id,
            "alice@example.com",
            "user@example.com",
            "thread2-msg1",
            "INBOX",
            now + 2,
            Some("thread-2"),
        );
        InsertMessage::insert(&conn, &msg);

        let scores = compute_scores(&conn, now + 10).unwrap();
        let alice = scores.iter().find(|s| s.email == "alice@example.com").unwrap();

        // alice is in 2 threads, user participated in 1 of them
        // reply_rate = 1/2 = 0.5... but actually both threads have the user in thread-1
        // thread-1: alice + user participate → reply_thread += 1 for alice
        // thread-2: only alice → has_user=false → reply_thread not incremented
        // total_threads for alice = 2, reply_threads = 1, so reply_rate = 0.5
        assert_eq!(alice.reply_rate_score, 0.5);
    }

    // Test 14: user's own email is excluded from contacts
    #[test]
    fn test_user_email_excluded() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_relationship_table(&conn);
        let account = create_test_account(&conn);

        let now = 1700000000;
        // User sends email (appears in Sent folder with from=user@example.com)
        let msg = make_message(
            &account.id,
            "user@example.com",
            "someone@example.com",
            "sent-msg",
            "Sent",
            now,
            None,
        );
        InsertMessage::insert(&conn, &msg);

        let scores = compute_scores(&conn, now).unwrap();
        // user@example.com should NOT appear in scores
        assert!(scores.iter().all(|s| s.email != "user@example.com"));
        // someone@example.com should appear (as a recipient)
        let someone = scores.iter().find(|s| s.email == "someone@example.com");
        assert!(someone.is_some());
    }
}
