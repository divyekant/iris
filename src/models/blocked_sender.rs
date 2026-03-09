use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use serde::{Deserialize, Serialize};

type Conn = PooledConnection<SqliteConnectionManager>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedSender {
    pub id: String,
    pub email_address: String,
    pub reason: Option<String>,
    pub created_at: i64,
}

impl BlockedSender {
    /// List all blocked senders, ordered by most recently blocked first.
    pub fn list(conn: &Conn) -> Vec<Self> {
        let mut stmt = conn
            .prepare(
                "SELECT id, email_address, reason, created_at
                 FROM blocked_senders
                 ORDER BY created_at DESC",
            )
            .expect("failed to prepare list blocked_senders");

        stmt.query_map([], |row| {
            Ok(Self {
                id: row.get("id")?,
                email_address: row.get("email_address")?,
                reason: row.get("reason")?,
                created_at: row.get("created_at")?,
            })
        })
        .expect("failed to query blocked_senders")
        .filter_map(|r| r.ok())
        .collect()
    }

    /// Block a sender by email address. Returns the new BlockedSender.
    /// If the sender is already blocked, returns the existing record.
    pub fn block(conn: &Conn, email_address: &str, reason: Option<&str>) -> Self {
        let normalized = email_address.trim().to_lowercase();

        // Check if already blocked
        if let Some(existing) = Self::find_by_email(conn, &normalized) {
            return existing;
        }

        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT OR IGNORE INTO blocked_senders (id, email_address, reason) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, normalized, reason],
        )
        .expect("failed to insert blocked_sender");

        // Return the record (could be the one we just inserted or a race-condition duplicate)
        Self::find_by_email(conn, &normalized).expect("blocked sender should exist after insert")
    }

    /// Unblock a sender by ID. Returns true if a row was deleted.
    pub fn unblock(conn: &Conn, id: &str) -> bool {
        conn.execute(
            "DELETE FROM blocked_senders WHERE id = ?1",
            rusqlite::params![id],
        )
        .map(|rows| rows > 0)
        .unwrap_or(false)
    }

    /// Check if an email address is blocked.
    pub fn is_blocked(conn: &Conn, email_address: &str) -> bool {
        let normalized = email_address.trim().to_lowercase();
        conn.query_row(
            "SELECT COUNT(*) FROM blocked_senders WHERE email_address = ?1",
            rusqlite::params![normalized],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
            > 0
    }

    /// Find a blocked sender by email address.
    fn find_by_email(conn: &Conn, email_address: &str) -> Option<Self> {
        conn.query_row(
            "SELECT id, email_address, reason, created_at FROM blocked_senders WHERE email_address = ?1",
            rusqlite::params![email_address],
            |row| {
                Ok(Self {
                    id: row.get("id")?,
                    email_address: row.get("email_address")?,
                    reason: row.get("reason")?,
                    created_at: row.get("created_at")?,
                })
            },
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    #[test]
    fn test_block_and_list() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let blocked = BlockedSender::block(&conn, "spam@example.com", Some("Sent spam"));
        assert_eq!(blocked.email_address, "spam@example.com");
        assert_eq!(blocked.reason.as_deref(), Some("Sent spam"));

        let all = BlockedSender::list(&conn);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].email_address, "spam@example.com");
    }

    #[test]
    fn test_block_normalizes_email() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        BlockedSender::block(&conn, "  SPAM@Example.COM  ", None);
        assert!(BlockedSender::is_blocked(&conn, "spam@example.com"));
    }

    #[test]
    fn test_block_idempotent() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let first = BlockedSender::block(&conn, "dup@example.com", Some("First"));
        let second = BlockedSender::block(&conn, "dup@example.com", Some("Second"));
        assert_eq!(first.id, second.id);

        let all = BlockedSender::list(&conn);
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_unblock() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let blocked = BlockedSender::block(&conn, "unblock@example.com", None);
        assert!(BlockedSender::is_blocked(&conn, "unblock@example.com"));

        assert!(BlockedSender::unblock(&conn, &blocked.id));
        assert!(!BlockedSender::is_blocked(&conn, "unblock@example.com"));

        let all = BlockedSender::list(&conn);
        assert_eq!(all.len(), 0);
    }

    #[test]
    fn test_is_blocked() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        assert!(!BlockedSender::is_blocked(&conn, "nobody@example.com"));
        BlockedSender::block(&conn, "nobody@example.com", None);
        assert!(BlockedSender::is_blocked(&conn, "nobody@example.com"));
    }
}
