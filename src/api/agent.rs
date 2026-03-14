use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use axum::Extension;
use axum::Json;
use rand::Rng;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::auth::refresh::ensure_fresh_token;
use crate::models::account::Account;
use crate::models::message::{self, MessageDetail};
use crate::smtp::{self, ComposeRequest};
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
// Bearer token extraction
// ---------------------------------------------------------------------------

pub fn extract_bearer_token(auth_header: &str) -> Option<String> {
    let lower = auth_header.to_lowercase();
    if lower.starts_with("bearer ") {
        Some(auth_header[7..].trim().to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Agent auth middleware
// ---------------------------------------------------------------------------

pub async fn agent_auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = extract_bearer_token(auth_header).ok_or(StatusCode::UNAUTHORIZED)?;

    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let api_key = validate_api_key(&conn, &token).ok_or(StatusCode::UNAUTHORIZED)?;

    request.extensions_mut().insert(api_key);
    Ok(next.run(request).await)
}

// ---------------------------------------------------------------------------
// Agent endpoint request/response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct AgentSearchParams {
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Deserialize)]
pub struct AgentDraftRequest {
    pub account_id: String,
    pub to: Option<Vec<String>>,
    pub subject: Option<String>,
    pub body_text: String,
}

#[derive(Deserialize)]
pub struct AgentSendRequest {
    pub account_id: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

// ---------------------------------------------------------------------------
// Agent endpoint handlers
// ---------------------------------------------------------------------------

/// GET /api/agent/search?q=...
/// Requires "search" permission. Reuses FTS5 search logic.
pub async fn agent_search(
    State(state): State<Arc<AppState>>,
    Extension(api_key): Extension<ApiKey>,
    Query(params): Query<AgentSearchParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !has_permission(&api_key.permission, "search") {
        let conn = state.db.get().ok();
        if let Some(conn) = conn {
            log_audit(&conn, &api_key.id, "search", None, None, None, "forbidden");
        }
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        ));
    }

    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    let query_str = params.q.as_deref().unwrap_or("").trim().to_string();
    if query_str.is_empty() {
        log_audit(
            &conn,
            &api_key.id,
            "search",
            Some("message"),
            None,
            Some("empty query"),
            "success",
        );
        return Ok(Json(serde_json::json!({"results": [], "total": 0, "query": ""})));
    }

    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    // Build FTS query with optional account_id scope
    let mut conditions = Vec::new();
    let mut filter_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    conditions.push(format!("fts.fts_messages MATCH ?{param_idx}"));
    let fts_query = query_str
        .split_whitespace()
        .map(|term| {
            let clean = term.replace('"', "");
            format!("\"{clean}\"")
        })
        .collect::<Vec<_>>()
        .join(" ");
    filter_params.push(Box::new(fts_query));
    param_idx += 1;

    // Scope to account if the API key has an account_id
    if let Some(ref acct_id) = api_key.account_id {
        conditions.push(format!("m.account_id = ?{param_idx}"));
        filter_params.push(Box::new(acct_id.clone()));
        param_idx += 1;
    }

    conditions.push("m.is_deleted = 0".to_string());
    let where_clause = conditions.join(" AND ");

    let sql = format!(
        "SELECT m.id, m.account_id, m.thread_id, m.from_address, m.from_name,
                m.subject, snippet(fts, -1, '<mark>', '</mark>', '...', 40) as match_snippet,
                m.date, m.is_read, m.has_attachments
         FROM fts_messages fts
         JOIN messages m ON fts.rowid = m.rowid
         WHERE {where_clause}
         ORDER BY rank
         LIMIT ?{param_idx} OFFSET ?{}",
        param_idx + 1
    );
    filter_params.push(Box::new(limit));
    filter_params.push(Box::new(offset));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        filter_params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        tracing::error!("Agent search query error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "search query error"})),
        )
    })?;

    let results: Vec<serde_json::Value> = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>("id")?,
                "account_id": row.get::<_, String>("account_id")?,
                "thread_id": row.get::<_, Option<String>>("thread_id")?,
                "from_address": row.get::<_, Option<String>>("from_address")?,
                "from_name": row.get::<_, Option<String>>("from_name")?,
                "subject": row.get::<_, Option<String>>("subject")?,
                "snippet": row.get::<_, String>("match_snippet")?,
                "date": row.get::<_, Option<i64>>("date")?,
                "is_read": row.get::<_, bool>("is_read")?,
                "has_attachments": row.get::<_, bool>("has_attachments")?,
            }))
        })
        .map_err(|e| {
            tracing::error!("Agent search execution error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "search execution error"})),
            )
        })?
        .filter_map(|r| r.ok())
        .collect();

    let total = results.len() as i64;

    log_audit(
        &conn,
        &api_key.id,
        "search",
        Some("message"),
        None,
        Some(&format!("q={query_str}, results={total}")),
        "success",
    );

    Ok(Json(serde_json::json!({
        "results": results,
        "total": total,
        "query": query_str,
    })))
}

/// GET /api/agent/messages/{id}
/// Requires "read" permission.
pub async fn agent_get_message(
    State(state): State<Arc<AppState>>,
    Extension(api_key): Extension<ApiKey>,
    Path(id): Path<String>,
) -> Result<Json<MessageDetail>, (StatusCode, Json<serde_json::Value>)> {
    if !has_permission(&api_key.permission, "read") {
        let conn = state.db.get().ok();
        if let Some(conn) = conn {
            log_audit(
                &conn,
                &api_key.id,
                "read",
                Some("message"),
                Some(&id),
                None,
                "forbidden",
            );
        }
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        ));
    }

    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    let msg = MessageDetail::get_by_id(&conn, &id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "message not found"})),
        )
    })?;

    // Scope check: if API key has account_id, ensure message belongs to that account
    if let Some(ref acct_id) = api_key.account_id {
        if msg.account_id != *acct_id {
            log_audit(
                &conn,
                &api_key.id,
                "read",
                Some("message"),
                Some(&id),
                Some("account scope mismatch"),
                "forbidden",
            );
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "message not in scope"})),
            ));
        }
    }

    log_audit(
        &conn,
        &api_key.id,
        "read",
        Some("message"),
        Some(&id),
        None,
        "success",
    );

    Ok(Json(msg))
}

/// GET /api/agent/threads/{id}
/// Requires "read" permission.
pub async fn agent_get_thread(
    State(state): State<Arc<AppState>>,
    Extension(api_key): Extension<ApiKey>,
    Path(id): Path<String>,
) -> Result<Json<Vec<MessageDetail>>, (StatusCode, Json<serde_json::Value>)> {
    if !has_permission(&api_key.permission, "read") {
        let conn = state.db.get().ok();
        if let Some(conn) = conn {
            log_audit(
                &conn,
                &api_key.id,
                "read",
                Some("thread"),
                Some(&id),
                None,
                "forbidden",
            );
        }
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        ));
    }

    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    let messages = MessageDetail::list_by_thread(&conn, &id);

    // Scope check: if API key has account_id, ensure all messages belong to that account
    if let Some(ref acct_id) = api_key.account_id {
        if let Some(first) = messages.first() {
            if first.account_id != *acct_id {
                log_audit(
                    &conn,
                    &api_key.id,
                    "read",
                    Some("thread"),
                    Some(&id),
                    Some("account scope mismatch"),
                    "forbidden",
                );
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"error": "thread not in scope"})),
                ));
            }
        }
    }

    log_audit(
        &conn,
        &api_key.id,
        "read",
        Some("thread"),
        Some(&id),
        Some(&format!("messages={}", messages.len())),
        "success",
    );

    Ok(Json(messages))
}

/// POST /api/agent/drafts
/// Requires "draft" permission.
pub async fn agent_create_draft(
    State(state): State<Arc<AppState>>,
    Extension(api_key): Extension<ApiKey>,
    Json(req): Json<AgentDraftRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !has_permission(&api_key.permission, "draft") {
        let conn = state.db.get().ok();
        if let Some(conn) = conn {
            log_audit(
                &conn,
                &api_key.id,
                "draft",
                Some("message"),
                None,
                None,
                "forbidden",
            );
        }
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        ));
    }

    // Scope check: if API key has account_id, ensure request targets that account
    if let Some(ref acct_id) = api_key.account_id {
        if req.account_id != *acct_id {
            let conn = state.db.get().ok();
            if let Some(conn) = conn {
                log_audit(
                    &conn,
                    &api_key.id,
                    "draft",
                    Some("message"),
                    None,
                    Some("account scope mismatch"),
                    "forbidden",
                );
            }
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "account not in scope"})),
            ));
        }
    }

    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    let to_json = req
        .to
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());

    let draft_id = message::save_draft(
        &conn,
        None, // always create new
        &req.account_id,
        to_json.as_deref(),
        None, // cc
        None, // bcc
        req.subject.as_deref(),
        &req.body_text,
        None, // body_html
    );

    log_audit(
        &conn,
        &api_key.id,
        "draft",
        Some("message"),
        Some(&draft_id),
        None,
        "success",
    );

    Ok(Json(serde_json::json!({"draft_id": draft_id})))
}

/// POST /api/agent/send
/// Requires "send" permission. Reuses compose/SMTP send pipeline.
pub async fn agent_send(
    State(state): State<Arc<AppState>>,
    Extension(api_key): Extension<ApiKey>,
    Json(req): Json<AgentSendRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !has_permission(&api_key.permission, "send") {
        let conn = state.db.get().ok();
        if let Some(conn) = conn {
            log_audit(
                &conn,
                &api_key.id,
                "send",
                Some("message"),
                None,
                None,
                "forbidden",
            );
        }
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        ));
    }

    // Scope check: if API key has account_id, ensure request targets that account
    if let Some(ref acct_id) = api_key.account_id {
        if req.account_id != *acct_id {
            let conn = state.db.get().ok();
            if let Some(conn) = conn {
                log_audit(
                    &conn,
                    &api_key.id,
                    "send",
                    Some("message"),
                    None,
                    Some("account scope mismatch"),
                    "forbidden",
                );
            }
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "account not in scope"})),
            ));
        }
    }

    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    let account = Account::get_by_id(&conn, &req.account_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "account not found"})),
        )
    })?;

    if api_key.permission == "send_with_approval" {
        let to_json = serde_json::to_string(&req.to).ok();
        let cc_json = (!req.cc.is_empty())
            .then(|| serde_json::to_string(&req.cc).ok())
            .flatten();
        let bcc_json = (!req.bcc.is_empty())
            .then(|| serde_json::to_string(&req.bcc).ok())
            .flatten();

        let draft_id = message::save_draft(
            &conn,
            None,
            &req.account_id,
            to_json.as_deref(),
            cc_json.as_deref(),
            bcc_json.as_deref(),
            Some(&req.subject),
            &req.body_text,
            req.body_html.as_deref(),
        );

        log_audit(
            &conn,
            &api_key.id,
            "send",
            Some("message"),
            Some(&draft_id),
            Some("saved as draft pending approval"),
            "success",
        );

        return Ok(Json(serde_json::json!({
            "sent": false,
            "requires_approval": true,
            "draft_id": draft_id,
        })));
    }

    // Refresh OAuth token if needed
    let access_token = ensure_fresh_token(&state.db, &account, &state.config)
        .await
        .map_err(|e| {
            log_audit(
                &conn,
                &api_key.id,
                "send",
                Some("message"),
                None,
                Some(&format!("token refresh failed: {e}")),
                "error",
            );
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("token refresh: {e}")})),
            )
        })?;

    // Build ComposeRequest to reuse existing SMTP pipeline
    let compose_req = ComposeRequest {
        account_id: req.account_id.clone(),
        to: req.to.clone(),
        cc: req.cc.clone(),
        bcc: req.bcc.clone(),
        subject: req.subject.clone(),
        body_text: req.body_text.clone(),
        body_html: req.body_html.clone(),
        in_reply_to: req.in_reply_to.clone(),
        references: req.references.clone(),
        attachments: vec![],
        schedule_at: None,
    };

    // Build email
    let email = smtp::build_email(
        &account.email,
        account.display_name.as_deref(),
        &compose_req,
    )
    .map_err(|e| {
        log_audit(
            &conn,
            &api_key.id,
            "send",
            Some("message"),
            None,
            Some(&format!("build failed: {e}")),
            "error",
        );
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    // Extract the Message-ID that lettre generated
    let rfc_message_id = email
        .headers()
        .get_raw("Message-ID")
        .map(|v| v.to_string());

    // Send via SMTP
    smtp::send_email(&account, access_token.as_deref(), email)
        .await
        .map_err(|e| {
            log_audit(
                &conn,
                &api_key.id,
                "send",
                Some("message"),
                None,
                Some(&format!("smtp failed: {e}")),
                "error",
            );
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    // Store in Sent folder (reuse pattern from compose.rs)
    let to_json = serde_json::to_string(&req.to).ok();
    let cc_json = if req.cc.is_empty() {
        None
    } else {
        serde_json::to_string(&req.cc).ok()
    };
    let bcc_json = if req.bcc.is_empty() {
        None
    } else {
        serde_json::to_string(&req.bcc).ok()
    };

    let sent_msg = message::InsertMessage {
        account_id: req.account_id.clone(),
        message_id: rfc_message_id.clone(),
        thread_id: req
            .in_reply_to
            .as_ref()
            .map(|r| r.trim_matches(|c| c == '<' || c == '>').to_string()),
        folder: "Sent".to_string(),
        from_address: Some(account.email.clone()),
        from_name: account.display_name.clone(),
        to_addresses: to_json,
        cc_addresses: cc_json,
        bcc_addresses: bcc_json,
        subject: Some(req.subject.clone()),
        date: Some(chrono::Utc::now().timestamp()),
        snippet: Some(req.body_text.chars().take(200).collect()),
        body_text: Some(req.body_text.clone()),
        body_html: req.body_html.clone(),
        is_read: true,
        is_starred: false,
        is_draft: false,
        labels: None,
        uid: None,
        modseq: None,
        raw_headers: None,
        has_attachments: false,
        attachment_names: None,
        size_bytes: None,
        list_unsubscribe: None,
        list_unsubscribe_post: false,
    };

    let msg_id = message::InsertMessage::insert(&conn, &sent_msg).expect("sent message should always insert");

    log_audit(
        &conn,
        &api_key.id,
        "send",
        Some("message"),
        Some(&msg_id),
        Some(&format!("to={:?}, subject={}", req.to, req.subject)),
        "success",
    );

    tracing::info!(
        agent = %api_key.name,
        account = %account.email,
        to = ?req.to,
        subject = %req.subject,
        "Agent sent email"
    );

    Ok(Json(serde_json::json!({"message_id": msg_id})))
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
    fn test_extract_bearer_token() {
        assert_eq!(
            extract_bearer_token("Bearer iris_abc123"),
            Some("iris_abc123".to_string())
        );
        assert_eq!(
            extract_bearer_token("bearer iris_abc123"),
            Some("iris_abc123".to_string())
        );
        assert_eq!(extract_bearer_token("Basic abc123"), None);
        assert_eq!(extract_bearer_token(""), None);
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
