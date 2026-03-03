use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use rand::Rng;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::AppState;

type Conn = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub permission: String,
    pub account_id: Option<String>,
    pub is_revoked: bool,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub api_key_id: String,
    pub key_name: Option<String>,
    pub action: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub details: Option<String>,
    pub status: String,
    pub created_at: i64,
}

#[derive(Deserialize)]
pub struct CreateKeyRequest {
    pub name: String,
    pub permission: String,
    pub account_id: Option<String>,
}

#[derive(Serialize)]
pub struct CreateKeyResponse {
    pub key: String,
    pub id: String,
    pub name: String,
    pub permission: String,
    pub key_prefix: String,
}

#[derive(Deserialize)]
pub struct AuditLogQuery {
    pub api_key_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Permission hierarchy
// ---------------------------------------------------------------------------

const VALID_PERMISSIONS: &[&str] = &["read_only", "draft_only", "send_with_approval", "autonomous"];

fn actions_for_permission(perm: &str) -> &'static [&'static str] {
    match perm {
        "read_only" => &["read", "search"],
        "draft_only" => &["read", "search", "draft"],
        "send_with_approval" => &["read", "search", "draft", "send"],
        "autonomous" => &["read", "search", "draft", "send", "execute", "configure"],
        _ => &[],
    }
}

pub fn has_permission(key_permission: &str, required_action: &str) -> bool {
    actions_for_permission(key_permission).contains(&required_action)
}

// ---------------------------------------------------------------------------
// Core functions
// ---------------------------------------------------------------------------

pub fn create_api_key(
    conn: &Conn,
    name: &str,
    permission: &str,
    account_id: Option<&str>,
) -> Result<(String, ApiKey), String> {
    if !VALID_PERMISSIONS.contains(&permission) {
        return Err(format!("invalid permission: {permission}"));
    }

    let id = uuid::Uuid::new_v4().to_string();

    // Generate raw key: iris_ + 32 random hex chars
    let mut rng = rand::thread_rng();
    let random_bytes: [u8; 16] = rng.r#gen();
    let hex_part: String = random_bytes.iter().map(|b| format!("{b:02x}")).collect();
    let raw_key = format!("iris_{hex_part}");

    let key_prefix = &raw_key[..12];

    // Hash with SHA-256
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, permission, account_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, name, key_hash, key_prefix, permission, account_id],
    )
    .map_err(|e| format!("db insert error: {e}"))?;

    let api_key = ApiKey {
        id,
        name: name.to_string(),
        key_prefix: key_prefix.to_string(),
        permission: permission.to_string(),
        account_id: account_id.map(|s| s.to_string()),
        is_revoked: false,
        last_used_at: None,
        created_at: 0, // will be set by DB default
    };

    Ok((raw_key, api_key))
}

pub fn validate_api_key(conn: &Conn, raw_key: &str) -> Option<ApiKey> {
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    let result = conn.query_row(
        "SELECT id, name, key_prefix, permission, account_id, is_revoked, last_used_at, created_at
         FROM api_keys WHERE key_hash = ?1 AND is_revoked = 0",
        params![key_hash],
        |row| {
            Ok(ApiKey {
                id: row.get(0)?,
                name: row.get(1)?,
                key_prefix: row.get(2)?,
                permission: row.get(3)?,
                account_id: row.get(4)?,
                is_revoked: row.get::<_, i32>(5)? != 0,
                last_used_at: row.get(6)?,
                created_at: row.get(7)?,
            })
        },
    );

    if let Ok(api_key) = result {
        // Update last_used_at
        let _ = conn.execute(
            "UPDATE api_keys SET last_used_at = unixepoch() WHERE id = ?1",
            params![api_key.id],
        );
        Some(api_key)
    } else {
        None
    }
}

pub fn revoke_api_key(conn: &Conn, key_id: &str) -> bool {
    let rows = conn
        .execute(
            "UPDATE api_keys SET is_revoked = 1, revoked_at = unixepoch() WHERE id = ?1",
            params![key_id],
        )
        .unwrap_or(0);
    rows > 0
}

pub fn list_api_keys(conn: &Conn) -> Vec<ApiKey> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, key_prefix, permission, account_id, is_revoked, last_used_at, created_at
             FROM api_keys WHERE is_revoked = 0
             ORDER BY created_at DESC",
        )
        .unwrap();

    stmt.query_map([], |row| {
        Ok(ApiKey {
            id: row.get(0)?,
            name: row.get(1)?,
            key_prefix: row.get(2)?,
            permission: row.get(3)?,
            account_id: row.get(4)?,
            is_revoked: row.get::<_, i32>(5)? != 0,
            last_used_at: row.get(6)?,
            created_at: row.get(7)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

// ---------------------------------------------------------------------------
// Audit logging
// ---------------------------------------------------------------------------

pub fn log_audit(
    conn: &Conn,
    key_id: &str,
    action: &str,
    resource_type: Option<&str>,
    resource_id: Option<&str>,
    details: Option<&str>,
    status: &str,
) {
    let _ = conn.execute(
        "INSERT INTO audit_log (api_key_id, action, resource_type, resource_id, details, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![key_id, action, resource_type, resource_id, details, status],
    );
}

pub fn get_audit_log(
    conn: &Conn,
    key_id_filter: Option<&str>,
    limit: i64,
    offset: i64,
) -> Vec<AuditEntry> {
    let (sql, filter_params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(kid) =
        key_id_filter
    {
        (
            "SELECT al.id, al.api_key_id, ak.name, al.action, al.resource_type, al.resource_id, al.details, al.status, al.created_at
             FROM audit_log al
             LEFT JOIN api_keys ak ON ak.id = al.api_key_id
             WHERE al.api_key_id = ?1
             ORDER BY al.created_at DESC
             LIMIT ?2 OFFSET ?3"
                .to_string(),
            vec![
                Box::new(kid.to_string()) as Box<dyn rusqlite::types::ToSql>,
                Box::new(limit),
                Box::new(offset),
            ],
        )
    } else {
        (
            "SELECT al.id, al.api_key_id, ak.name, al.action, al.resource_type, al.resource_id, al.details, al.status, al.created_at
             FROM audit_log al
             LEFT JOIN api_keys ak ON ak.id = al.api_key_id
             ORDER BY al.created_at DESC
             LIMIT ?1 OFFSET ?2"
                .to_string(),
            vec![
                Box::new(limit) as Box<dyn rusqlite::types::ToSql>,
                Box::new(offset),
            ],
        )
    };

    let mut stmt = conn.prepare(&sql).unwrap();
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = filter_params.iter().map(|p| p.as_ref()).collect();

    stmt.query_map(params_refs.as_slice(), |row| {
        Ok(AuditEntry {
            id: row.get(0)?,
            api_key_id: row.get(1)?,
            key_name: row.get(2)?,
            action: row.get(3)?,
            resource_type: row.get(4)?,
            resource_id: row.get(5)?,
            details: row.get(6)?,
            status: row.get(7)?,
            created_at: row.get(8)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

pub async fn create_key_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<(StatusCode, Json<CreateKeyResponse>), (StatusCode, Json<serde_json::Value>)> {
    if !VALID_PERMISSIONS.contains(&req.permission.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("invalid permission: {}", req.permission)})),
        ));
    }

    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    let (raw_key, api_key) =
        create_api_key(&conn, &req.name, &req.permission, req.account_id.as_deref()).map_err(
            |e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e})),
                )
            },
        )?;

    Ok((
        StatusCode::CREATED,
        Json(CreateKeyResponse {
            key: raw_key,
            id: api_key.id,
            name: api_key.name,
            permission: api_key.permission,
            key_prefix: api_key.key_prefix,
        }),
    ))
}

pub async fn list_keys_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ApiKey>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(list_api_keys(&conn)))
}

pub async fn revoke_key_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if revoke_api_key(&conn, &id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

pub async fn get_audit_log_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AuditLogQuery>,
) -> Result<Json<Vec<AuditEntry>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);
    let entries = get_audit_log(&conn, params.api_key_id.as_deref(), limit, offset);
    Ok(Json(entries))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    fn setup() -> Conn {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        // Migration 003 is run by create_test_pool via migrations::run
        // Verify the api_keys table exists
        conn.execute("SELECT 1 FROM api_keys LIMIT 0", []).unwrap();
        conn
    }

    #[test]
    fn test_create_api_key() {
        let conn = setup();
        let (key, stored) = create_api_key(&conn, "Test Key", "read_only", None).unwrap();
        assert!(key.starts_with("iris_"));
        assert_eq!(key.len(), 37); // "iris_" (5) + 32 hex chars
        assert_eq!(stored.name, "Test Key");
        assert_eq!(stored.permission, "read_only");
        assert!(!stored.is_revoked);
    }

    #[test]
    fn test_create_api_key_invalid_permission() {
        let conn = setup();
        let result = create_api_key(&conn, "Bad Key", "superadmin", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_api_key() {
        let conn = setup();
        let (key, _) = create_api_key(&conn, "Validate Test", "read_only", None).unwrap();
        let found = validate_api_key(&conn, &key);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Validate Test");
    }

    #[test]
    fn test_validate_invalid_key() {
        let conn = setup();
        let found = validate_api_key(&conn, "iris_notarealkey000000000000000000");
        assert!(found.is_none());
    }

    #[test]
    fn test_revoke_api_key() {
        let conn = setup();
        let (key, stored) = create_api_key(&conn, "Revoke Test", "read_only", None).unwrap();
        assert!(revoke_api_key(&conn, &stored.id));
        assert!(validate_api_key(&conn, &key).is_none());
    }

    #[test]
    fn test_revoke_nonexistent_key() {
        let conn = setup();
        assert!(!revoke_api_key(&conn, "nonexistent-id"));
    }

    #[test]
    fn test_list_api_keys() {
        let conn = setup();
        create_api_key(&conn, "Key A", "read_only", None).unwrap();
        create_api_key(&conn, "Key B", "draft_only", None).unwrap();
        let keys = list_api_keys(&conn);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_list_excludes_revoked() {
        let conn = setup();
        let (_, key_a) = create_api_key(&conn, "Key A", "read_only", None).unwrap();
        create_api_key(&conn, "Key B", "draft_only", None).unwrap();
        revoke_api_key(&conn, &key_a.id);
        let keys = list_api_keys(&conn);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].name, "Key B");
    }

    #[test]
    fn test_permission_hierarchy() {
        // autonomous allows everything
        assert!(has_permission("autonomous", "read"));
        assert!(has_permission("autonomous", "search"));
        assert!(has_permission("autonomous", "draft"));
        assert!(has_permission("autonomous", "send"));
        assert!(has_permission("autonomous", "execute"));
        assert!(has_permission("autonomous", "configure"));

        // send_with_approval allows read, search, draft, send
        assert!(has_permission("send_with_approval", "read"));
        assert!(has_permission("send_with_approval", "search"));
        assert!(has_permission("send_with_approval", "draft"));
        assert!(has_permission("send_with_approval", "send"));
        assert!(!has_permission("send_with_approval", "execute"));

        // draft_only allows read, search, draft
        assert!(has_permission("draft_only", "read"));
        assert!(has_permission("draft_only", "search"));
        assert!(has_permission("draft_only", "draft"));
        assert!(!has_permission("draft_only", "send"));

        // read_only allows read, search
        assert!(has_permission("read_only", "read"));
        assert!(has_permission("read_only", "search"));
        assert!(!has_permission("read_only", "draft"));
        assert!(!has_permission("read_only", "send"));
    }

    #[test]
    fn test_log_audit_entry() {
        let conn = setup();
        let (_, stored) = create_api_key(&conn, "Audit Test", "read_only", None).unwrap();
        log_audit(
            &conn,
            &stored.id,
            "search",
            Some("message"),
            None,
            None,
            "success",
        );
        let entries = get_audit_log(&conn, Some(&stored.id), 50, 0);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].action, "search");
        assert_eq!(entries[0].api_key_id, stored.id);
        assert_eq!(entries[0].key_name.as_deref(), Some("Audit Test"));
        assert_eq!(entries[0].status, "success");
    }

    #[test]
    fn test_audit_log_filtering() {
        let conn = setup();
        let (_, key_a) = create_api_key(&conn, "Agent A", "read_only", None).unwrap();
        let (_, key_b) = create_api_key(&conn, "Agent B", "draft_only", None).unwrap();

        log_audit(&conn, &key_a.id, "search", Some("message"), None, None, "success");
        log_audit(&conn, &key_b.id, "draft", Some("message"), None, None, "success");
        log_audit(&conn, &key_a.id, "read", Some("thread"), Some("t1"), None, "success");

        // All entries
        let all = get_audit_log(&conn, None, 50, 0);
        assert_eq!(all.len(), 3);

        // Filtered by key_a
        let filtered = get_audit_log(&conn, Some(&key_a.id), 50, 0);
        assert_eq!(filtered.len(), 2);

        // Limit/offset
        let page = get_audit_log(&conn, None, 1, 0);
        assert_eq!(page.len(), 1);
    }
}
