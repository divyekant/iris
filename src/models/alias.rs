use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Alias {
    pub id: String,
    pub account_id: String,
    pub email: String,
    pub display_name: String,
    pub reply_to: Option<String>,
    pub is_default: bool,
    pub created_at: i64,
}

pub fn create(
    conn: &Connection,
    account_id: &str,
    email: &str,
    display_name: &str,
    reply_to: Option<&str>,
    is_default: bool,
) -> rusqlite::Result<Alias> {
    let id = Uuid::new_v4().to_string();

    // If setting as default, clear other defaults for this account
    if is_default {
        conn.execute(
            "UPDATE aliases SET is_default = 0 WHERE account_id = ?1",
            params![account_id],
        )?;
    }

    conn.execute(
        "INSERT INTO aliases (id, account_id, email, display_name, reply_to, is_default) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, account_id, email, display_name, reply_to, is_default],
    )?;
    get(conn, &id)
}

pub fn list(conn: &Connection, account_id: Option<&str>) -> rusqlite::Result<Vec<Alias>> {
    if let Some(acct) = account_id {
        let mut stmt = conn.prepare(
            "SELECT id, account_id, email, display_name, reply_to, is_default, created_at FROM aliases WHERE account_id = ?1 ORDER BY is_default DESC, created_at ASC",
        )?;
        let rows = stmt.query_map(params![acct], row_to_alias)?;
        rows.collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, account_id, email, display_name, reply_to, is_default, created_at FROM aliases ORDER BY account_id, is_default DESC, created_at ASC",
        )?;
        let rows = stmt.query_map([], row_to_alias)?;
        rows.collect()
    }
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Alias> {
    conn.query_row(
        "SELECT id, account_id, email, display_name, reply_to, is_default, created_at FROM aliases WHERE id = ?1",
        params![id],
        row_to_alias,
    )
}

pub fn update(
    conn: &Connection,
    id: &str,
    email: &str,
    display_name: &str,
    reply_to: Option<&str>,
    is_default: bool,
) -> rusqlite::Result<Alias> {
    // If setting as default, clear other defaults for same account
    if is_default {
        let account_id: String = conn.query_row(
            "SELECT account_id FROM aliases WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        conn.execute(
            "UPDATE aliases SET is_default = 0 WHERE account_id = ?1",
            params![account_id],
        )?;
    }

    let changed = conn.execute(
        "UPDATE aliases SET email = ?2, display_name = ?3, reply_to = ?4, is_default = ?5 WHERE id = ?1",
        params![id, email, display_name, reply_to, is_default],
    )?;
    if changed == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let changed = conn.execute("DELETE FROM aliases WHERE id = ?1", params![id])?;
    Ok(changed > 0)
}

fn row_to_alias(row: &rusqlite::Row) -> rusqlite::Result<Alias> {
    Ok(Alias {
        id: row.get(0)?,
        account_id: row.get(1)?,
        email: row.get(2)?,
        display_name: row.get(3)?,
        reply_to: row.get(4)?,
        is_default: row.get(5)?,
        created_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        let schema = std::fs::read_to_string("migrations/001_initial.sql").unwrap();
        conn.execute_batch(&schema).unwrap();
        let migration = std::fs::read_to_string("migrations/017_aliases.sql").unwrap();
        conn.execute_batch(&migration).unwrap();
        conn
    }

    #[test]
    fn test_create_and_list() {
        let conn = setup_db();
        let alias = create(&conn, "acc1", "john@company.com", "John Doe", None, true).unwrap();
        assert_eq!(alias.email, "john@company.com");
        assert_eq!(alias.display_name, "John Doe");
        assert!(alias.is_default);

        let all = list(&conn, Some("acc1")).unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_default_toggle() {
        let conn = setup_db();
        let a1 = create(&conn, "acc1", "a@test.com", "A", None, true).unwrap();
        assert!(a1.is_default);

        let _a2 = create(&conn, "acc1", "b@test.com", "B", None, true).unwrap();
        // a1 should no longer be default
        let a1_refreshed = get(&conn, &a1.id).unwrap();
        assert!(!a1_refreshed.is_default);
    }

    #[test]
    fn test_delete() {
        let conn = setup_db();
        let alias = create(&conn, "acc1", "test@test.com", "Test", None, false).unwrap();
        assert!(delete(&conn, &alias.id).unwrap());
        assert!(!delete(&conn, &alias.id).unwrap());
    }
}
