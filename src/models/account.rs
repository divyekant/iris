use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Row;
use serde::{Deserialize, Serialize};

type Conn = PooledConnection<SqliteConnectionManager>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub provider: String,
    pub email: String,
    pub display_name: Option<String>,
    #[serde(skip_serializing)]
    pub access_token: Option<String>,
    #[serde(skip_serializing)]
    pub refresh_token: Option<String>,
    pub token_expires_at: Option<i64>,
    pub imap_host: Option<String>,
    pub imap_port: Option<i32>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<i32>,
    pub username: Option<String>,
    #[serde(skip_serializing)]
    pub password_encrypted: Option<String>,
    pub last_sync_at: Option<i64>,
    pub sync_status: Option<String>,
    pub sync_error: Option<String>,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Account {
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        let access_token = crate::secrets::decrypt_optional_secret(row.get("access_token")?)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
        let refresh_token = crate::secrets::decrypt_optional_secret(row.get("refresh_token")?)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
        let password_encrypted = crate::secrets::decrypt_optional_secret(
            row.get("password_encrypted")?,
        )
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;

        Ok(Self {
            id: row.get("id")?,
            provider: row.get("provider")?,
            email: row.get("email")?,
            display_name: row.get("display_name")?,
            access_token,
            refresh_token,
            token_expires_at: row.get("token_expires_at")?,
            imap_host: row.get("imap_host")?,
            imap_port: row.get("imap_port")?,
            smtp_host: row.get("smtp_host")?,
            smtp_port: row.get("smtp_port")?,
            username: row.get("username")?,
            password_encrypted,
            last_sync_at: row.get("last_sync_at")?,
            sync_status: row.get("sync_status")?,
            sync_error: row.get("sync_error")?,
            is_active: row.get("is_active")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    pub fn list(conn: &Conn) -> Vec<Self> {
        let mut stmt = conn
            .prepare("SELECT * FROM accounts WHERE is_active = 1 ORDER BY created_at ASC")
            .expect("failed to prepare list accounts query");
        stmt.query_map([], Self::from_row)
            .expect("failed to query accounts")
            .filter_map(|r| r.map_err(|e| tracing::warn!("Account row skip: {e}")).ok())
            .collect()
    }

    pub fn get_by_id(conn: &Conn, id: &str) -> Option<Self> {
        conn.query_row("SELECT * FROM accounts WHERE id = ?1", [id], Self::from_row)
            .ok()
    }

    pub fn create(conn: &Conn, input: &CreateAccount) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let encrypted_password = crate::secrets::encrypt_optional_secret(input.password.as_deref())
            .expect("failed to encrypt account password");
        conn.execute(
            "INSERT INTO accounts (id, provider, email, display_name, imap_host, imap_port, smtp_host, smtp_port, username, password_encrypted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id,
                input.provider,
                input.email,
                input.display_name,
                input.imap_host,
                input.imap_port,
                input.smtp_host,
                input.smtp_port,
                input.username,
                encrypted_password,
            ],
        )
        .expect("failed to insert account");

        Self::get_by_id(conn, &id).expect("failed to retrieve created account")
    }

    pub fn update_oauth_tokens(
        conn: &Conn,
        id: &str,
        access_token: &str,
        refresh_token: &str,
        expires_at: i64,
    ) {
        let access_token = crate::secrets::encrypt_secret_for_storage(access_token)
            .expect("failed to encrypt access token");
        let refresh_token = crate::secrets::encrypt_secret_for_storage(refresh_token)
            .expect("failed to encrypt refresh token");
        conn.execute(
            "UPDATE accounts SET access_token = ?1, refresh_token = ?2, token_expires_at = ?3, updated_at = unixepoch() WHERE id = ?4",
            rusqlite::params![access_token, refresh_token, expires_at, id],
        )
        .expect("failed to update oauth tokens");
    }

    pub fn update_sync_status(conn: &Conn, id: &str, status: &str, error: Option<&str>) {
        conn.execute(
            "UPDATE accounts SET sync_status = ?1, sync_error = ?2, last_sync_at = unixepoch(), updated_at = unixepoch() WHERE id = ?3",
            rusqlite::params![status, error, id],
        )
        .expect("failed to update sync status");
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateAccount {
    pub provider: String,
    pub email: String,
    pub display_name: Option<String>,
    pub imap_host: Option<String>,
    pub imap_port: Option<i32>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<i32>,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    struct SecretsKeyReset;

    impl Drop for SecretsKeyReset {
        fn drop(&mut self) {
            crate::secrets::set_runtime_key_for_tests(None);
        }
    }

    #[test]
    fn test_create_and_list_accounts() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "test@example.com".to_string(),
            display_name: Some("Test User".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("test@example.com".to_string()),
            password: Some("secret123".to_string()),
        };

        let account = Account::create(&conn, &input);
        assert_eq!(account.email, "test@example.com");
        assert_eq!(account.provider, "gmail");
        assert!(account.is_active);

        let accounts = Account::list(&conn);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].email, "test@example.com");
    }

    #[test]
    fn test_sensitive_fields_not_serialized() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let input = CreateAccount {
            provider: "imap".to_string(),
            email: "user@example.com".to_string(),
            display_name: None,
            imap_host: Some("mail.example.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("mail.example.com".to_string()),
            smtp_port: Some(587),
            username: Some("user@example.com".to_string()),
            password: Some("mypassword".to_string()),
        };

        let account = Account::create(&conn, &input);
        let json = serde_json::to_string(&account).unwrap();
        assert!(!json.contains("access_token"));
        assert!(!json.contains("refresh_token"));
        assert!(!json.contains("password_encrypted"));
        assert!(!json.contains("mypassword"));
    }

    #[test]
    fn test_update_oauth_tokens() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "oauth@example.com".to_string(),
            display_name: None,
            imap_host: None,
            imap_port: None,
            smtp_host: None,
            smtp_port: None,
            username: None,
            password: None,
        };

        let account = Account::create(&conn, &input);
        Account::update_oauth_tokens(&conn, &account.id, "new_access", "new_refresh", 9999999);

        let updated = Account::get_by_id(&conn, &account.id).unwrap();
        assert_eq!(updated.access_token.as_deref(), Some("new_access"));
        assert_eq!(updated.refresh_token.as_deref(), Some("new_refresh"));
        assert_eq!(updated.token_expires_at, Some(9999999));
    }

    #[test]
    fn test_update_sync_status() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "sync@example.com".to_string(),
            display_name: None,
            imap_host: None,
            imap_port: None,
            smtp_host: None,
            smtp_port: None,
            username: None,
            password: None,
        };

        let account = Account::create(&conn, &input);
        Account::update_sync_status(&conn, &account.id, "error", Some("Connection refused"));

        let updated = Account::get_by_id(&conn, &account.id).unwrap();
        assert_eq!(updated.sync_status.as_deref(), Some("error"));
        assert_eq!(updated.sync_error.as_deref(), Some("Connection refused"));
    }

    #[test]
    fn test_account_password_encrypted_at_rest_when_key_configured() {
        let _lock = crate::secrets::lock_test_runtime_key();
        let _reset = SecretsKeyReset;
        crate::secrets::set_runtime_key_for_tests(Some([11_u8; 32]));

        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let input = CreateAccount {
            provider: "imap".to_string(),
            email: "secure@example.com".to_string(),
            display_name: None,
            imap_host: Some("mail.example.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("mail.example.com".to_string()),
            smtp_port: Some(587),
            username: Some("secure@example.com".to_string()),
            password: Some("topsecret".to_string()),
        };

        let account = Account::create(&conn, &input);
        let raw_password: String = conn
            .query_row(
                "SELECT password_encrypted FROM accounts WHERE id = ?1",
                [&account.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(raw_password.starts_with("enc:v1:"));

        let reloaded = Account::get_by_id(&conn, &account.id).unwrap();
        assert_eq!(reloaded.password_encrypted.as_deref(), Some("topsecret"));
    }

    #[test]
    fn test_oauth_tokens_encrypted_at_rest_when_key_configured() {
        let _lock = crate::secrets::lock_test_runtime_key();
        let _reset = SecretsKeyReset;
        crate::secrets::set_runtime_key_for_tests(Some([13_u8; 32]));

        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "oauth@example.com".to_string(),
            display_name: None,
            imap_host: None,
            imap_port: None,
            smtp_host: None,
            smtp_port: None,
            username: None,
            password: None,
        };

        let account = Account::create(&conn, &input);
        Account::update_oauth_tokens(&conn, &account.id, "enc-access", "enc-refresh", 12345);

        let (raw_access, raw_refresh): (String, String) = conn
            .query_row(
                "SELECT access_token, refresh_token FROM accounts WHERE id = ?1",
                [&account.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(raw_access.starts_with("enc:v1:"));
        assert!(raw_refresh.starts_with("enc:v1:"));

        let reloaded = Account::get_by_id(&conn, &account.id).unwrap();
        assert_eq!(reloaded.access_token.as_deref(), Some("enc-access"));
        assert_eq!(reloaded.refresh_token.as_deref(), Some("enc-refresh"));
    }
}
