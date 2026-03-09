use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedSearch {
    pub id: String,
    pub name: String,
    pub query: String,
    pub account_id: Option<String>,
    pub created_at: i64,
}

pub fn create(conn: &Connection, name: &str, query: &str, account_id: Option<&str>) -> rusqlite::Result<SavedSearch> {
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO saved_searches (id, name, query, account_id) VALUES (?1, ?2, ?3, ?4)",
        params![id, name, query, account_id],
    )?;
    get(conn, &id)
}

pub fn list(conn: &Connection) -> rusqlite::Result<Vec<SavedSearch>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, query, account_id, created_at FROM saved_searches ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(SavedSearch {
            id: row.get(0)?,
            name: row.get(1)?,
            query: row.get(2)?,
            account_id: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<SavedSearch> {
    conn.query_row(
        "SELECT id, name, query, account_id, created_at FROM saved_searches WHERE id = ?1",
        params![id],
        |row| {
            Ok(SavedSearch {
                id: row.get(0)?,
                name: row.get(1)?,
                query: row.get(2)?,
                account_id: row.get(3)?,
                created_at: row.get(4)?,
            })
        },
    )
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let changed = conn.execute("DELETE FROM saved_searches WHERE id = ?1", params![id])?;
    Ok(changed > 0)
}

/// Count results for a saved search query (for badge display)
pub fn count_results(conn: &Connection, query_text: &str) -> rusqlite::Result<i64> {
    // Simple FTS5 count — doesn't parse operators, just raw text search
    let fts_query = query_text
        .split_whitespace()
        .filter(|t| !t.contains(':'))
        .map(|term| {
            let clean = term.replace('"', "");
            format!("\"{clean}\"")
        })
        .collect::<Vec<_>>()
        .join(" ");

    if fts_query.is_empty() {
        return Ok(0);
    }

    conn.query_row(
        "SELECT COUNT(*) FROM fts_messages fts JOIN messages m ON fts.rowid = m.rowid WHERE fts.fts_messages MATCH ?1 AND m.is_deleted = 0",
        params![fts_query],
        |row| row.get(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        let schema = std::fs::read_to_string("migrations/001_initial.sql").unwrap();
        conn.execute_batch(&schema).unwrap();
        let saved = std::fs::read_to_string("migrations/015_saved_searches.sql").unwrap();
        conn.execute_batch(&saved).unwrap();
        conn
    }

    #[test]
    fn test_create_and_list() {
        let conn = setup_db();
        let s = create(&conn, "VIP Unread", "from:boss@acme.com is:unread", None).unwrap();
        assert_eq!(s.name, "VIP Unread");
        assert_eq!(s.query, "from:boss@acme.com is:unread");
        assert!(s.account_id.is_none());

        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_delete() {
        let conn = setup_db();
        let s = create(&conn, "Test", "hello", None).unwrap();
        assert!(delete(&conn, &s.id).unwrap());
        assert!(!delete(&conn, &s.id).unwrap());
        assert_eq!(list(&conn).unwrap().len(), 0);
    }

    #[test]
    fn test_create_with_account() {
        let conn = setup_db();
        let s = create(&conn, "Work", "project update", Some("acc-123")).unwrap();
        assert_eq!(s.account_id.as_deref(), Some("acc-123"));
    }
}
