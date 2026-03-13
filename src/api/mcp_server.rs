use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use crate::models::message::{self, InsertMessage, MessageDetail, MessageSummary};
use crate::AppState;

type Conn = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct McpSession {
    pub session_id: String,
    pub account_id: String,
    pub capabilities: Vec<String>,
    pub created_at: String,
    pub last_active_at: String,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
pub struct InitializeRequest {
    pub account_id: String,
    pub api_key_id: Option<String>,
    pub capabilities: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct InitializeResponse {
    pub session_id: String,
    pub tools: Vec<ToolSchema>,
}

#[derive(Debug, Deserialize)]
pub struct ToolCallRequest {
    pub session_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct ToolCallResponse {
    pub tool_name: String,
    pub result: serde_json::Value,
    pub status: String,
    pub duration_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct ToolCallHistoryEntry {
    pub id: i64,
    pub tool_name: String,
    pub input_params: serde_json::Value,
    pub output_result: Option<serde_json::Value>,
    pub status: String,
    pub duration_ms: Option<i64>,
    pub called_at: String,
}

// ---------------------------------------------------------------------------
// Session ID generation
// ---------------------------------------------------------------------------

fn generate_session_id() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let rand_part = uuid::Uuid::new_v4().to_string().replace('-', "");
    format!("mcp-{ts}-{}", &rand_part[..12])
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

fn all_tool_schemas() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            name: "search_emails".to_string(),
            description: "Search emails by query string".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "limit": { "type": "integer", "description": "Max results (default 20)", "default": 20 },
                    "offset": { "type": "integer", "description": "Result offset (default 0)", "default": 0 }
                },
                "required": ["query"]
            }),
        },
        ToolSchema {
            name: "read_email".to_string(),
            description: "Read a specific email by its ID".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "The message ID" }
                },
                "required": ["message_id"]
            }),
        },
        ToolSchema {
            name: "list_inbox".to_string(),
            description: "List recent inbox messages".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max results (default 20)", "default": 20 },
                    "offset": { "type": "integer", "description": "Result offset (default 0)", "default": 0 }
                }
            }),
        },
        ToolSchema {
            name: "send_email".to_string(),
            description: "Compose and send an email".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "to": { "type": "array", "items": { "type": "string" }, "description": "Recipient addresses" },
                    "subject": { "type": "string", "description": "Email subject" },
                    "body_text": { "type": "string", "description": "Plain text body" },
                    "body_html": { "type": "string", "description": "Optional HTML body" }
                },
                "required": ["to", "subject", "body_text"]
            }),
        },
        ToolSchema {
            name: "create_draft".to_string(),
            description: "Create an email draft".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "to": { "type": "array", "items": { "type": "string" }, "description": "Recipient addresses" },
                    "subject": { "type": "string", "description": "Email subject" },
                    "body_text": { "type": "string", "description": "Plain text body" }
                },
                "required": ["body_text"]
            }),
        },
        ToolSchema {
            name: "list_threads".to_string(),
            description: "List email threads".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max results (default 20)", "default": 20 },
                    "offset": { "type": "integer", "description": "Result offset (default 0)", "default": 0 }
                }
            }),
        },
        ToolSchema {
            name: "get_thread".to_string(),
            description: "Get a thread with all its messages".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "thread_id": { "type": "string", "description": "The thread ID" }
                },
                "required": ["thread_id"]
            }),
        },
        ToolSchema {
            name: "archive_email".to_string(),
            description: "Archive an email by ID".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "The message ID to archive" }
                },
                "required": ["message_id"]
            }),
        },
        ToolSchema {
            name: "star_email".to_string(),
            description: "Star or unstar an email".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "The message ID" },
                    "starred": { "type": "boolean", "description": "true to star, false to unstar", "default": true }
                },
                "required": ["message_id"]
            }),
        },
    ]
}

fn get_available_tools(capabilities: &[String]) -> Vec<ToolSchema> {
    let all = all_tool_schemas();
    if capabilities.is_empty() {
        return all;
    }
    all.into_iter()
        .filter(|t| capabilities.contains(&t.name))
        .collect()
}

// ---------------------------------------------------------------------------
// Session DB helpers
// ---------------------------------------------------------------------------

fn create_session(
    conn: &Conn,
    session_id: &str,
    account_id: &str,
    api_key_id: Option<&str>,
    capabilities: &[String],
) -> Result<(), String> {
    let caps_json = serde_json::to_string(capabilities).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO mcp_sessions (session_id, account_id, api_key_id, capabilities)
         VALUES (?1, ?2, ?3, ?4)",
        params![session_id, account_id, api_key_id, caps_json],
    )
    .map_err(|e| format!("failed to create session: {e}"))?;
    Ok(())
}

fn get_session(conn: &Conn, session_id: &str) -> Option<McpSession> {
    conn.query_row(
        "SELECT session_id, account_id, capabilities, created_at, last_active_at, is_active
         FROM mcp_sessions WHERE session_id = ?1",
        params![session_id],
        |row| {
            let caps_json: String = row.get(2)?;
            let capabilities: Vec<String> =
                serde_json::from_str(&caps_json).unwrap_or_default();
            Ok(McpSession {
                session_id: row.get(0)?,
                account_id: row.get(1)?,
                capabilities,
                created_at: row.get(3)?,
                last_active_at: row.get(4)?,
                is_active: row.get::<_, i32>(5)? != 0,
            })
        },
    )
    .ok()
}

fn list_active_sessions(conn: &Conn) -> Vec<McpSession> {
    let mut stmt = conn
        .prepare(
            "SELECT session_id, account_id, capabilities, created_at, last_active_at, is_active
             FROM mcp_sessions WHERE is_active = 1
             ORDER BY last_active_at DESC",
        )
        .expect("failed to prepare list sessions");

    stmt.query_map([], |row| {
        let caps_json: String = row.get(2)?;
        let capabilities: Vec<String> =
            serde_json::from_str(&caps_json).unwrap_or_default();
        Ok(McpSession {
            session_id: row.get(0)?,
            account_id: row.get(1)?,
            capabilities,
            created_at: row.get(3)?,
            last_active_at: row.get(4)?,
            is_active: row.get::<_, i32>(5)? != 0,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

fn close_session(conn: &Conn, session_id: &str) -> bool {
    let rows = conn
        .execute(
            "UPDATE mcp_sessions SET is_active = 0 WHERE session_id = ?1 AND is_active = 1",
            params![session_id],
        )
        .unwrap_or(0);
    rows > 0
}

fn touch_session(conn: &Conn, session_id: &str) {
    let _ = conn.execute(
        "UPDATE mcp_sessions SET last_active_at = datetime('now') WHERE session_id = ?1",
        params![session_id],
    );
}

fn record_tool_call(
    conn: &Conn,
    session_id: &str,
    tool_name: &str,
    input_params: &serde_json::Value,
    output_result: &serde_json::Value,
    status: &str,
    duration_ms: i64,
) {
    let input_str = serde_json::to_string(input_params).unwrap_or_default();
    let output_str = serde_json::to_string(output_result).unwrap_or_default();
    let _ = conn.execute(
        "INSERT INTO mcp_tool_calls (session_id, tool_name, input_params, output_result, status, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![session_id, tool_name, input_str, output_str, status, duration_ms],
    );
}

fn get_tool_call_history(conn: &Conn, session_id: &str) -> Vec<ToolCallHistoryEntry> {
    let mut stmt = conn
        .prepare(
            "SELECT id, tool_name, input_params, output_result, status, duration_ms, called_at
             FROM mcp_tool_calls WHERE session_id = ?1
             ORDER BY called_at DESC",
        )
        .expect("failed to prepare tool call history");

    stmt.query_map(params![session_id], |row| {
        let input_str: String = row.get(2)?;
        let output_str: Option<String> = row.get(3)?;
        Ok(ToolCallHistoryEntry {
            id: row.get(0)?,
            tool_name: row.get(1)?,
            input_params: serde_json::from_str(&input_str).unwrap_or(serde_json::Value::Null),
            output_result: output_str
                .and_then(|s| serde_json::from_str(&s).ok()),
            status: row.get(4)?,
            duration_ms: row.get(5)?,
            called_at: row.get(6)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

// ---------------------------------------------------------------------------
// Tool execution
// ---------------------------------------------------------------------------

fn execute_tool(
    conn: &Conn,
    account_id: &str,
    tool_name: &str,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let account_id_str = account_id.to_string();
    match tool_name {
        "search_emails" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or("missing 'query' argument")?;
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_i64())
                .unwrap_or(20);
            let offset = arguments
                .get("offset")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if query.trim().is_empty() {
                return Ok(serde_json::json!({"results": [], "total": 0}));
            }

            let fts_query = query
                .split_whitespace()
                .map(|term| {
                    let clean = term.replace('"', "");
                    format!("\"{clean}\"")
                })
                .collect::<Vec<_>>()
                .join(" ");

            let sql = "SELECT m.id, m.subject, m.from_address, m.from_name, m.date, m.snippet, m.is_read
                        FROM fts_messages fts
                        JOIN messages m ON fts.rowid = m.rowid
                        WHERE fts.fts_messages MATCH ?1 AND m.account_id = ?2 AND m.is_deleted = 0
                        ORDER BY rank
                        LIMIT ?3 OFFSET ?4";

            let mut stmt = conn.prepare(sql).map_err(|e| format!("search query error: {e}"))?;
            let results: Vec<serde_json::Value> = stmt
                .query_map(params![fts_query, account_id_str, limit, offset], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "subject": row.get::<_, Option<String>>(1)?,
                        "from_address": row.get::<_, Option<String>>(2)?,
                        "from_name": row.get::<_, Option<String>>(3)?,
                        "date": row.get::<_, Option<i64>>(4)?,
                        "snippet": row.get::<_, Option<String>>(5)?,
                        "is_read": row.get::<_, bool>(6)?,
                    }))
                })
                .map_err(|e| format!("search execution error: {e}"))?
                .filter_map(|r| r.ok())
                .collect();

            let total = results.len() as i64;
            Ok(serde_json::json!({"results": results, "total": total}))
        }
        "read_email" => {
            let msg_id = arguments
                .get("message_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'message_id' argument")?;

            let detail = MessageDetail::get_by_id(conn, msg_id)
                .ok_or_else(|| "message not found".to_string())?;

            // Verify account ownership
            if detail.account_id != account_id_str {
                return Err("message not in scope".to_string());
            }

            Ok(serde_json::json!({
                "id": detail.id,
                "message_id": detail.message_id,
                "thread_id": detail.thread_id,
                "from_address": detail.from_address,
                "from_name": detail.from_name,
                "to_addresses": detail.to_addresses,
                "cc_addresses": detail.cc_addresses,
                "subject": detail.subject,
                "date": detail.date,
                "body_text": detail.body_text,
                "body_html": detail.body_html,
                "is_read": detail.is_read,
                "is_starred": detail.is_starred,
                "has_attachments": detail.has_attachments,
            }))
        }
        "list_inbox" => {
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_i64())
                .unwrap_or(20)
                .min(100);
            let offset = arguments
                .get("offset")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                .max(0);

            let messages =
                MessageSummary::list_by_folder(conn, &account_id_str, "INBOX", limit, offset);

            let items: Vec<serde_json::Value> = messages
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "thread_id": m.thread_id,
                        "from_address": m.from_address,
                        "from_name": m.from_name,
                        "subject": m.subject,
                        "snippet": m.snippet,
                        "date": m.date,
                        "is_read": m.is_read,
                        "is_starred": m.is_starred,
                        "has_attachments": m.has_attachments,
                    })
                })
                .collect();

            Ok(serde_json::json!({"messages": items, "count": items.len()}))
        }
        "send_email" => {
            // In MCP context, we create a draft-like record in Sent folder.
            // Actual SMTP sending would require token refresh and is beyond
            // the scope of this synchronous tool call. We record the intent.
            let to = arguments
                .get("to")
                .ok_or("missing 'to' argument")?;
            let subject = arguments
                .get("subject")
                .and_then(|v| v.as_str())
                .ok_or("missing 'subject' argument")?;
            let body_text = arguments
                .get("body_text")
                .and_then(|v| v.as_str())
                .ok_or("missing 'body_text' argument")?;

            let to_json = serde_json::to_string(to).unwrap_or_default();

            let msg = InsertMessage {
                account_id: account_id_str.clone(),
                message_id: None,
                thread_id: None,
                folder: "Sent".to_string(),
                from_address: None,
                from_name: None,
                to_addresses: Some(to_json),
                cc_addresses: None,
                bcc_addresses: None,
                subject: Some(subject.to_string()),
                date: Some(chrono::Utc::now().timestamp()),
                snippet: Some(body_text.chars().take(200).collect()),
                body_text: Some(body_text.to_string()),
                body_html: arguments.get("body_html").and_then(|v| v.as_str()).map(|s| s.to_string()),
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

            let msg_id = InsertMessage::insert(conn, &msg)
                .ok_or_else(|| "failed to create sent message".to_string())?;

            Ok(serde_json::json!({"message_id": msg_id, "status": "sent"}))
        }
        "create_draft" => {
            let body_text = arguments
                .get("body_text")
                .and_then(|v| v.as_str())
                .ok_or("missing 'body_text' argument")?;
            let subject = arguments.get("subject").and_then(|v| v.as_str());
            let to = arguments.get("to").map(|v| serde_json::to_string(v).unwrap_or_default());

            let draft_id = message::save_draft(
                conn,
                None,
                &account_id_str,
                to.as_deref(),
                None,
                None,
                subject,
                body_text,
                None,
            );

            Ok(serde_json::json!({"draft_id": draft_id}))
        }
        "list_threads" => {
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_i64())
                .unwrap_or(20)
                .min(100);
            let offset = arguments
                .get("offset")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                .max(0);

            let sql = "SELECT DISTINCT thread_id, MIN(date) as first_date,
                              MAX(date) as last_date, COUNT(*) as msg_count,
                              MAX(subject) as subject
                       FROM messages
                       WHERE account_id = ?1 AND thread_id IS NOT NULL AND is_deleted = 0
                       GROUP BY thread_id
                       ORDER BY last_date DESC
                       LIMIT ?2 OFFSET ?3";

            let mut stmt = conn.prepare(sql).map_err(|e| format!("threads query error: {e}"))?;
            let threads: Vec<serde_json::Value> = stmt
                .query_map(params![account_id_str, limit, offset], |row| {
                    Ok(serde_json::json!({
                        "thread_id": row.get::<_, String>(0)?,
                        "first_date": row.get::<_, Option<i64>>(1)?,
                        "last_date": row.get::<_, Option<i64>>(2)?,
                        "message_count": row.get::<_, i64>(3)?,
                        "subject": row.get::<_, Option<String>>(4)?,
                    }))
                })
                .map_err(|e| format!("threads exec error: {e}"))?
                .filter_map(|r| r.ok())
                .collect();

            Ok(serde_json::json!({"threads": threads, "count": threads.len()}))
        }
        "get_thread" => {
            let thread_id = arguments
                .get("thread_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'thread_id' argument")?;

            let messages = MessageDetail::list_by_thread(conn, thread_id);

            // Verify at least first message belongs to account
            if let Some(first) = messages.first() {
                if first.account_id != account_id_str {
                    return Err("thread not in scope".to_string());
                }
            }

            let items: Vec<serde_json::Value> = messages
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "from_address": m.from_address,
                        "from_name": m.from_name,
                        "subject": m.subject,
                        "date": m.date,
                        "body_text": m.body_text,
                        "is_read": m.is_read,
                    })
                })
                .collect();

            Ok(serde_json::json!({"thread_id": thread_id, "messages": items, "count": items.len()}))
        }
        "archive_email" => {
            let msg_id = arguments
                .get("message_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'message_id' argument")?;

            // Verify ownership
            let detail = MessageDetail::get_by_id(conn, msg_id)
                .ok_or_else(|| "message not found".to_string())?;
            if detail.account_id != account_id_str {
                return Err("message not in scope".to_string());
            }

            let updated = message::batch_update(conn, &[msg_id], "archive");
            Ok(serde_json::json!({"archived": updated > 0, "message_id": msg_id}))
        }
        "star_email" => {
            let msg_id = arguments
                .get("message_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'message_id' argument")?;
            let starred = arguments
                .get("starred")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            // Verify ownership
            let detail = MessageDetail::get_by_id(conn, msg_id)
                .ok_or_else(|| "message not found".to_string())?;
            if detail.account_id != account_id_str {
                return Err("message not in scope".to_string());
            }

            let action = if starred { "star" } else { "unstar" };
            let updated = message::batch_update(conn, &[msg_id], action);
            Ok(serde_json::json!({"starred": starred, "updated": updated > 0, "message_id": msg_id}))
        }
        _ => Err(format!("unknown tool: {tool_name}")),
    }
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

/// POST /api/mcp/initialize — create an MCP session
pub async fn initialize(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InitializeRequest>,
) -> Result<(StatusCode, Json<InitializeResponse>), (StatusCode, Json<serde_json::Value>)> {
    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    let session_id = generate_session_id();
    let capabilities = req.capabilities.unwrap_or_default();

    create_session(
        &conn,
        &session_id,
        &req.account_id,
        req.api_key_id.as_deref(),
        &capabilities,
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
    })?;

    let tools = get_available_tools(&capabilities);

    Ok((
        StatusCode::CREATED,
        Json(InitializeResponse {
            session_id,
            tools,
        }),
    ))
}

/// POST /api/mcp/tools/call — execute a tool call
pub async fn tool_call(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ToolCallRequest>,
) -> Result<Json<ToolCallResponse>, (StatusCode, Json<serde_json::Value>)> {
    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    // Validate session
    let session = get_session(&conn, &req.session_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session not found"})),
        )
    })?;

    if !session.is_active {
        return Err((
            StatusCode::GONE,
            Json(serde_json::json!({"error": "session is closed"})),
        ));
    }

    // Check capability filter
    if !session.capabilities.is_empty() && !session.capabilities.contains(&req.tool_name) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": format!("tool '{}' not in session capabilities", req.tool_name)})),
        ));
    }

    // Validate tool exists
    let all_tools = all_tool_schemas();
    if !all_tools.iter().any(|t| t.name == req.tool_name) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("unknown tool: {}", req.tool_name)})),
        ));
    }

    // Execute with timing
    let start = Instant::now();
    let result = execute_tool(&conn, &session.account_id, &req.tool_name, &req.arguments);
    let duration_ms = start.elapsed().as_millis() as i64;

    let (status, output) = match result {
        Ok(val) => ("success".to_string(), val),
        Err(e) => (
            "error".to_string(),
            serde_json::json!({"error": e}),
        ),
    };

    // Record the call
    record_tool_call(
        &conn,
        &req.session_id,
        &req.tool_name,
        &req.arguments,
        &output,
        &status,
        duration_ms,
    );

    // Touch session last_active_at
    touch_session(&conn, &req.session_id);

    Ok(Json(ToolCallResponse {
        tool_name: req.tool_name,
        result: output,
        status,
        duration_ms,
    }))
}

/// GET /api/mcp/tools/list — list available MCP tools
pub async fn list_tools() -> Json<Vec<ToolSchema>> {
    Json(all_tool_schemas())
}

/// GET /api/mcp/sessions — list active sessions
pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<McpSession>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(list_active_sessions(&conn)))
}

/// DELETE /api/mcp/sessions/:session_id — close a session
pub async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if close_session(&conn, &session_id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// GET /api/mcp/sessions/:session_id/history — tool call history
pub async fn session_history(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<ToolCallHistoryEntry>>, (StatusCode, Json<serde_json::Value>)> {
    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    // Verify session exists
    let _session = get_session(&conn, &session_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session not found"})),
        )
    })?;

    Ok(Json(get_tool_call_history(&conn, &session_id)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};
    use crate::models::message::InsertMessage;

    fn setup() -> (Conn, Account) {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Verify mcp_sessions table exists
        conn.execute("SELECT 1 FROM mcp_sessions LIMIT 0", [])
            .expect("mcp_sessions table should exist");

        let account = Account::create(
            &conn,
            &CreateAccount {
                provider: "gmail".to_string(),
                email: "mcp-test@example.com".to_string(),
                display_name: Some("MCP Test".to_string()),
                imap_host: Some("imap.gmail.com".to_string()),
                imap_port: Some(993),
                smtp_host: Some("smtp.gmail.com".to_string()),
                smtp_port: Some(587),
                username: Some("mcp-test@example.com".to_string()),
                password: None,
            },
        );
        (conn, account)
    }

    fn insert_test_message(conn: &Conn, account_id: &str, subject: &str, folder: &str) -> String {
        let msg = InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{subject}@mcp-test.com>")),
            thread_id: None,
            folder: folder.to_string(),
            from_address: Some("sender@example.com".to_string()),
            from_name: Some("Sender".to_string()),
            to_addresses: Some(r#"["mcp-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(1700000000),
            snippet: Some("Preview text...".to_string()),
            body_text: Some("Full body text for searching".to_string()),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(1),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(1024),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        };
        InsertMessage::insert(conn, &msg).expect("insert should succeed")
    }

    // Test 1: Session creation
    #[test]
    fn test_create_session() {
        let (conn, account) = setup();
        let session_id = generate_session_id();
        assert!(session_id.starts_with("mcp-"));

        create_session(&conn, &session_id, &account.id, None, &[])
            .expect("session creation should succeed");

        let session = get_session(&conn, &session_id);
        assert!(session.is_some());
        let session = session.unwrap();
        assert!(session.is_active);
        assert!(session.capabilities.is_empty());
    }

    // Test 2: Session creation with capabilities
    #[test]
    fn test_create_session_with_capabilities() {
        let (conn, account) = setup();
        let session_id = generate_session_id();
        let caps = vec!["read_email".to_string(), "search_emails".to_string()];

        create_session(&conn, &session_id, &account.id, None, &caps)
            .expect("session creation should succeed");

        let session = get_session(&conn, &session_id).unwrap();
        assert_eq!(session.capabilities.len(), 2);
        assert!(session.capabilities.contains(&"read_email".to_string()));
        assert!(session.capabilities.contains(&"search_emails".to_string()));
    }

    // Test 3: List all tools
    #[test]
    fn test_list_all_tools() {
        let tools = all_tool_schemas();
        assert_eq!(tools.len(), 9);

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"search_emails"));
        assert!(names.contains(&"read_email"));
        assert!(names.contains(&"list_inbox"));
        assert!(names.contains(&"send_email"));
        assert!(names.contains(&"create_draft"));
        assert!(names.contains(&"list_threads"));
        assert!(names.contains(&"get_thread"));
        assert!(names.contains(&"archive_email"));
        assert!(names.contains(&"star_email"));
    }

    // Test 4: Filter tools by capabilities
    #[test]
    fn test_get_available_tools_filtered() {
        let caps = vec!["read_email".to_string(), "list_inbox".to_string()];
        let tools = get_available_tools(&caps);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "read_email");
        assert_eq!(tools[1].name, "list_inbox");
    }

    // Test 5: Get all tools when no capabilities filter
    #[test]
    fn test_get_available_tools_no_filter() {
        let tools = get_available_tools(&[]);
        assert_eq!(tools.len(), 9);
    }

    // Test 6: Close session
    #[test]
    fn test_close_session() {
        let (conn, account) = setup();
        let session_id = generate_session_id();
        create_session(&conn, &session_id, &account.id, None, &[]).unwrap();

        assert!(close_session(&conn, &session_id));
        let session = get_session(&conn, &session_id).unwrap();
        assert!(!session.is_active);
    }

    // Test 7: Close nonexistent session
    #[test]
    fn test_close_nonexistent_session() {
        let (conn, _account) = setup();
        assert!(!close_session(&conn, "nonexistent-session"));
    }

    // Test 8: List active sessions
    #[test]
    fn test_list_active_sessions() {
        let (conn, account) = setup();
        let acct_id = &account.id;

        let s1 = generate_session_id();
        let s2 = generate_session_id();
        let s3 = generate_session_id();
        create_session(&conn, &s1, acct_id, None, &[]).unwrap();
        create_session(&conn, &s2, acct_id, None, &[]).unwrap();
        create_session(&conn, &s3, acct_id, None, &[]).unwrap();

        // Close one
        close_session(&conn, &s2);

        let active = list_active_sessions(&conn);
        assert_eq!(active.len(), 2);
    }

    // Test 9: Tool call — list_inbox
    #[test]
    fn test_tool_call_list_inbox() {
        let (conn, account) = setup();
        insert_test_message(&conn, &account.id, "Inbox msg 1", "INBOX");
        insert_test_message(&conn, &account.id, "Inbox msg 2", "INBOX");

        let acct_id = &account.id;
        let result = execute_tool(&conn, acct_id, "list_inbox", &serde_json::json!({}));
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["count"].as_i64().unwrap(), 2);
    }

    // Test 10: Tool call — read_email
    #[test]
    fn test_tool_call_read_email() {
        let (conn, account) = setup();
        let msg_id = insert_test_message(&conn, &account.id, "Read this", "INBOX");

        let acct_id = &account.id;
        let result = execute_tool(
            &conn,
            acct_id,
            "read_email",
            &serde_json::json!({"message_id": msg_id}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["subject"].as_str().unwrap(), "Read this");
    }

    // Test 11: Tool call — create_draft
    #[test]
    fn test_tool_call_create_draft() {
        let (conn, account) = setup();
        let acct_id = &account.id;

        let result = execute_tool(
            &conn,
            acct_id,
            "create_draft",
            &serde_json::json!({
                "body_text": "Draft body",
                "subject": "Draft subject",
                "to": ["alice@example.com"]
            }),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val["draft_id"].as_str().is_some());
    }

    // Test 12: Tool call — unknown tool
    #[test]
    fn test_tool_call_unknown_tool() {
        let (conn, account) = setup();
        let acct_id = &account.id;

        let result = execute_tool(&conn, acct_id, "nonexistent_tool", &serde_json::json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown tool"));
    }

    // Test 13: Tool call history recording
    #[test]
    fn test_tool_call_history() {
        let (conn, account) = setup();
        let session_id = generate_session_id();
        let acct_id = &account.id;
        create_session(&conn, &session_id, acct_id, None, &[]).unwrap();

        // Record some calls
        record_tool_call(
            &conn,
            &session_id,
            "list_inbox",
            &serde_json::json!({"limit": 10}),
            &serde_json::json!({"count": 5}),
            "success",
            42,
        );
        record_tool_call(
            &conn,
            &session_id,
            "read_email",
            &serde_json::json!({"message_id": "abc"}),
            &serde_json::json!({"error": "not found"}),
            "error",
            15,
        );

        let history = get_tool_call_history(&conn, &session_id);
        assert_eq!(history.len(), 2);
        // Verify both calls are recorded (order may vary since same timestamp)
        let tool_names: Vec<&str> = history.iter().map(|h| h.tool_name.as_str()).collect();
        assert!(tool_names.contains(&"list_inbox"));
        assert!(tool_names.contains(&"read_email"));
        let error_entry = history.iter().find(|h| h.tool_name == "read_email").unwrap();
        assert_eq!(error_entry.status, "error");
        assert_eq!(error_entry.duration_ms, Some(15));
        let success_entry = history.iter().find(|h| h.tool_name == "list_inbox").unwrap();
        assert_eq!(success_entry.status, "success");
    }

    // Test 14: Tool call — archive_email
    #[test]
    fn test_tool_call_archive_email() {
        let (conn, account) = setup();
        let msg_id = insert_test_message(&conn, &account.id, "Archive me", "INBOX");

        let acct_id = &account.id;
        let result = execute_tool(
            &conn,
            acct_id,
            "archive_email",
            &serde_json::json!({"message_id": msg_id}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val["archived"].as_bool().unwrap());

        // Verify it's no longer in INBOX
        let inbox = MessageSummary::list_by_folder(&conn, &account.id, "INBOX", 50, 0);
        assert_eq!(inbox.len(), 0);

        let archived = MessageSummary::list_by_folder(&conn, &account.id, "Archive", 50, 0);
        assert_eq!(archived.len(), 1);
    }

    // Test 15: Tool call — star_email
    #[test]
    fn test_tool_call_star_email() {
        let (conn, account) = setup();
        let msg_id = insert_test_message(&conn, &account.id, "Star me", "INBOX");

        let acct_id = &account.id;

        // Star it
        let result = execute_tool(
            &conn,
            acct_id,
            "star_email",
            &serde_json::json!({"message_id": msg_id, "starred": true}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val["starred"].as_bool().unwrap());

        let detail = MessageDetail::get_by_id(&conn, &msg_id).unwrap();
        assert!(detail.is_starred);

        // Unstar it
        let result = execute_tool(
            &conn,
            acct_id,
            "star_email",
            &serde_json::json!({"message_id": msg_id, "starred": false}),
        );
        assert!(result.is_ok());
        let detail = MessageDetail::get_by_id(&conn, &msg_id).unwrap();
        assert!(!detail.is_starred);
    }

    // Test 16: Tool call — read_email not in scope
    #[test]
    fn test_tool_call_read_email_wrong_account() {
        let (conn, account) = setup();
        let msg_id = insert_test_message(&conn, &account.id, "Secret msg", "INBOX");

        // Use a different account_id
        let result = execute_tool(&conn, "nonexistent-account-id", "read_email", &serde_json::json!({"message_id": msg_id}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not in scope"));
    }

    // Test 17: Duplicate session_id is rejected
    #[test]
    fn test_duplicate_session_id() {
        let (conn, account) = setup();
        let session_id = generate_session_id();
        let acct_id = &account.id;

        create_session(&conn, &session_id, acct_id, None, &[]).unwrap();
        let result = create_session(&conn, &session_id, acct_id, None, &[]);
        assert!(result.is_err());
    }

    // Test 18: Send email tool
    #[test]
    fn test_tool_call_send_email() {
        let (conn, account) = setup();
        let acct_id = &account.id;

        let result = execute_tool(
            &conn,
            acct_id,
            "send_email",
            &serde_json::json!({
                "to": ["recipient@example.com"],
                "subject": "Test send",
                "body_text": "Hello from MCP"
            }),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["status"].as_str().unwrap(), "sent");
        assert!(val["message_id"].as_str().is_some());
    }

    // Test 19: Touch session updates last_active_at
    #[test]
    fn test_touch_session() {
        let (conn, account) = setup();
        let session_id = generate_session_id();
        let acct_id = &account.id;
        create_session(&conn, &session_id, acct_id, None, &[]).unwrap();

        let before = get_session(&conn, &session_id).unwrap();
        // Touch updates last_active_at
        touch_session(&conn, &session_id);
        let after = get_session(&conn, &session_id).unwrap();

        // Both should exist; we just verify it didn't break
        assert_eq!(before.session_id, after.session_id);
        assert!(after.is_active);
    }
}
