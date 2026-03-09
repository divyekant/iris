use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Label {
    pub id: String,
    pub name: String,
    pub color: String,
    pub created_at: i64,
}

pub fn create(conn: &Connection, name: &str, color: &str) -> rusqlite::Result<Label> {
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO labels (id, name, color) VALUES (?1, ?2, ?3)",
        params![id, name, color],
    )?;
    get(conn, &id)
}

pub fn list(conn: &Connection) -> rusqlite::Result<Vec<Label>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, color, created_at FROM labels ORDER BY name ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Label {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Label> {
    conn.query_row(
        "SELECT id, name, color, created_at FROM labels WHERE id = ?1",
        params![id],
        |row| {
            Ok(Label {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                created_at: row.get(3)?,
            })
        },
    )
}

pub fn update(conn: &Connection, id: &str, name: &str, color: &str) -> rusqlite::Result<Label> {
    let changed = conn.execute(
        "UPDATE labels SET name = ?2, color = ?3 WHERE id = ?1",
        params![id, name, color],
    )?;
    if changed == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let changed = conn.execute("DELETE FROM labels WHERE id = ?1", params![id])?;
    Ok(changed > 0)
}

/// Count messages that have a given label
pub fn message_count(conn: &Connection, label_name: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE labels IS NOT NULL AND labels LIKE ?1",
        params![format!("%\"{}%", label_name)],
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
        let migration = std::fs::read_to_string("migrations/018_labels.sql").unwrap();
        conn.execute_batch(&migration).unwrap();
        conn
    }

    #[test]
    fn test_create_and_list() {
        let conn = setup_db();
        let label = create(&conn, "Work", "#3B82F6").unwrap();
        assert_eq!(label.name, "Work");
        assert_eq!(label.color, "#3B82F6");

        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_update() {
        let conn = setup_db();
        let label = create(&conn, "Test", "#000000").unwrap();
        let updated = update(&conn, &label.id, "Updated", "#FF0000").unwrap();
        assert_eq!(updated.name, "Updated");
        assert_eq!(updated.color, "#FF0000");
    }

    #[test]
    fn test_delete() {
        let conn = setup_db();
        let label = create(&conn, "Temp", "#000000").unwrap();
        assert!(delete(&conn, &label.id).unwrap());
        assert!(!delete(&conn, &label.id).unwrap());
    }

    #[test]
    fn test_unique_name() {
        let conn = setup_db();
        create(&conn, "Unique", "#000000").unwrap();
        assert!(create(&conn, "Unique", "#FF0000").is_err());
    }
}
