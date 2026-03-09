use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Row;
use serde::{Deserialize, Serialize};

type Conn = PooledConnection<SqliteConnectionManager>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub body_text: String,
    pub body_html: String,
    pub is_default: bool,
    pub created_at: i64,
}

impl Signature {
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            account_id: row.get("account_id")?,
            name: row.get("name")?,
            body_text: row.get("body_text")?,
            body_html: row.get("body_html")?,
            is_default: row.get("is_default")?,
            created_at: row.get("created_at")?,
        })
    }

    pub fn list_for_account(conn: &Conn, account_id: &str) -> Vec<Self> {
        let mut stmt = conn
            .prepare("SELECT * FROM signatures WHERE account_id = ?1 ORDER BY is_default DESC, created_at ASC")
            .expect("failed to prepare list signatures query");
        stmt.query_map([account_id], Self::from_row)
            .expect("failed to query signatures")
            .filter_map(|r| r.map_err(|e| tracing::warn!("Signature row skip: {e}")).ok())
            .collect()
    }

    pub fn get_by_id(conn: &Conn, id: &str) -> Option<Self> {
        conn.query_row("SELECT * FROM signatures WHERE id = ?1", [id], Self::from_row)
            .ok()
    }

    pub fn create(conn: &Conn, input: &CreateSignature) -> Self {
        let id = uuid::Uuid::new_v4().to_string();

        // If this is the default, unset other defaults for this account
        if input.is_default.unwrap_or(false) {
            conn.execute(
                "UPDATE signatures SET is_default = 0 WHERE account_id = ?1",
                [&input.account_id],
            )
            .expect("failed to unset default signatures");
        }

        conn.execute(
            "INSERT INTO signatures (id, account_id, name, body_text, body_html, is_default)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id,
                input.account_id,
                input.name,
                input.body_text.as_deref().unwrap_or(""),
                input.body_html.as_deref().unwrap_or(""),
                input.is_default.unwrap_or(false),
            ],
        )
        .expect("failed to insert signature");

        Self::get_by_id(conn, &id).expect("failed to retrieve created signature")
    }

    pub fn update(conn: &Conn, id: &str, input: &UpdateSignature) -> Option<Self> {
        let existing = Self::get_by_id(conn, id)?;

        // If setting as default, unset other defaults for this account
        if input.is_default == Some(true) {
            conn.execute(
                "UPDATE signatures SET is_default = 0 WHERE account_id = ?1",
                [&existing.account_id],
            )
            .expect("failed to unset default signatures");
        }

        let name = input.name.as_deref().unwrap_or(&existing.name);
        let body_text = input.body_text.as_deref().unwrap_or(&existing.body_text);
        let body_html = input.body_html.as_deref().unwrap_or(&existing.body_html);
        let is_default = input.is_default.unwrap_or(existing.is_default);

        conn.execute(
            "UPDATE signatures SET name = ?1, body_text = ?2, body_html = ?3, is_default = ?4 WHERE id = ?5",
            rusqlite::params![name, body_text, body_html, is_default, id],
        )
        .expect("failed to update signature");

        Self::get_by_id(conn, id)
    }

    pub fn delete(conn: &Conn, id: &str) -> bool {
        let rows = conn
            .execute("DELETE FROM signatures WHERE id = ?1", [id])
            .expect("failed to delete signature");
        rows > 0
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateSignature {
    pub account_id: String,
    pub name: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateSignature {
    pub name: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub is_default: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};

    fn create_test_account(conn: &Conn) -> Account {
        Account::create(
            conn,
            &CreateAccount {
                provider: "gmail".to_string(),
                email: "test@example.com".to_string(),
                display_name: Some("Test User".to_string()),
                imap_host: None,
                imap_port: None,
                smtp_host: None,
                smtp_port: None,
                username: None,
                password: None,
            },
        )
    }

    #[test]
    fn test_create_and_list_signatures() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let sig = Signature::create(
            &conn,
            &CreateSignature {
                account_id: account.id.clone(),
                name: "Work".to_string(),
                body_text: Some("Best regards,\nTest User".to_string()),
                body_html: None,
                is_default: Some(true),
            },
        );
        assert_eq!(sig.name, "Work");
        assert!(sig.is_default);

        let sigs = Signature::list_for_account(&conn, &account.id);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "Work");
    }

    #[test]
    fn test_default_unsets_others() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let sig1 = Signature::create(
            &conn,
            &CreateSignature {
                account_id: account.id.clone(),
                name: "Work".to_string(),
                body_text: Some("Work sig".to_string()),
                body_html: None,
                is_default: Some(true),
            },
        );
        assert!(sig1.is_default);

        let sig2 = Signature::create(
            &conn,
            &CreateSignature {
                account_id: account.id.clone(),
                name: "Personal".to_string(),
                body_text: Some("Personal sig".to_string()),
                body_html: None,
                is_default: Some(true),
            },
        );
        assert!(sig2.is_default);

        // sig1 should no longer be default
        let updated_sig1 = Signature::get_by_id(&conn, &sig1.id).unwrap();
        assert!(!updated_sig1.is_default);
    }

    #[test]
    fn test_update_signature() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let sig = Signature::create(
            &conn,
            &CreateSignature {
                account_id: account.id.clone(),
                name: "Draft".to_string(),
                body_text: Some("old text".to_string()),
                body_html: None,
                is_default: None,
            },
        );

        let updated = Signature::update(
            &conn,
            &sig.id,
            &UpdateSignature {
                name: Some("Final".to_string()),
                body_text: Some("new text".to_string()),
                body_html: None,
                is_default: Some(true),
            },
        )
        .unwrap();

        assert_eq!(updated.name, "Final");
        assert_eq!(updated.body_text, "new text");
        assert!(updated.is_default);
    }

    #[test]
    fn test_delete_signature() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let sig = Signature::create(
            &conn,
            &CreateSignature {
                account_id: account.id.clone(),
                name: "Temp".to_string(),
                body_text: None,
                body_html: None,
                is_default: None,
            },
        );

        assert!(Signature::delete(&conn, &sig.id));
        assert!(Signature::get_by_id(&conn, &sig.id).is_none());
        assert!(!Signature::delete(&conn, &sig.id)); // already deleted
    }
}
