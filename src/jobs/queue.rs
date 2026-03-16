use rusqlite::Connection;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Payload types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiClassifyPayload {
    pub subject: String,
    pub from: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoriesStorePayload {
    pub account_id: String,
    pub rfc_message_id: Option<String>,
    pub from_name: Option<String>,
    pub from_address: Option<String>,
    pub subject: Option<String>,
    pub body_text: Option<String>,
    pub date: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSummarizePayload {
    pub session_id: String,
}

// ---------------------------------------------------------------------------
// Job row
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Job {
    pub id: i64,
    pub job_type: String,
    pub message_id: Option<String>,
    pub status: String,
    pub attempts: i64,
    pub max_attempts: i64,
    pub payload: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub next_retry_at: i64,
}

// ---------------------------------------------------------------------------
// Queue status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct QueueStatus {
    pub pending: i64,
    pub processing: i64,
    pub failed: i64,
    pub done_today: i64,
}

// ---------------------------------------------------------------------------
// Enqueue functions
// ---------------------------------------------------------------------------

/// Enqueue an AI classification job for a message.
pub fn enqueue_ai_classify(conn: &Connection, message_id: &str, subject: &str, from: &str, body: &str) {
    let payload = serde_json::to_string(&AiClassifyPayload {
        subject: subject.to_string(),
        from: from.to_string(),
        body: body.to_string(),
    })
    .unwrap_or_default();

    let r = conn.execute(
        "INSERT INTO processing_jobs (job_type, message_id, payload) VALUES ('ai_classify', ?1, ?2)",
        rusqlite::params![message_id, payload],
    );
    if r.is_ok() {
        conn.execute(
            "UPDATE messages SET ai_status = 'pending' WHERE id = ?1",
            rusqlite::params![message_id],
        )
        .ok();
    }
}

/// Enqueue a Memories storage job for a message.
pub fn enqueue_memories_store(conn: &Connection, message_id: &str, payload: &MemoriesStorePayload) {
    let payload_json = serde_json::to_string(payload).unwrap_or_default();

    let r = conn.execute(
        "INSERT INTO processing_jobs (job_type, message_id, payload) VALUES ('memories_store', ?1, ?2)",
        rusqlite::params![message_id, payload_json],
    );
    if r.is_ok() {
        conn.execute(
            "UPDATE messages SET memories_status = 'pending' WHERE id = ?1",
            rusqlite::params![message_id],
        )
        .ok();
    }
}

/// Enqueue a chat summarization job (with dedup check).
pub fn enqueue_chat_summarize(conn: &Connection, session_id: &str) {
    // Deduplicate: skip if there's already a pending/processing job for this session
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM processing_jobs WHERE job_type = 'chat_summarize' AND payload LIKE ?1 AND status IN ('pending','processing'))",
            rusqlite::params![format!("%{}%", session_id)],
            |row| row.get(0),
        )
        .unwrap_or(true);

    if exists {
        return;
    }

    let payload = serde_json::to_string(&ChatSummarizePayload {
        session_id: session_id.to_string(),
    })
    .unwrap_or_default();

    if let Err(e) = conn.execute(
        "INSERT INTO processing_jobs (job_type, payload) VALUES ('chat_summarize', ?1)",
        rusqlite::params![payload],
    ) {
        tracing::warn!("Failed to enqueue chat_summarize: {e}");
    }
}

/// Enqueue a writing style extraction job for an account (with dedup + 7-day cooldown).
pub fn enqueue_style_extract(conn: &Connection, account_id: &str) {
    // Check cooldown: skip if analyzed in last 7 days
    let last_key = format!("style_last_analyzed_{}", account_id);
    let last_run: Option<i64> = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM config WHERE key = ?1",
            rusqlite::params![last_key],
            |row| row.get(0),
        )
        .ok();

    if let Some(ts) = last_run {
        let now = chrono::Utc::now().timestamp();
        if now - ts < 7 * 86400 {
            return; // Too recent
        }
    }

    // Dedup check
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM processing_jobs WHERE job_type = 'style_extract' AND payload LIKE ?1 AND status IN ('pending','processing'))",
            rusqlite::params![format!("%{}%", account_id)],
            |row| row.get(0),
        )
        .unwrap_or(true);

    if exists {
        return;
    }

    let payload = serde_json::json!({ "account_id": account_id }).to_string();
    if let Err(e) = conn.execute(
        "INSERT INTO processing_jobs (job_type, payload) VALUES ('style_extract', ?1)",
        rusqlite::params![payload],
    ) {
        tracing::warn!("Failed to enqueue style_extract: {e}");
    }
}

/// Enqueue a preference extraction job (with dedup check).
pub fn enqueue_pref_extract(conn: &Connection) {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM processing_jobs WHERE job_type = 'pref_extract' AND status IN ('pending','processing'))",
            [],
            |row| row.get(0),
        )
        .unwrap_or(true);

    if exists {
        return;
    }

    if let Err(e) = conn.execute(
        "INSERT INTO processing_jobs (job_type) VALUES ('pref_extract')",
        [],
    ) {
        tracing::warn!("Failed to enqueue pref_extract: {e}");
    }
}

// ---------------------------------------------------------------------------
// Claim / complete / fail
// ---------------------------------------------------------------------------

/// Claim a batch of pending jobs for processing.
/// Sets status to 'processing' and returns them.
pub fn claim_batch(conn: &Connection, limit: usize) -> Vec<Job> {
    let now = chrono::Utc::now().timestamp();

    // SELECT pending jobs ready for processing
    let mut stmt = match conn.prepare(
        "SELECT id, job_type, message_id, status, attempts, max_attempts, payload, error, created_at, next_retry_at
         FROM processing_jobs
         WHERE status = 'pending' AND next_retry_at <= ?1 AND attempts < max_attempts
         ORDER BY created_at ASC
         LIMIT ?2",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let jobs: Vec<Job> = stmt
        .query_map(rusqlite::params![now, limit as i64], |row| {
            Ok(Job {
                id: row.get(0)?,
                job_type: row.get(1)?,
                message_id: row.get(2)?,
                status: row.get(3)?,
                attempts: row.get(4)?,
                max_attempts: row.get(5)?,
                payload: row.get(6)?,
                error: row.get(7)?,
                created_at: row.get(8)?,
                next_retry_at: row.get(9)?,
            })
        })
        .ok()
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    // Mark claimed jobs as 'processing' in a single batch UPDATE
    if !jobs.is_empty() {
        let placeholders: Vec<String> = (1..=jobs.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "UPDATE processing_jobs SET status = 'processing', attempts = attempts + 1, updated_at = unixepoch() WHERE id IN ({})",
            placeholders.join(", ")
        );
        let ids: Vec<rusqlite::types::Value> = jobs.iter().map(|j| rusqlite::types::Value::Integer(j.id)).collect();
        let params: Vec<&dyn rusqlite::types::ToSql> = ids.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
        conn.execute(&sql, params.as_slice()).ok();
    }

    jobs
}

/// Mark a job as completed.
pub fn complete_job(conn: &Connection, job_id: i64, job_type: &str, message_id: Option<&str>) {
    conn.execute(
        "UPDATE processing_jobs SET status = 'done', updated_at = unixepoch() WHERE id = ?1",
        rusqlite::params![job_id],
    )
    .ok();

    // Update message status columns
    if let Some(msg_id) = message_id {
        let col = match job_type {
            "ai_classify" => Some("ai_status"),
            "memories_store" => Some("memories_status"),
            _ => None,
        };
        if let Some(col) = col {
            conn.execute(
                &format!("UPDATE messages SET {} = 'done' WHERE id = ?1", col),
                rusqlite::params![msg_id],
            )
            .ok();
        }
    }
}

/// Mark a job as failed, with exponential backoff for retry.
pub fn fail_job(conn: &Connection, job_id: i64, job_type: &str, message_id: Option<&str>, error: &str, attempts: i64, max_attempts: i64) {
    if attempts >= max_attempts {
        // Permanently failed
        conn.execute(
            "UPDATE processing_jobs SET status = 'failed', error = ?1, updated_at = unixepoch() WHERE id = ?2",
            rusqlite::params![error, job_id],
        )
        .ok();

        // Update message status to 'failed'
        if let Some(msg_id) = message_id {
            let col = match job_type {
                "ai_classify" => Some("ai_status"),
                "memories_store" => Some("memories_status"),
                _ => None,
            };
            if let Some(col) = col {
                conn.execute(
                    &format!("UPDATE messages SET {} = 'failed' WHERE id = ?1", col),
                    rusqlite::params![msg_id],
                )
                .ok();
            }
        }
    } else {
        // Retry with exponential backoff: attempts^2 * 5 seconds
        let backoff = attempts * attempts * 5;
        let next_retry = chrono::Utc::now().timestamp() + backoff;
        conn.execute(
            "UPDATE processing_jobs SET status = 'pending', error = ?1, next_retry_at = ?2, updated_at = unixepoch() WHERE id = ?3",
            rusqlite::params![error, next_retry, job_id],
        )
        .ok();
    }
}

// ---------------------------------------------------------------------------
// Status / cleanup
// ---------------------------------------------------------------------------

/// Get queue status counts — single table scan instead of 4 separate queries.
pub fn get_queue_status(conn: &Connection) -> QueueStatus {
    let today_start = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();

    let (pending, processing, failed, done_today) = conn
        .query_row(
            "SELECT \
                SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), \
                SUM(CASE WHEN status = 'processing' THEN 1 ELSE 0 END), \
                SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), \
                SUM(CASE WHEN status = 'done' AND updated_at >= ?1 THEN 1 ELSE 0 END) \
             FROM processing_jobs",
            rusqlite::params![today_start],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                ))
            },
        )
        .unwrap_or((0, 0, 0, 0));

    QueueStatus {
        pending,
        processing,
        failed,
        done_today,
    }
}

/// Delete completed jobs older than N days.
pub fn cleanup_completed(conn: &Connection, days: i64) {
    let cutoff = chrono::Utc::now().timestamp() - (days * 86400);
    conn.execute(
        "DELETE FROM processing_jobs WHERE status = 'done' AND updated_at < ?1",
        rusqlite::params![cutoff],
    )
    .ok();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // Create minimal schema for testing
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL DEFAULT (unixepoch()));
             CREATE TABLE IF NOT EXISTS messages (
                 id TEXT PRIMARY KEY,
                 ai_status TEXT DEFAULT NULL,
                 memories_status TEXT DEFAULT NULL
             );
             CREATE TABLE IF NOT EXISTS processing_jobs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 job_type TEXT NOT NULL CHECK(job_type IN ('ai_classify','memories_store','chat_summarize','pref_extract','entity_extract','style_extract','auto_draft')),
                 message_id TEXT,
                 status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','processing','done','failed')),
                 attempts INTEGER NOT NULL DEFAULT 0,
                 max_attempts INTEGER NOT NULL DEFAULT 4,
                 payload TEXT,
                 error TEXT,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 next_retry_at INTEGER NOT NULL DEFAULT (unixepoch())
             );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_enqueue_ai_classify() {
        let conn = setup_db();
        conn.execute("INSERT INTO messages (id) VALUES ('msg1')", []).unwrap();
        enqueue_ai_classify(&conn, "msg1", "Test Subject", "alice@test.com", "Hello world");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM processing_jobs WHERE job_type = 'ai_classify'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        let status: String = conn
            .query_row("SELECT ai_status FROM messages WHERE id = 'msg1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "pending");
    }

    #[test]
    fn test_enqueue_memories_store() {
        let conn = setup_db();
        conn.execute("INSERT INTO messages (id) VALUES ('msg2')", []).unwrap();
        let payload = MemoriesStorePayload {
            account_id: "acc1".to_string(),
            rfc_message_id: Some("<msg@test.com>".to_string()),
            from_name: Some("Bob".to_string()),
            from_address: Some("bob@test.com".to_string()),
            subject: Some("Test".to_string()),
            body_text: Some("Body".to_string()),
            date: Some(1000000),
        };
        enqueue_memories_store(&conn, "msg2", &payload);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM processing_jobs WHERE job_type = 'memories_store'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        let status: String = conn
            .query_row("SELECT memories_status FROM messages WHERE id = 'msg2'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "pending");
    }

    #[test]
    fn test_enqueue_chat_summarize_dedup() {
        let conn = setup_db();
        enqueue_chat_summarize(&conn, "session-abc");
        enqueue_chat_summarize(&conn, "session-abc"); // duplicate, should be skipped

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM processing_jobs WHERE job_type = 'chat_summarize'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_enqueue_pref_extract_dedup() {
        let conn = setup_db();
        enqueue_pref_extract(&conn);
        enqueue_pref_extract(&conn); // duplicate

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM processing_jobs WHERE job_type = 'pref_extract'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_claim_batch() {
        let conn = setup_db();
        conn.execute("INSERT INTO messages (id) VALUES ('msg3')", []).unwrap();
        enqueue_ai_classify(&conn, "msg3", "Sub", "from@test.com", "body");

        let jobs = claim_batch(&conn, 10);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].job_type, "ai_classify");

        // Job should now be 'processing'
        let status: String = conn
            .query_row("SELECT status FROM processing_jobs WHERE id = ?1", [jobs[0].id], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "processing");
    }

    #[test]
    fn test_complete_job() {
        let conn = setup_db();
        conn.execute("INSERT INTO messages (id) VALUES ('msg4')", []).unwrap();
        enqueue_ai_classify(&conn, "msg4", "Sub", "f@t.com", "b");

        let jobs = claim_batch(&conn, 10);
        assert_eq!(jobs.len(), 1);

        complete_job(&conn, jobs[0].id, "ai_classify", Some("msg4"));

        let status: String = conn
            .query_row("SELECT status FROM processing_jobs WHERE id = ?1", [jobs[0].id], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "done");

        let ai_status: String = conn
            .query_row("SELECT ai_status FROM messages WHERE id = 'msg4'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(ai_status, "done");
    }

    #[test]
    fn test_fail_job_retry() {
        let conn = setup_db();
        conn.execute("INSERT INTO messages (id) VALUES ('msg5')", []).unwrap();
        enqueue_ai_classify(&conn, "msg5", "Sub", "f@t.com", "b");

        let jobs = claim_batch(&conn, 10);
        fail_job(&conn, jobs[0].id, "ai_classify", Some("msg5"), "timeout", 1, 4);

        let status: String = conn
            .query_row("SELECT status FROM processing_jobs WHERE id = ?1", [jobs[0].id], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "pending"); // retryable
    }

    #[test]
    fn test_fail_job_permanent() {
        let conn = setup_db();
        conn.execute("INSERT INTO messages (id) VALUES ('msg6')", []).unwrap();
        enqueue_ai_classify(&conn, "msg6", "Sub", "f@t.com", "b");

        let jobs = claim_batch(&conn, 10);
        fail_job(&conn, jobs[0].id, "ai_classify", Some("msg6"), "max retries", 4, 4);

        let status: String = conn
            .query_row("SELECT status FROM processing_jobs WHERE id = ?1", [jobs[0].id], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "failed");

        // Message ai_status should also be 'failed'
        let ai_status: String = conn
            .query_row("SELECT ai_status FROM messages WHERE id = 'msg6'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(ai_status, "failed");
    }

    #[test]
    fn test_get_queue_status() {
        let conn = setup_db();
        conn.execute("INSERT INTO messages (id) VALUES ('msg7')", []).unwrap();
        enqueue_ai_classify(&conn, "msg7", "Sub", "f@t.com", "b");

        let status = get_queue_status(&conn);
        assert_eq!(status.pending, 1);
        assert_eq!(status.processing, 0);
        assert_eq!(status.failed, 0);
    }

}
