use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Row;
use serde::{Deserialize, Serialize};

type Conn = PooledConnection<SqliteConnectionManager>;

/// Attachment metadata stored as JSON in the attachment_names column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub filename: String,
    pub mime_type: String,
    pub size: usize,
}

/// Lightweight message struct for inbox list views (no body fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    pub id: String,
    pub account_id: String,
    pub thread_id: Option<String>,
    pub folder: String,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub date: Option<i64>,
    pub is_read: bool,
    pub is_starred: bool,
    pub has_attachments: bool,
    pub labels: Option<String>,
    pub ai_priority_label: Option<String>,
    pub ai_category: Option<String>,
    pub ai_sentiment: Option<String>,
    pub ai_needs_reply: bool,
    pub intent: Option<String>,
}

impl MessageSummary {
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            account_id: row.get("account_id")?,
            thread_id: row.get("thread_id")?,
            folder: row.get("folder")?,
            from_address: row.get("from_address")?,
            from_name: row.get("from_name")?,
            subject: row.get("subject")?,
            snippet: row.get("snippet")?,
            date: row.get("date")?,
            is_read: row.get("is_read")?,
            is_starred: row.get("is_starred")?,
            has_attachments: row.get("has_attachments")?,
            labels: row.get("labels")?,
            ai_priority_label: row.get("ai_priority_label")?,
            ai_category: row.get("ai_category")?,
            ai_sentiment: row.get("ai_sentiment")?,
            ai_needs_reply: row.get::<_, Option<bool>>("ai_needs_reply")?.unwrap_or(false),
            intent: row.get("intent")?,
        })
    }

    /// List messages in a folder, paginated, ordered by date DESC.
    /// Only returns non-deleted messages.
    pub fn list_by_folder(
        conn: &Conn,
        account_id: &str,
        folder: &str,
        limit: i64,
        offset: i64,
    ) -> Vec<Self> {
        let mut stmt = conn
            .prepare(
                "SELECT id, account_id, thread_id, folder, from_address, from_name, subject, snippet,
                        date, is_read, is_starred, has_attachments, labels, ai_priority_label, ai_category, ai_sentiment, ai_needs_reply, intent
                 FROM messages
                 WHERE account_id = ?1 AND folder = ?2 AND is_deleted = 0
                 ORDER BY date DESC
                 LIMIT ?3 OFFSET ?4",
            )
            .expect("failed to prepare list_by_folder query");

        stmt.query_map(rusqlite::params![account_id, folder, limit, offset], Self::from_row)
            .expect("failed to query messages")
            .filter_map(|r| r.map_err(|e| tracing::warn!("Message row skip: {e}")).ok())
            .collect()
    }
}

/// Full message detail including body, used for thread/detail views.
#[derive(Debug, Clone, Serialize)]
pub struct MessageDetail {
    pub id: String,
    pub message_id: Option<String>,
    pub account_id: String,
    pub thread_id: Option<String>,
    pub folder: String,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub to_addresses: Option<String>,
    pub cc_addresses: Option<String>,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub date: Option<i64>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub has_attachments: bool,
    pub attachments: Vec<AttachmentMeta>,
    pub ai_intent: Option<String>,
    pub ai_priority_score: Option<f64>,
    pub ai_priority_label: Option<String>,
    pub ai_category: Option<String>,
    pub ai_summary: Option<String>,
    pub ai_sentiment: Option<String>,
    pub ai_needs_reply: bool,
    pub list_unsubscribe: Option<String>,
    pub list_unsubscribe_post: bool,
}

impl MessageDetail {
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        let attachment_json: Option<String> = row.get("attachment_names")?;
        let attachments: Vec<AttachmentMeta> = attachment_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        Ok(Self {
            id: row.get("id")?,
            message_id: row.get("message_id")?,
            account_id: row.get("account_id")?,
            thread_id: row.get("thread_id")?,
            folder: row.get("folder")?,
            from_address: row.get("from_address")?,
            from_name: row.get("from_name")?,
            to_addresses: row.get("to_addresses")?,
            cc_addresses: row.get("cc_addresses")?,
            subject: row.get("subject")?,
            snippet: row.get("snippet")?,
            date: row.get("date")?,
            body_text: row.get("body_text")?,
            body_html: row.get("body_html")?,
            is_read: row.get("is_read")?,
            is_starred: row.get("is_starred")?,
            has_attachments: row.get("has_attachments")?,
            attachments,
            ai_intent: row.get("ai_intent")?,
            ai_priority_score: row.get("ai_priority_score")?,
            ai_priority_label: row.get("ai_priority_label")?,
            ai_category: row.get("ai_category")?,
            ai_summary: row.get("ai_summary")?,
            ai_sentiment: row.get("ai_sentiment")?,
            ai_needs_reply: row.get::<_, Option<bool>>("ai_needs_reply")?.unwrap_or(false),
            list_unsubscribe: row.get("list_unsubscribe")?,
            list_unsubscribe_post: row.get::<_, bool>("list_unsubscribe_post").unwrap_or(false),
        })
    }

    pub fn get_by_id(conn: &Conn, id: &str) -> Option<Self> {
        conn.query_row(
            "SELECT id, message_id, account_id, thread_id, folder, from_address, from_name,
                    to_addresses, cc_addresses, subject, snippet, date,
                    body_text, body_html, is_read, is_starred, has_attachments, attachment_names,
                    ai_intent, ai_priority_score, ai_priority_label, ai_category, ai_summary, ai_sentiment, ai_needs_reply,
                    list_unsubscribe, list_unsubscribe_post
             FROM messages WHERE id = ?1 AND is_deleted = 0",
            rusqlite::params![id],
            Self::from_row,
        )
        .ok()
    }

    pub fn list_by_thread(conn: &Conn, thread_id: &str) -> Vec<Self> {
        // Deduplicate thread messages in two stages:
        // 1. Exclude NULL message_id rows when a matching row (same sender+subject, date within 60s) exists with a real message_id
        // 2. Dedup across folders (INBOX vs Sent) by message_id, preferring INBOX
        let mut stmt = conn
            .prepare(
                "WITH cleaned AS (
                    SELECT * FROM messages
                    WHERE thread_id = ?1 AND is_deleted = 0
                    AND NOT (
                        message_id IS NULL
                        AND EXISTS (
                            SELECT 1 FROM messages m2
                            WHERE m2.thread_id = ?1
                            AND m2.is_deleted = 0
                            AND m2.message_id IS NOT NULL
                            AND m2.from_address = messages.from_address
                            AND m2.subject = messages.subject
                            AND ABS(m2.date - messages.date) <= 60
                        )
                    )
                ),
                ranked AS (
                    SELECT *, ROW_NUMBER() OVER (
                        PARTITION BY COALESCE(message_id, id)
                        ORDER BY CASE WHEN folder = 'INBOX' THEN 0 ELSE 1 END, date ASC
                    ) as rn
                    FROM cleaned
                )
                SELECT id, message_id, account_id, thread_id, folder, from_address, from_name,
                        to_addresses, cc_addresses, subject, snippet, date,
                        body_text, body_html, is_read, is_starred, has_attachments, attachment_names,
                        ai_intent, ai_priority_score, ai_priority_label, ai_category, ai_summary, ai_sentiment, ai_needs_reply,
                        list_unsubscribe, list_unsubscribe_post
                 FROM ranked WHERE rn = 1
                 ORDER BY date ASC",
            )
            .expect("failed to prepare list_by_thread");

        stmt.query_map(rusqlite::params![thread_id], Self::from_row)
            .expect("failed to query thread messages")
            .filter_map(|r| r.map_err(|e| tracing::warn!("Thread row skip: {e}")).ok())
            .collect()
    }
}

/// Mark a message as read. Returns true if a row was updated.
pub fn mark_as_read(conn: &Conn, id: &str) -> bool {
    conn.execute(
        "UPDATE messages SET is_read = 1, updated_at = datetime('now') WHERE id = ?1",
        rusqlite::params![id],
    )
    .map(|rows| rows > 0)
    .unwrap_or(false)
}

/// Struct for inserting synced messages from IMAP.
#[derive(Debug, Clone, Deserialize)]
pub struct InsertMessage {
    pub account_id: String,
    pub message_id: Option<String>,
    pub thread_id: Option<String>,
    pub folder: String,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub to_addresses: Option<String>,
    pub cc_addresses: Option<String>,
    pub bcc_addresses: Option<String>,
    pub subject: Option<String>,
    pub date: Option<i64>,
    pub snippet: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_draft: bool,
    pub labels: Option<String>,
    pub uid: Option<i64>,
    pub modseq: Option<i64>,
    pub raw_headers: Option<String>,
    pub has_attachments: bool,
    pub attachment_names: Option<String>,
    pub size_bytes: Option<i64>,
    pub list_unsubscribe: Option<String>,
    pub list_unsubscribe_post: bool,
}

impl InsertMessage {
    /// Resolve thread_id by chaining: if our thread_id matches an existing
    /// message's message_id, adopt that message's thread_id instead.
    /// This handles cases where References header points to a reply rather
    /// than the thread root (common with Gmail Sent folder copies).
    fn resolve_thread_id(conn: &Conn, thread_id: &Option<String>) -> Option<String> {
        let tid = thread_id.as_deref()?;
        // Wrap in angle brackets for matching against message_id which stores them
        let bracketed = if tid.starts_with('<') { tid.to_string() } else { format!("<{}>", tid) };
        let existing: Option<String> = conn
            .query_row(
                "SELECT thread_id FROM messages WHERE message_id = ?1 LIMIT 1",
                rusqlite::params![bracketed],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        // If the existing message has a different thread_id, use it (it's closer to the root)
        if let Some(ref resolved) = existing {
            if resolved != tid {
                return Some(resolved.clone());
            }
        }
        Some(tid.to_string())
    }

    /// Insert a message into the database.
    /// Uses INSERT OR IGNORE to handle duplicates (unique on account_id+message_id+folder).
    /// Returns Some(id) if inserted, None if duplicate was skipped.
    pub fn insert(conn: &Conn, msg: &InsertMessage) -> Option<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let resolved_thread_id = Self::resolve_thread_id(conn, &msg.thread_id);
        let rows = conn.execute(
            "INSERT OR IGNORE INTO messages (
                id, account_id, message_id, thread_id, folder,
                from_address, from_name, to_addresses, cc_addresses, bcc_addresses,
                subject, date, snippet, body_text, body_html,
                is_read, is_starred, is_draft, labels,
                uid, modseq, raw_headers,
                has_attachments, attachment_names, size_bytes,
                list_unsubscribe, list_unsubscribe_post
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15,
                ?16, ?17, ?18, ?19,
                ?20, ?21, ?22,
                ?23, ?24, ?25,
                ?26, ?27
            )",
            rusqlite::params![
                id,
                msg.account_id,
                msg.message_id,
                resolved_thread_id,
                msg.folder,
                msg.from_address,
                msg.from_name,
                msg.to_addresses,
                msg.cc_addresses,
                msg.bcc_addresses,
                msg.subject,
                msg.date,
                msg.snippet,
                msg.body_text,
                msg.body_html,
                msg.is_read,
                msg.is_starred,
                msg.is_draft,
                msg.labels,
                msg.uid,
                msg.modseq,
                msg.raw_headers,
                msg.has_attachments,
                msg.attachment_names,
                msg.size_bytes,
                msg.list_unsubscribe,
                msg.list_unsubscribe_post,
            ],
        )
        .expect("failed to insert message");

        if rows > 0 { Some(id) } else { None }
    }
}

/// Count unread messages in a folder for an account.
pub fn unread_count(conn: &Conn, account_id: &str, folder: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND folder = ?2 AND is_read = 0 AND is_deleted = 0",
        rusqlite::params![account_id, folder],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

/// Update AI metadata columns on a message.
pub fn update_ai_metadata(
    conn: &Conn,
    id: &str,
    intent: &str,
    priority_score: f64,
    priority_label: &str,
    category: &str,
    summary: &str,
    entities: Option<&str>,
    deadline: Option<&str>,
    sentiment: Option<&str>,
    needs_reply: bool,
) -> bool {
    let updated = conn
        .execute(
            "UPDATE messages SET
                ai_intent = ?2,
                ai_priority_score = ?3,
                ai_priority_label = ?4,
                ai_category = ?5,
                ai_summary = ?6,
                ai_entities = ?7,
                ai_deadline = ?8,
                ai_sentiment = ?9,
                ai_needs_reply = ?10,
                updated_at = unixepoch()
             WHERE id = ?1",
            rusqlite::params![id, intent, priority_score, priority_label, category, summary, entities, deadline, sentiment, needs_reply],
        )
        .unwrap_or(0);
    updated > 0
}

/// List messages that need a reply, paginated, ordered by date DESC.
/// Returns non-deleted, unread messages where ai_needs_reply = 1.
pub fn list_needs_reply(
    conn: &Conn,
    account_id: Option<&str>,
    limit: i64,
    offset: i64,
) -> (Vec<MessageSummary>, i64) {
    let (where_clause, count_query) = if let Some(_) = account_id {
        (
            "WHERE ai_needs_reply = 1 AND is_read = 0 AND is_deleted = 0 AND account_id = ?1",
            "SELECT COUNT(*) FROM messages WHERE ai_needs_reply = 1 AND is_read = 0 AND is_deleted = 0 AND account_id = ?1",
        )
    } else {
        (
            "WHERE ai_needs_reply = 1 AND is_read = 0 AND is_deleted = 0",
            "SELECT COUNT(*) FROM messages WHERE ai_needs_reply = 1 AND is_read = 0 AND is_deleted = 0",
        )
    };

    let total: i64 = if let Some(aid) = account_id {
        conn.query_row(count_query, rusqlite::params![aid], |row| row.get(0))
            .unwrap_or(0)
    } else {
        conn.query_row(count_query, [], |row| row.get(0))
            .unwrap_or(0)
    };

    let query = format!(
        "SELECT id, account_id, thread_id, folder, from_address, from_name, subject, snippet,
                date, is_read, is_starred, has_attachments, labels, ai_priority_label, ai_category, ai_sentiment, ai_needs_reply, intent
         FROM messages
         {where_clause}
         ORDER BY date DESC
         LIMIT ?{} OFFSET ?{}",
        if account_id.is_some() { "2" } else { "1" },
        if account_id.is_some() { "3" } else { "2" },
    );

    let mut stmt = conn
        .prepare(&query)
        .expect("failed to prepare list_needs_reply query");

    let messages: Vec<MessageSummary> = if let Some(aid) = account_id {
        stmt.query_map(rusqlite::params![aid, limit, offset], MessageSummary::from_row)
    } else {
        stmt.query_map(rusqlite::params![limit, offset], MessageSummary::from_row)
    }
        .expect("failed to query needs_reply messages")
        .filter_map(|r| r.map_err(|e| tracing::warn!("NeedsReply row skip: {e}")).ok())
        .collect();

    (messages, total)
}

/// Save a draft message. If `draft_id` is Some, update existing; otherwise create new.
/// Returns the draft's message ID.
pub fn save_draft(
    conn: &Conn,
    draft_id: Option<&str>,
    account_id: &str,
    to_addresses: Option<&str>,
    cc_addresses: Option<&str>,
    bcc_addresses: Option<&str>,
    subject: Option<&str>,
    body_text: &str,
    body_html: Option<&str>,
) -> String {
    if let Some(id) = draft_id {
        // Update existing draft (scoped to account_id for ownership verification)
        conn.execute(
            "UPDATE messages SET
                to_addresses = ?1, cc_addresses = ?2, bcc_addresses = ?3,
                subject = ?4, body_text = ?5, body_html = ?6,
                snippet = ?7, updated_at = unixepoch()
             WHERE id = ?8 AND account_id = ?9 AND is_draft = 1",
            rusqlite::params![
                to_addresses,
                cc_addresses,
                bcc_addresses,
                subject,
                body_text,
                body_html,
                &body_text.chars().take(200).collect::<String>(),
                id,
                account_id,
            ],
        )
        .expect("failed to update draft");
        id.to_string()
    } else {
        // Create new draft
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO messages (
                id, account_id, folder, to_addresses, cc_addresses, bcc_addresses,
                subject, body_text, body_html, snippet,
                is_read, is_starred, is_draft, date, from_address, from_name, has_attachments
            ) VALUES (
                ?1, ?2, 'Drafts', ?3, ?4, ?5,
                ?6, ?7, ?8, ?9,
                1, 0, 1, unixepoch(), NULL, NULL, 0
            )",
            rusqlite::params![
                id,
                account_id,
                to_addresses,
                cc_addresses,
                bcc_addresses,
                subject,
                body_text,
                body_html,
                &body_text.chars().take(200).collect::<String>(),
            ],
        )
        .expect("failed to insert draft");
        id
    }
}

/// List drafts for an account, ordered by most recently updated.
pub fn list_drafts(conn: &Conn, account_id: &str) -> Vec<MessageSummary> {
    let mut stmt = conn
        .prepare(
            "SELECT id, account_id, thread_id, folder, from_address, from_name, subject, snippet,
                    date, is_read, is_starred, has_attachments, labels, ai_priority_label, ai_category, ai_sentiment, ai_needs_reply, intent
             FROM messages
             WHERE account_id = ?1 AND is_draft = 1 AND is_deleted = 0
             ORDER BY updated_at DESC
             LIMIT 100",
        )
        .expect("failed to prepare list_drafts query");

    stmt.query_map(rusqlite::params![account_id], MessageSummary::from_row)
        .expect("failed to query drafts")
        .filter_map(|r| r.map_err(|e| tracing::warn!("Draft row skip: {e}")).ok())
        .collect()
}

/// Soft-delete a draft. Returns true if a row was updated.
pub fn delete_draft(conn: &Conn, id: &str) -> bool {
    conn.execute(
        "UPDATE messages SET is_deleted = 1, updated_at = unixepoch() WHERE id = ?1 AND is_draft = 1",
        rusqlite::params![id],
    )
    .map(|rows| rows > 0)
    .unwrap_or(false)
}

/// Decay priority scores for messages older than `threshold_days` with no recent activity.
/// Only decays messages with ai_priority_score > 0.3 (i.e., above "low").
/// Returns the number of messages updated.
pub fn decay_priority_scores(conn: &Conn, threshold_days: i64, decay_factor: f64) -> usize {
    let cutoff = chrono::Utc::now().timestamp() - (threshold_days * 86400);

    conn.execute(
        "UPDATE messages SET
            ai_priority_score = MAX(0.1, ai_priority_score * ?1),
            ai_priority_label = CASE
                WHEN ai_priority_score * ?1 >= 0.8 THEN 'urgent'
                WHEN ai_priority_score * ?1 >= 0.6 THEN 'high'
                WHEN ai_priority_score * ?1 >= 0.3 THEN 'normal'
                ELSE 'low'
            END,
            updated_at = unixepoch()
        WHERE ai_priority_score > 0.3
            AND date < ?2
            AND is_deleted = 0",
        rusqlite::params![decay_factor, cutoff],
    )
    .unwrap_or(0)
}

/// Convert a draft to a sent message (mark as not-draft, move to Sent folder).
pub fn finalize_draft_as_sent(conn: &Conn, id: &str, message_id: Option<&str>, thread_id: Option<&str>) -> bool {
    conn.execute(
        "UPDATE messages SET
            is_draft = 0, folder = 'Sent', is_read = 1,
            message_id = ?1, thread_id = ?2,
            date = unixepoch(), updated_at = unixepoch()
         WHERE id = ?3",
        rusqlite::params![message_id, thread_id, id],
    )
    .map(|rows| rows > 0)
    .unwrap_or(false)
}

/// Set snooze on messages. Returns count of rows affected.
pub fn snooze_messages(conn: &Conn, ids: &[&str], snooze_until: i64) -> usize {
    if ids.is_empty() {
        return 0;
    }

    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{}", i + 1)).collect();
    let in_clause = placeholders.join(", ");

    let sql = format!(
        "UPDATE messages SET snoozed_until = ?1, updated_at = unixepoch() WHERE id IN ({in_clause})"
    );

    let mut params: Vec<&dyn rusqlite::types::ToSql> = Vec::with_capacity(ids.len() + 1);
    params.push(&snooze_until);
    for id in ids {
        params.push(id as &dyn rusqlite::types::ToSql);
    }

    conn.execute(&sql, params.as_slice()).unwrap_or(0)
}

/// Clear snooze on messages. Returns count of rows affected.
pub fn unsnooze_messages(conn: &Conn, ids: &[&str]) -> usize {
    if ids.is_empty() {
        return 0;
    }

    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
    let in_clause = placeholders.join(", ");

    let sql = format!(
        "UPDATE messages SET snoozed_until = NULL, updated_at = unixepoch() WHERE id IN ({in_clause})"
    );

    let params: Vec<&dyn rusqlite::types::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();
    conn.execute(&sql, params.as_slice()).unwrap_or(0)
}

/// List currently snoozed messages (snoozed_until > now).
pub fn list_snoozed(conn: &Conn) -> Vec<MessageSummary> {
    let mut stmt = conn
        .prepare(
            "SELECT id, account_id, thread_id, folder, from_address, from_name, subject, snippet,
                    date, is_read, is_starred, has_attachments, labels, ai_priority_label, ai_category, ai_sentiment, ai_needs_reply
             FROM messages
             WHERE snoozed_until IS NOT NULL AND snoozed_until > unixepoch() AND is_deleted = 0
             ORDER BY snoozed_until ASC
             LIMIT 500",
        )
        .expect("failed to prepare list_snoozed query");

    stmt.query_map([], MessageSummary::from_row)
        .expect("failed to query snoozed messages")
        .filter_map(|r| r.map_err(|e| tracing::warn!("Snoozed row skip: {e}")).ok())
        .collect()
}

/// Wake up snoozed messages whose snooze time has passed.
/// Returns count of messages unsnoozed.
pub fn wake_snoozed(conn: &Conn) -> usize {
    conn.execute(
        "UPDATE messages SET snoozed_until = NULL, updated_at = unixepoch()
         WHERE snoozed_until IS NOT NULL AND snoozed_until <= unixepoch()",
        [],
    )
    .unwrap_or(0)
}

/// Batch update messages by action. Returns count of rows affected.
/// Supported actions: archive, delete, mark_read, mark_unread, star, unstar, spam.
pub fn batch_update(conn: &Conn, ids: &[&str], action: &str) -> usize {
    if ids.is_empty() {
        return 0;
    }

    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
    let in_clause = placeholders.join(", ");

    let sql = match action {
        "archive" => format!(
            "UPDATE messages SET folder = 'Archive', updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        "delete" => format!(
            "UPDATE messages SET is_deleted = 1, updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        "mark_read" => format!(
            "UPDATE messages SET is_read = 1, updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        "mark_unread" => format!(
            "UPDATE messages SET is_read = 0, updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        "star" => format!(
            "UPDATE messages SET is_starred = 1, updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        "unstar" => format!(
            "UPDATE messages SET is_starred = 0, updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        "spam" => format!(
            "UPDATE messages SET folder = 'Spam', updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        _ => return 0,
    };

    let params: Vec<&dyn rusqlite::types::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();
    conn.execute(&sql, params.as_slice())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};

    fn create_test_account(conn: &Conn) -> Account {
        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "msg-test@example.com".to_string(),
            display_name: Some("Message Test".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("msg-test@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn make_insert_message(account_id: &str, folder: &str, subject: &str, is_read: bool) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{subject}@example.com>")),
            thread_id: None,
            folder: folder.to_string(),
            from_address: Some("sender@example.com".to_string()),
            from_name: Some("Sender".to_string()),
            to_addresses: Some(r#"["msg-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(1700000000),
            snippet: Some("Preview text...".to_string()),
            body_text: Some("Full body text".to_string()),
            body_html: None,
            is_read,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(1),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(1024),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    #[test]
    fn test_insert_message_and_list_by_folder() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account = create_test_account(&conn);

        let msg1 = make_insert_message(&account.id, "INBOX", "Hello World", false);
        let msg2 = make_insert_message(&account.id, "INBOX", "Second Email", true);
        let msg3 = make_insert_message(&account.id, "Sent", "Sent Message", true);

        let id1 = InsertMessage::insert(&conn, &msg1).unwrap();
        let id2 = InsertMessage::insert(&conn, &msg2).unwrap();
        let _id3 = InsertMessage::insert(&conn, &msg3);

        assert!(!id1.is_empty());
        assert!(!id2.is_empty());

        let inbox = MessageSummary::list_by_folder(&conn, &account.id, "INBOX", 50, 0);
        assert_eq!(inbox.len(), 2);

        let sent = MessageSummary::list_by_folder(&conn, &account.id, "Sent", 50, 0);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].subject.as_deref(), Some("Sent Message"));
    }

    #[test]
    fn test_unread_count() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account = create_test_account(&conn);

        let msg1 = make_insert_message(&account.id, "INBOX", "Unread 1", false);
        let msg2 = make_insert_message(&account.id, "INBOX", "Unread 2", false);
        let msg3 = make_insert_message(&account.id, "INBOX", "Read 1", true);

        InsertMessage::insert(&conn, &msg1);
        InsertMessage::insert(&conn, &msg2);
        InsertMessage::insert(&conn, &msg3);

        let count = unread_count(&conn, &account.id, "INBOX");
        assert_eq!(count, 2);

        let sent_count = unread_count(&conn, &account.id, "Sent");
        assert_eq!(sent_count, 0);
    }

    #[test]
    fn test_pagination() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account = create_test_account(&conn);

        for i in 0..5 {
            let mut msg = make_insert_message(&account.id, "INBOX", &format!("Email {i}"), false);
            msg.date = Some(1700000000 + i);
            msg.message_id = Some(format!("<email-{i}@example.com>"));
            msg.uid = Some(i + 1);
            InsertMessage::insert(&conn, &msg);
        }

        let page1 = MessageSummary::list_by_folder(&conn, &account.id, "INBOX", 2, 0);
        assert_eq!(page1.len(), 2);

        let page2 = MessageSummary::list_by_folder(&conn, &account.id, "INBOX", 2, 2);
        assert_eq!(page2.len(), 2);

        let page3 = MessageSummary::list_by_folder(&conn, &account.id, "INBOX", 2, 4);
        assert_eq!(page3.len(), 1);
    }

    #[test]
    fn test_message_detail_get_by_id() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg = make_insert_message(&account.id, "INBOX", "Detail Test", false);
        msg.body_text = Some("Full body text here.".to_string());
        msg.body_html = Some("<p>Full body HTML</p>".to_string());
        let id = InsertMessage::insert(&conn, &msg).unwrap();

        let detail = MessageDetail::get_by_id(&conn, &id);
        assert!(detail.is_some());
        let detail = detail.unwrap();
        assert_eq!(detail.id, id);
        assert_eq!(detail.body_text.as_deref(), Some("Full body text here."));
        assert_eq!(detail.body_html.as_deref(), Some("<p>Full body HTML</p>"));
    }

    #[test]
    fn test_message_detail_list_by_thread() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let thread_id = "thread-abc@example.com";
        for i in 0..3 {
            let mut msg = make_insert_message(&account.id, "INBOX", &format!("Thread msg {i}"), false);
            msg.thread_id = Some(thread_id.to_string());
            msg.message_id = Some(format!("<thread-msg-{i}@example.com>"));
            msg.date = Some(1700000000 + i);
            msg.uid = Some(100 + i);
            InsertMessage::insert(&conn, &msg);
        }

        let mut other = make_insert_message(&account.id, "INBOX", "Other thread", false);
        other.thread_id = Some("other-thread@example.com".to_string());
        other.message_id = Some("<other@example.com>".to_string());
        other.uid = Some(200);
        InsertMessage::insert(&conn, &other);

        let thread = MessageDetail::list_by_thread(&conn, thread_id);
        assert_eq!(thread.len(), 3);
        assert_eq!(thread[0].subject.as_deref(), Some("Thread msg 0"));
        assert_eq!(thread[2].subject.as_deref(), Some("Thread msg 2"));
    }

    #[test]
    fn test_mark_as_read() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let msg = make_insert_message(&account.id, "INBOX", "Unread msg", false);
        let id = InsertMessage::insert(&conn, &msg).unwrap();

        let detail = MessageDetail::get_by_id(&conn, &id).unwrap();
        assert!(!detail.is_read);

        let result = mark_as_read(&conn, &id);
        assert!(result);

        let detail = MessageDetail::get_by_id(&conn, &id).unwrap();
        assert!(detail.is_read);
    }

    #[test]
    fn test_message_detail_includes_message_id() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg = make_insert_message(&account.id, "INBOX", "ID Test", false);
        msg.message_id = Some("<unique-123@example.com>".to_string());
        let id = InsertMessage::insert(&conn, &msg).unwrap();

        let detail = MessageDetail::get_by_id(&conn, &id).unwrap();
        assert_eq!(detail.message_id.as_deref(), Some("<unique-123@example.com>"));
    }

    #[test]
    fn test_save_and_list_drafts() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let draft_id = save_draft(
            &conn,
            None, // new draft
            &account.id,
            Some(r#"["alice@example.com"]"#),
            None,
            None,
            Some("Draft subject"),
            "Draft body text",
            None,
        );
        assert!(!draft_id.is_empty());

        let drafts = list_drafts(&conn, &account.id);
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].id, draft_id);
        assert_eq!(drafts[0].subject.as_deref(), Some("Draft subject"));
    }

    #[test]
    fn test_update_existing_draft() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let draft_id = save_draft(
            &conn,
            None,
            &account.id,
            Some(r#"["alice@example.com"]"#),
            None,
            None,
            Some("First version"),
            "Body v1",
            None,
        );

        // Update the same draft
        let same_id = save_draft(
            &conn,
            Some(&draft_id),
            &account.id,
            Some(r#"["alice@example.com","bob@example.com"]"#),
            None,
            None,
            Some("Updated subject"),
            "Body v2",
            None,
        );
        assert_eq!(same_id, draft_id);

        let detail = MessageDetail::get_by_id(&conn, &draft_id).unwrap();
        assert_eq!(detail.subject.as_deref(), Some("Updated subject"));
        assert_eq!(detail.body_text.as_deref(), Some("Body v2"));
    }

    #[test]
    fn test_delete_draft() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let draft_id = save_draft(
            &conn,
            None,
            &account.id,
            None,
            None,
            None,
            Some("To delete"),
            "Body",
            None,
        );

        assert!(delete_draft(&conn, &draft_id));
        let drafts = list_drafts(&conn, &account.id);
        assert_eq!(drafts.len(), 0);
    }

    #[test]
    fn test_batch_update_archive() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg1 = make_insert_message(&account.id, "INBOX", "Msg 1", false);
        msg1.message_id = Some("<batch-1@example.com>".to_string());
        let id1 = InsertMessage::insert(&conn, &msg1).unwrap();

        let mut msg2 = make_insert_message(&account.id, "INBOX", "Msg 2", false);
        msg2.message_id = Some("<batch-2@example.com>".to_string());
        let id2 = InsertMessage::insert(&conn, &msg2).unwrap();

        let mut msg3 = make_insert_message(&account.id, "INBOX", "Msg 3", false);
        msg3.message_id = Some("<batch-3@example.com>".to_string());
        let _id3 = InsertMessage::insert(&conn, &msg3);

        let updated = batch_update(&conn, &[&id1, &id2], "archive");
        assert_eq!(updated, 2);

        let inbox = MessageSummary::list_by_folder(&conn, &account.id, "INBOX", 50, 0);
        assert_eq!(inbox.len(), 1);

        let archived = MessageSummary::list_by_folder(&conn, &account.id, "Archive", 50, 0);
        assert_eq!(archived.len(), 2);
    }

    #[test]
    fn test_batch_update_delete() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg1 = make_insert_message(&account.id, "INBOX", "Delete me", false);
        msg1.message_id = Some("<del-1@example.com>".to_string());
        let id1 = InsertMessage::insert(&conn, &msg1).unwrap();

        let updated = batch_update(&conn, &[&id1], "delete");
        assert_eq!(updated, 1);

        let inbox = MessageSummary::list_by_folder(&conn, &account.id, "INBOX", 50, 0);
        assert_eq!(inbox.len(), 0);
    }

    #[test]
    fn test_batch_update_read_unread() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg1 = make_insert_message(&account.id, "INBOX", "Unread", false);
        msg1.message_id = Some("<read-1@example.com>".to_string());
        let id1 = InsertMessage::insert(&conn, &msg1).unwrap();

        let updated = batch_update(&conn, &[&id1], "mark_read");
        assert_eq!(updated, 1);
        let detail = MessageDetail::get_by_id(&conn, &id1).unwrap();
        assert!(detail.is_read);

        let updated = batch_update(&conn, &[&id1], "mark_unread");
        assert_eq!(updated, 1);
        let detail = MessageDetail::get_by_id(&conn, &id1).unwrap();
        assert!(!detail.is_read);
    }

    #[test]
    fn test_batch_update_star() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg1 = make_insert_message(&account.id, "INBOX", "Star me", false);
        msg1.message_id = Some("<star-1@example.com>".to_string());
        let id1 = InsertMessage::insert(&conn, &msg1).unwrap();

        let updated = batch_update(&conn, &[&id1], "star");
        assert_eq!(updated, 1);
        let detail = MessageDetail::get_by_id(&conn, &id1).unwrap();
        assert!(detail.is_starred);

        let updated = batch_update(&conn, &[&id1], "unstar");
        assert_eq!(updated, 1);
        let detail = MessageDetail::get_by_id(&conn, &id1).unwrap();
        assert!(!detail.is_starred);
    }

    #[test]
    fn test_fts5_search() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg1 = make_insert_message(&account.id, "INBOX", "Invoice from Amazon", false);
        msg1.message_id = Some("<fts-1@example.com>".to_string());
        msg1.body_text = Some("Please find attached your invoice for order #12345.".to_string());
        InsertMessage::insert(&conn, &msg1);

        let mut msg2 = make_insert_message(&account.id, "INBOX", "Meeting tomorrow", false);
        msg2.message_id = Some("<fts-2@example.com>".to_string());
        msg2.body_text = Some("Let's meet at 3pm to discuss the project.".to_string());
        InsertMessage::insert(&conn, &msg2);

        // Search for "invoice" — should match msg1
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM fts_messages WHERE fts_messages MATCH '\"invoice\"'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert!(count >= 1);

        // Search for "meeting" — should match msg2
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM fts_messages WHERE fts_messages MATCH '\"meeting\"'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert!(count >= 1);

        // Search for "nonexistent" — should match nothing
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM fts_messages WHERE fts_messages MATCH '\"zzzznonexistent\"'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 0);

        // Snippet extraction
        let snippet: String = conn.query_row(
            "SELECT snippet(fts_messages, 2, '<mark>', '</mark>', '...', 20)
             FROM fts_messages WHERE fts_messages MATCH '\"invoice\"'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert!(snippet.contains("<mark>"));
    }

    #[test]
    fn test_update_ai_metadata() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let msg = make_insert_message(&account.id, "INBOX", "AI Test", false);
        let id = InsertMessage::insert(&conn, &msg).unwrap();

        // Initially AI fields are null
        let detail = MessageDetail::get_by_id(&conn, &id).unwrap();
        assert!(detail.ai_intent.is_none());
        assert!(detail.ai_priority_label.is_none());

        // Update AI metadata
        let entities = r#"{"people":["Alice","Bob"],"dates":["2024-03-15"],"amounts":[],"topics":["project review"]}"#;
        let updated = update_ai_metadata(
            &conn, &id, "ACTION_REQUEST", 0.85, "high", "Primary", "Test email requesting action",
            Some(entities), Some("2024-03-15"), Some("positive"), true,
        );
        assert!(updated);

        // Verify fields are set
        let detail = MessageDetail::get_by_id(&conn, &id).unwrap();
        assert_eq!(detail.ai_intent.as_deref(), Some("ACTION_REQUEST"));
        assert_eq!(detail.ai_priority_score, Some(0.85));
        assert_eq!(detail.ai_priority_label.as_deref(), Some("high"));
        assert_eq!(detail.ai_category.as_deref(), Some("Primary"));
        assert_eq!(detail.ai_summary.as_deref(), Some("Test email requesting action"));
        assert!(detail.ai_needs_reply);
    }

    #[test]
    fn test_list_needs_reply() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Create 3 messages: 2 unread needing reply, 1 read needing reply
        let mut msg1 = make_insert_message(&account.id, "INBOX", "Question 1", false);
        msg1.message_id = Some("<nr-1@example.com>".to_string());
        let id1 = InsertMessage::insert(&conn, &msg1).unwrap();

        let mut msg2 = make_insert_message(&account.id, "INBOX", "Question 2", false);
        msg2.message_id = Some("<nr-2@example.com>".to_string());
        let id2 = InsertMessage::insert(&conn, &msg2).unwrap();

        let mut msg3 = make_insert_message(&account.id, "INBOX", "Question 3 (read)", true);
        msg3.message_id = Some("<nr-3@example.com>".to_string());
        let id3 = InsertMessage::insert(&conn, &msg3).unwrap();

        // Mark all as needing reply
        update_ai_metadata(&conn, &id1, "ACTION_REQUEST", 0.9, "urgent", "Primary", "Q1", None, None, None, true);
        update_ai_metadata(&conn, &id2, "ACTION_REQUEST", 0.8, "high", "Primary", "Q2", None, None, None, true);
        update_ai_metadata(&conn, &id3, "ACTION_REQUEST", 0.7, "high", "Primary", "Q3", None, None, None, true);

        // Only unread messages that need reply
        let (needs_reply, total) = list_needs_reply(&conn, Some(&account.id), 50, 0);
        assert_eq!(total, 2);
        assert_eq!(needs_reply.len(), 2);
        assert!(needs_reply.iter().all(|m| m.ai_needs_reply));
    }

    #[test]
    fn test_list_needs_reply_all_accounts() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg = make_insert_message(&account.id, "INBOX", "Reply me", false);
        msg.message_id = Some("<nr-all-1@example.com>".to_string());
        let id = InsertMessage::insert(&conn, &msg).unwrap();
        update_ai_metadata(&conn, &id, "ACTION_REQUEST", 0.9, "urgent", "Primary", "Q", None, None, None, true);

        let (needs_reply, total) = list_needs_reply(&conn, None, 50, 0);
        assert_eq!(total, 1);
        assert_eq!(needs_reply.len(), 1);
    }

    #[test]
    fn test_decay_priority_scores() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert a message with an old date (30 days ago) and high priority
        let mut msg = make_insert_message(&account.id, "INBOX", "Old High Priority", false);
        msg.date = Some(chrono::Utc::now().timestamp() - 30 * 86400);
        msg.message_id = Some("<decay-test-1@example.com>".to_string());
        let id = InsertMessage::insert(&conn, &msg).unwrap();

        // Set AI metadata: high priority (0.85)
        update_ai_metadata(&conn, &id, "ACTION_REQUEST", 0.85, "high", "Primary", "Test", None, None, None, false);

        // Run decay with 7-day threshold and 0.85 factor
        let decayed = decay_priority_scores(&conn, 7, 0.85);
        assert_eq!(decayed, 1);

        // Verify score was reduced: 0.85 * 0.85 = 0.7225
        let detail = MessageDetail::get_by_id(&conn, &id).unwrap();
        let score = detail.ai_priority_score.unwrap();
        assert!(score < 0.85, "Score should have been reduced from 0.85, got {}", score);
        assert!(score >= 0.7, "Score should be around 0.7225, got {}", score);
        assert_eq!(detail.ai_priority_label.as_deref(), Some("high"));
    }

    #[test]
    fn test_decay_skips_recent() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert a message with a recent date (1 day ago) and high priority
        let mut msg = make_insert_message(&account.id, "INBOX", "Recent High Priority", false);
        msg.date = Some(chrono::Utc::now().timestamp() - 86400);
        msg.message_id = Some("<decay-recent@example.com>".to_string());
        let id = InsertMessage::insert(&conn, &msg).unwrap();

        update_ai_metadata(&conn, &id, "ACTION_REQUEST", 0.85, "high", "Primary", "Test", None, None, None, false);

        // Decay with 7-day threshold: recent message should NOT be affected
        let decayed = decay_priority_scores(&conn, 7, 0.85);
        assert_eq!(decayed, 0);

        let detail = MessageDetail::get_by_id(&conn, &id).unwrap();
        assert_eq!(detail.ai_priority_score, Some(0.85));
    }

    #[test]
    fn test_decay_skips_low_priority() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert old message with low priority (score 0.2 < 0.3 threshold)
        let mut msg = make_insert_message(&account.id, "INBOX", "Old Low Priority", false);
        msg.date = Some(chrono::Utc::now().timestamp() - 30 * 86400);
        msg.message_id = Some("<decay-low@example.com>".to_string());
        let id = InsertMessage::insert(&conn, &msg).unwrap();

        update_ai_metadata(&conn, &id, "FYI", 0.2, "low", "Updates", "Test", None, None, None, false);

        // Decay should skip messages with score <= 0.3
        let decayed = decay_priority_scores(&conn, 7, 0.85);
        assert_eq!(decayed, 0);

        let detail = MessageDetail::get_by_id(&conn, &id).unwrap();
        assert_eq!(detail.ai_priority_score, Some(0.2));
    }

    #[test]
    fn test_decay_label_update() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert old message with urgent priority (0.9)
        let mut msg = make_insert_message(&account.id, "INBOX", "Old Urgent", false);
        msg.date = Some(chrono::Utc::now().timestamp() - 30 * 86400);
        msg.message_id = Some("<decay-label@example.com>".to_string());
        let id = InsertMessage::insert(&conn, &msg).unwrap();

        update_ai_metadata(&conn, &id, "URGENT", 0.9, "urgent", "Primary", "Test", None, None, None, false);

        // Run decay multiple times to push score down through label thresholds
        // 0.9 * 0.5 = 0.45 (normal range: 0.3-0.6)
        let decayed = decay_priority_scores(&conn, 7, 0.5);
        assert_eq!(decayed, 1);

        let detail = MessageDetail::get_by_id(&conn, &id).unwrap();
        let score = detail.ai_priority_score.unwrap();
        assert!(score < 0.6, "Score should be below high threshold, got {}", score);
        assert_eq!(detail.ai_priority_label.as_deref(), Some("normal"));
    }
}
