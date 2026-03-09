use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use serde::{Deserialize, Serialize};

type Conn = PooledConnection<SqliteConnectionManager>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutedThread {
    pub thread_id: String,
    pub muted_at: i64,
}

/// Mute a thread. Uses INSERT OR IGNORE so muting an already-muted thread is a no-op.
/// Returns true on success.
pub fn mute(conn: &Conn, thread_id: &str) -> bool {
    conn.execute(
        "INSERT OR IGNORE INTO muted_threads (thread_id) VALUES (?1)",
        rusqlite::params![thread_id],
    )
    .is_ok()
}

/// Unmute a thread. Returns true if the thread was actually muted (row deleted).
pub fn unmute(conn: &Conn, thread_id: &str) -> bool {
    conn.execute(
        "DELETE FROM muted_threads WHERE thread_id = ?1",
        rusqlite::params![thread_id],
    )
    .map(|rows| rows > 0)
    .unwrap_or(false)
}

/// Check if a thread is muted.
pub fn is_muted(conn: &Conn, thread_id: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM muted_threads WHERE thread_id = ?1)",
        rusqlite::params![thread_id],
        |row| row.get(0),
    )
    .unwrap_or(false)
}

/// List all muted thread IDs.
pub fn list_muted(conn: &Conn) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT thread_id FROM muted_threads ORDER BY muted_at DESC")
        .expect("failed to prepare list_muted query");
    stmt.query_map([], |row| row.get(0))
        .expect("failed to query muted threads")
        .filter_map(|r| r.ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    #[test]
    fn test_mute_thread() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        assert!(mute(&conn, "thread-123"));
        assert!(is_muted(&conn, "thread-123"));
    }

    #[test]
    fn test_mute_idempotent() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        assert!(mute(&conn, "thread-123"));
        // Muting again should still succeed (INSERT OR IGNORE)
        assert!(mute(&conn, "thread-123"));
        assert!(is_muted(&conn, "thread-123"));

        // Should only have one entry
        let muted = list_muted(&conn);
        assert_eq!(muted.len(), 1);
    }

    #[test]
    fn test_unmute_thread() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        mute(&conn, "thread-456");
        assert!(is_muted(&conn, "thread-456"));

        assert!(unmute(&conn, "thread-456"));
        assert!(!is_muted(&conn, "thread-456"));
    }

    #[test]
    fn test_unmute_nonexistent() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Unmuting a thread that was never muted returns false
        assert!(!unmute(&conn, "thread-nonexistent"));
    }

    #[test]
    fn test_is_muted_false_by_default() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        assert!(!is_muted(&conn, "thread-never-muted"));
    }

    #[test]
    fn test_list_muted() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        mute(&conn, "thread-a");
        mute(&conn, "thread-b");
        mute(&conn, "thread-c");

        let muted = list_muted(&conn);
        assert_eq!(muted.len(), 3);
        assert!(muted.contains(&"thread-a".to_string()));
        assert!(muted.contains(&"thread-b".to_string()));
        assert!(muted.contains(&"thread-c".to_string()));
    }

    #[test]
    fn test_list_muted_after_unmute() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        mute(&conn, "thread-x");
        mute(&conn, "thread-y");
        unmute(&conn, "thread-x");

        let muted = list_muted(&conn);
        assert_eq!(muted.len(), 1);
        assert_eq!(muted[0], "thread-y");
    }
}
