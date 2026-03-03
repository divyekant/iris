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
                        date, is_read, is_starred, has_attachments, labels, ai_priority_label, ai_category
                 FROM messages
                 WHERE account_id = ?1 AND folder = ?2 AND is_deleted = 0
                 ORDER BY date DESC
                 LIMIT ?3 OFFSET ?4",
            )
            .expect("failed to prepare list_by_folder query");

        stmt.query_map(rusqlite::params![account_id, folder, limit, offset], Self::from_row)
            .expect("failed to query messages")
            .filter_map(|r| r.ok())
            .collect()
    }
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
}

impl InsertMessage {
    /// Insert a message into the database.
    /// Uses INSERT OR IGNORE to handle duplicate primary keys gracefully.
    /// Returns the message ID.
    pub fn insert(conn: &Conn, msg: &InsertMessage) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT OR IGNORE INTO messages (
                id, account_id, message_id, thread_id, folder,
                from_address, from_name, to_addresses, cc_addresses, bcc_addresses,
                subject, date, snippet, body_text, body_html,
                is_read, is_starred, is_draft, labels,
                uid, modseq, raw_headers,
                has_attachments, attachment_names, size_bytes
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15,
                ?16, ?17, ?18, ?19,
                ?20, ?21, ?22,
                ?23, ?24, ?25
            )",
            rusqlite::params![
                id,
                msg.account_id,
                msg.message_id,
                msg.thread_id,
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
            ],
        )
        .expect("failed to insert message");

        id
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

        let id1 = InsertMessage::insert(&conn, &msg1);
        let id2 = InsertMessage::insert(&conn, &msg2);
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
}
