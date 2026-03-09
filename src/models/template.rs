use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Row;
use serde::{Deserialize, Serialize};

type Conn = PooledConnection<SqliteConnectionManager>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub subject: Option<String>,
    pub body_text: String,
    pub body_html: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Template {
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            name: row.get("name")?,
            subject: row.get("subject")?,
            body_text: row.get("body_text")?,
            body_html: row.get("body_html")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    pub fn list(conn: &Conn) -> Vec<Self> {
        let mut stmt = conn
            .prepare("SELECT * FROM email_templates ORDER BY updated_at DESC")
            .expect("failed to prepare list templates query");
        stmt.query_map([], Self::from_row)
            .expect("failed to query templates")
            .filter_map(|r| r.map_err(|e| tracing::warn!("Template row skip: {e}")).ok())
            .collect()
    }

    pub fn get_by_id(conn: &Conn, id: &str) -> Option<Self> {
        conn.query_row(
            "SELECT * FROM email_templates WHERE id = ?1",
            [id],
            Self::from_row,
        )
        .ok()
    }

    pub fn create(conn: &Conn, input: &CreateTemplate) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO email_templates (id, name, subject, body_text, body_html) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, input.name, input.subject, input.body_text, input.body_html],
        )
        .expect("failed to insert template");

        Self::get_by_id(conn, &id).expect("failed to retrieve created template")
    }

    pub fn update(conn: &Conn, id: &str, input: &UpdateTemplate) -> Option<Self> {
        let rows = conn
            .execute(
                "UPDATE email_templates SET name = ?1, subject = ?2, body_text = ?3, body_html = ?4, updated_at = unixepoch() WHERE id = ?5",
                rusqlite::params![input.name, input.subject, input.body_text, input.body_html, id],
            )
            .expect("failed to update template");

        if rows == 0 {
            return None;
        }
        Self::get_by_id(conn, id)
    }

    pub fn delete(conn: &Conn, id: &str) -> bool {
        let rows = conn
            .execute("DELETE FROM email_templates WHERE id = ?1", [id])
            .expect("failed to delete template");
        rows > 0
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTemplate {
    pub name: String,
    pub subject: Option<String>,
    pub body_text: String,
    pub body_html: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateTemplate {
    pub name: String,
    pub subject: Option<String>,
    pub body_text: String,
    pub body_html: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    #[test]
    fn test_create_and_list_templates() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let input = CreateTemplate {
            name: "Welcome".to_string(),
            subject: Some("Welcome aboard!".to_string()),
            body_text: "Hi there, welcome to our team.".to_string(),
            body_html: None,
        };

        let template = Template::create(&conn, &input);
        assert_eq!(template.name, "Welcome");
        assert_eq!(template.subject.as_deref(), Some("Welcome aboard!"));
        assert_eq!(template.body_text, "Hi there, welcome to our team.");

        let templates = Template::list(&conn);
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].name, "Welcome");
    }

    #[test]
    fn test_update_template() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let input = CreateTemplate {
            name: "Original".to_string(),
            subject: None,
            body_text: "Original body".to_string(),
            body_html: None,
        };

        let template = Template::create(&conn, &input);

        let update = UpdateTemplate {
            name: "Updated".to_string(),
            subject: Some("New subject".to_string()),
            body_text: "Updated body".to_string(),
            body_html: Some("<p>Updated body</p>".to_string()),
        };

        let updated = Template::update(&conn, &template.id, &update).unwrap();
        assert_eq!(updated.name, "Updated");
        assert_eq!(updated.subject.as_deref(), Some("New subject"));
        assert_eq!(updated.body_text, "Updated body");
        assert_eq!(updated.body_html.as_deref(), Some("<p>Updated body</p>"));
    }

    #[test]
    fn test_delete_template() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let input = CreateTemplate {
            name: "Temp".to_string(),
            subject: None,
            body_text: "Temporary".to_string(),
            body_html: None,
        };

        let template = Template::create(&conn, &input);
        assert!(Template::delete(&conn, &template.id));
        assert!(Template::get_by_id(&conn, &template.id).is_none());

        let templates = Template::list(&conn);
        assert_eq!(templates.len(), 0);
    }

    #[test]
    fn test_delete_nonexistent_template() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        assert!(!Template::delete(&conn, "nonexistent-id"));
    }

    #[test]
    fn test_update_nonexistent_template() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let update = UpdateTemplate {
            name: "Ghost".to_string(),
            subject: None,
            body_text: "Does not exist".to_string(),
            body_html: None,
        };

        assert!(Template::update(&conn, "nonexistent-id", &update).is_none());
    }
}
