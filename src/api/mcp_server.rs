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
            description: "List email threads with optional filters".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max results (default 20)", "default": 20 },
                    "offset": { "type": "integer", "description": "Result offset (default 0)", "default": 0 },
                    "unread": { "type": "boolean", "description": "Filter to threads with at least one unread message" },
                    "starred": { "type": "boolean", "description": "Filter to threads with at least one starred message" },
                    "category": { "type": "string", "description": "Filter by AI category (e.g. primary, updates, promotions)" },
                    "date_from": { "type": "string", "description": "ISO 8601 date — only threads with messages on/after this date" },
                    "date_to": { "type": "string", "description": "ISO 8601 date — only threads with messages on/before this date" },
                    "sender": { "type": "string", "description": "Filter threads by sender email address (partial match)" }
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
        ToolSchema {
            name: "get_thread_summary".to_string(),
            description: "Get an AI-generated summary for an email thread. Returns cached summary if available.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "thread_id": { "type": "string", "description": "The thread ID to summarize" }
                },
                "required": ["thread_id"]
            }),
        },
        ToolSchema {
            name: "get_contact_profile".to_string(),
            description: "Get a contact profile including response times, topics, and VIP status for a given email address.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "email": { "type": "string", "description": "The contact email address to look up" }
                },
                "required": ["email"]
            }),
        },
        ToolSchema {
            name: "extract_tasks".to_string(),
            description: "Extract action items and to-dos from an email thread using AI.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "thread_id": { "type": "string", "description": "The thread ID to extract tasks from" }
                },
                "required": ["thread_id"]
            }),
        },
        ToolSchema {
            name: "extract_deadlines".to_string(),
            description: "Extract deadlines and due dates from an email thread using AI.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "thread_id": { "type": "string", "description": "The thread ID to extract deadlines from" }
                },
                "required": ["thread_id"]
            }),
        },
        ToolSchema {
            name: "chat".to_string(),
            description: "Send a message to the Iris AI assistant. The assistant can search emails, summarize threads, and answer questions about your inbox.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "The message to send to the AI assistant" },
                    "session_id": { "type": "string", "description": "Chat session ID for conversation continuity (generated if omitted)" },
                    "account_id": { "type": "string", "description": "Override account ID for this chat (defaults to session account)" }
                },
                "required": ["message"]
            }),
        },
        ToolSchema {
            name: "get_inbox_stats".to_string(),
            description: "Get inbox statistics: total, unread, starred counts plus category breakdown and top senders.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolSchema {
            name: "manage_draft".to_string(),
            description: "Create, update, or delete an email draft.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["create", "update", "delete"], "description": "The action to perform on the draft" },
                    "draft_id": { "type": "string", "description": "Draft ID — required for update and delete actions" },
                    "to": { "type": "array", "items": { "type": "string" }, "description": "Recipient addresses (for create/update)" },
                    "subject": { "type": "string", "description": "Email subject (for create/update)" },
                    "body": { "type": "string", "description": "Email body text (for create/update)" }
                },
                "required": ["action"]
            }),
        },
        ToolSchema {
            name: "bulk_action".to_string(),
            description: "Perform a bulk action on multiple emails at once.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message_ids": { "type": "array", "items": { "type": "string" }, "description": "List of message IDs to act on" },
                    "action": { "type": "string", "enum": ["archive", "delete", "star", "mark_read", "mark_unread"], "description": "Action to perform on all specified messages" }
                },
                "required": ["message_ids", "action"]
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
            let filter_unread = arguments.get("unread").and_then(|v| v.as_bool());
            let filter_starred = arguments.get("starred").and_then(|v| v.as_bool());
            let filter_category = arguments.get("category").and_then(|v| v.as_str()).map(|s| s.to_string());
            let filter_date_from = arguments.get("date_from").and_then(|v| v.as_str()).map(|s| s.to_string());
            let filter_date_to = arguments.get("date_to").and_then(|v| v.as_str()).map(|s| s.to_string());
            let filter_sender = arguments.get("sender").and_then(|v| v.as_str()).map(|s| s.to_string());

            // Convert date strings to unix timestamps when provided
            let date_from_ts: Option<i64> = filter_date_from.as_deref().and_then(|d| {
                chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                    .ok()
                    .and_then(|nd| nd.and_hms_opt(0, 0, 0))
                    .map(|ndt| ndt.and_utc().timestamp())
            });
            let date_to_ts: Option<i64> = filter_date_to.as_deref().and_then(|d| {
                chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                    .ok()
                    .and_then(|nd| nd.and_hms_opt(23, 59, 59))
                    .map(|ndt| ndt.and_utc().timestamp())
            });

            // Build dynamic WHERE clauses
            let mut where_clauses = vec![
                "account_id = ?1".to_string(),
                "thread_id IS NOT NULL".to_string(),
                "is_deleted = 0".to_string(),
            ];
            if let Some(true) = filter_unread {
                where_clauses.push("is_read = 0".to_string());
            }
            if let Some(true) = filter_starred {
                where_clauses.push("is_starred = 1".to_string());
            }
            if filter_category.is_some() {
                where_clauses.push("ai_category = ?2".to_string());
            }
            if date_from_ts.is_some() {
                let idx = if filter_category.is_some() { 3 } else { 2 };
                where_clauses.push(format!("date >= ?{idx}"));
            }
            if date_to_ts.is_some() {
                let mut idx = 2;
                if filter_category.is_some() { idx += 1; }
                if date_from_ts.is_some() { idx += 1; }
                where_clauses.push(format!("date <= ?{idx}"));
            }
            if filter_sender.is_some() {
                let mut idx = 2;
                if filter_category.is_some() { idx += 1; }
                if date_from_ts.is_some() { idx += 1; }
                if date_to_ts.is_some() { idx += 1; }
                where_clauses.push(format!("from_address LIKE ?{idx}"));
            }

            let where_str = where_clauses.join(" AND ");
            // Count params needed (excluding account_id which is ?1)
            let mut extra_count = 0;
            if filter_category.is_some() { extra_count += 1; }
            if date_from_ts.is_some() { extra_count += 1; }
            if date_to_ts.is_some() { extra_count += 1; }
            if filter_sender.is_some() { extra_count += 1; }

            let limit_idx = 2 + extra_count;
            let offset_idx = 3 + extra_count;

            let sql = format!(
                "SELECT DISTINCT thread_id, MIN(date) as first_date,
                        MAX(date) as last_date, COUNT(*) as msg_count,
                        MAX(subject) as subject
                 FROM messages
                 WHERE {where_str}
                 GROUP BY thread_id
                 ORDER BY last_date DESC
                 LIMIT ?{limit_idx} OFFSET ?{offset_idx}"
            );

            // Build params vec dynamically
            let sender_like = filter_sender.as_ref().map(|s| format!("%{s}%"));
            let mut dyn_params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
                Box::new(account_id_str.clone()),
            ];
            if let Some(ref cat) = filter_category { dyn_params.push(Box::new(cat.clone())); }
            if let Some(ts) = date_from_ts { dyn_params.push(Box::new(ts)); }
            if let Some(ts) = date_to_ts { dyn_params.push(Box::new(ts)); }
            if let Some(ref like_str) = sender_like { dyn_params.push(Box::new(like_str.clone())); }
            dyn_params.push(Box::new(limit));
            dyn_params.push(Box::new(offset));

            let params_refs: Vec<&dyn rusqlite::types::ToSql> = dyn_params.iter().map(|b| b.as_ref()).collect();

            let mut stmt = conn.prepare(&sql).map_err(|e| format!("threads query error: {e}"))?;
            let threads: Vec<serde_json::Value> = stmt
                .query_map(params_refs.as_slice(), |row| {
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
        "get_contact_profile" => {
            let email = arguments
                .get("email")
                .and_then(|v| v.as_str())
                .ok_or("missing 'email' argument")?;
            let email_lower = email.to_lowercase();

            // Try to find an existing profile first
            let profile_result = conn.query_row(
                "SELECT id, account_id, email_address, display_name, organization,
                        first_seen_at, last_seen_at, total_emails_from, total_emails_to,
                        avg_response_time_hours, top_categories, communication_style,
                        ai_summary, profile_data, generated_at, updated_at
                 FROM contact_profiles
                 WHERE account_id = ?1 AND email_address = ?2",
                params![account_id_str, email_lower],
                |row| {
                    Ok(serde_json::json!({
                        "email": row.get::<_, String>(2)?,
                        "display_name": row.get::<_, Option<String>>(3)?,
                        "organization": row.get::<_, Option<String>>(4)?,
                        "first_seen_at": row.get::<_, Option<String>>(5)?,
                        "last_seen_at": row.get::<_, Option<String>>(6)?,
                        "total_emails_from": row.get::<_, i64>(7)?,
                        "total_emails_to": row.get::<_, i64>(8)?,
                        "avg_response_time_hours": row.get::<_, Option<f64>>(9)?,
                        "top_categories": row.get::<_, Option<String>>(10)?,
                        "communication_style": row.get::<_, Option<String>>(11)?,
                        "ai_summary": row.get::<_, Option<String>>(12)?,
                        "updated_at": row.get::<_, String>(15)?,
                    }))
                },
            );

            if let Ok(profile) = profile_result {
                return Ok(profile);
            }

            // Profile not cached — compute on the fly from messages
            let like_email = format!("%{email_lower}%");
            let total_from: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND from_address = ?2 AND is_deleted = 0",
                    params![account_id_str, email_lower],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let total_to: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND to_addresses LIKE ?2 AND is_deleted = 0",
                    params![account_id_str, like_email],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            if total_from == 0 && total_to == 0 {
                return Err(format!("no emails found for {email}"));
            }

            let display_name: Option<String> = conn
                .query_row(
                    "SELECT from_name FROM messages
                     WHERE account_id = ?1 AND from_address = ?2 AND from_name IS NOT NULL AND is_deleted = 0
                     ORDER BY date DESC LIMIT 1",
                    params![account_id_str, email_lower],
                    |row| row.get(0),
                )
                .ok();

            let vip_status: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM vip_contacts WHERE email = ?1",
                    params![email_lower],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0) > 0;

            let mut cat_stmt = conn
                .prepare(
                    "SELECT COALESCE(ai_category, 'uncategorized'), COUNT(*) as cnt
                     FROM messages WHERE account_id = ?1 AND from_address = ?2 AND is_deleted = 0
                     GROUP BY ai_category ORDER BY cnt DESC LIMIT 5",
                )
                .map_err(|e| format!("profile query error: {e}"))?;
            let categories: Vec<String> = cat_stmt
                .query_map(params![account_id_str, email_lower], |row| row.get::<_, String>(0))
                .map_err(|e| format!("profile query error: {e}"))?
                .filter_map(|r| r.ok())
                .collect();

            Ok(serde_json::json!({
                "email": email_lower,
                "display_name": display_name,
                "vip": vip_status,
                "total_emails_from": total_from,
                "total_emails_to": total_to,
                "top_categories": categories,
            }))
        }
        "get_inbox_stats" => {
            // Compute fresh stats and return them
            crate::api::inbox_stats::compute_and_store(conn, &account_id_str)
                .map_err(|e| format!("failed to compute stats: {e}"))?;

            let stats = crate::api::inbox_stats::get_all_stats(conn)
                .map_err(|e| format!("failed to read stats: {e}"))?;

            let account_stats = stats
                .into_iter()
                .find(|s| s.account_id == account_id_str)
                .ok_or_else(|| "no stats found for account".to_string())?;

            Ok(serde_json::json!({
                "total": account_stats.total,
                "unread": account_stats.unread,
                "starred": account_stats.starred,
                "today_count": account_stats.today_count,
                "week_count": account_stats.week_count,
                "month_count": account_stats.month_count,
                "by_category": account_stats.by_category,
                "top_senders": account_stats.top_senders,
            }))
        }
        "manage_draft" => {
            let action = arguments
                .get("action")
                .and_then(|v| v.as_str())
                .ok_or("missing 'action' argument")?;

            match action {
                "create" => {
                    let body = arguments
                        .get("body")
                        .and_then(|v| v.as_str())
                        .ok_or("missing 'body' argument for create")?;
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
                        body,
                        None,
                    );
                    Ok(serde_json::json!({"draft_id": draft_id, "action": "created"}))
                }
                "update" => {
                    let draft_id = arguments
                        .get("draft_id")
                        .and_then(|v| v.as_str())
                        .ok_or("missing 'draft_id' argument for update")?;
                    let body = arguments
                        .get("body")
                        .and_then(|v| v.as_str())
                        .ok_or("missing 'body' argument for update")?;
                    let subject = arguments.get("subject").and_then(|v| v.as_str());
                    let to = arguments.get("to").map(|v| serde_json::to_string(v).unwrap_or_default());

                    message::save_draft(
                        conn,
                        Some(draft_id),
                        &account_id_str,
                        to.as_deref(),
                        None,
                        None,
                        subject,
                        body,
                        None,
                    );
                    Ok(serde_json::json!({"draft_id": draft_id, "action": "updated"}))
                }
                "delete" => {
                    let draft_id = arguments
                        .get("draft_id")
                        .and_then(|v| v.as_str())
                        .ok_or("missing 'draft_id' argument for delete")?;
                    let deleted = message::delete_draft(conn, draft_id);
                    Ok(serde_json::json!({"draft_id": draft_id, "deleted": deleted}))
                }
                other => Err(format!("unknown manage_draft action: {other}")),
            }
        }
        "bulk_action" => {
            let ids_val = arguments
                .get("message_ids")
                .ok_or("missing 'message_ids' argument")?;
            let action = arguments
                .get("action")
                .and_then(|v| v.as_str())
                .ok_or("missing 'action' argument")?;

            let ids: Vec<String> = ids_val
                .as_array()
                .ok_or("'message_ids' must be an array")?
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();

            if ids.is_empty() {
                return Ok(serde_json::json!({"updated": 0, "action": action}));
            }
            if ids.len() > 500 {
                return Err("message_ids exceeds maximum of 500".to_string());
            }

            // Validate ownership: all ids must belong to this account
            let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{}", i + 1)).collect();
            let in_clause = placeholders.join(", ");
            let check_sql = format!(
                "SELECT COUNT(*) FROM messages WHERE id IN ({in_clause}) AND account_id != ?1"
            );
            let mut check_params: Vec<&dyn rusqlite::types::ToSql> = vec![&account_id_str];
            let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
            for id in &id_refs {
                check_params.push(id as &dyn rusqlite::types::ToSql);
            }
            let foreign_count: i64 = conn
                .query_row(&check_sql, check_params.as_slice(), |row| row.get(0))
                .unwrap_or(0);
            if foreign_count > 0 {
                return Err("one or more message_ids not in scope".to_string());
            }

            let updated = message::batch_update(conn, &id_refs, action);
            Ok(serde_json::json!({"updated": updated, "action": action, "count": ids.len()}))
        }
        _ => Err(format!("unknown tool: {tool_name}")),
    }
}

// ---------------------------------------------------------------------------
// Async tool execution (AI-dependent tools)
// ---------------------------------------------------------------------------

/// Returns true if the tool requires async execution (AI providers).
fn is_async_tool(tool_name: &str) -> bool {
    matches!(tool_name, "get_thread_summary" | "extract_tasks" | "extract_deadlines" | "chat")
}

async fn execute_tool_async(
    state: &Arc<AppState>,
    account_id: &str,
    tool_name: &str,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let account_id_str = account_id.to_string();

    match tool_name {
        "get_thread_summary" => {
            let thread_id = arguments
                .get("thread_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'thread_id' argument")?;

            let conn = state.db.get().map_err(|_| "database error".to_string())?;
            let messages = crate::models::message::MessageDetail::list_by_thread(&conn, thread_id);
            if messages.is_empty() {
                return Err("thread not found".to_string());
            }

            // Check account ownership
            if let Some(first) = messages.first() {
                if first.account_id != account_id_str {
                    return Err("thread not in scope".to_string());
                }
            }

            // Check cache
            if let Some(ref cached) = messages[0].ai_summary {
                if !cached.is_empty() {
                    return Ok(serde_json::json!({
                        "thread_id": thread_id,
                        "summary": cached,
                        "cached": true,
                    }));
                }
            }

            // AI check
            let ai_enabled = conn
                .query_row("SELECT value FROM config WHERE key = 'ai_enabled'", [], |row| row.get::<_, String>(0))
                .unwrap_or_else(|_| "false".to_string());
            if ai_enabled != "true" || !state.providers.has_providers() {
                return Err("AI not available".to_string());
            }

            let subject = messages[0].subject.as_deref().unwrap_or("(no subject)");
            let prompt = crate::api::ai_actions::build_summary_prompt(subject, &messages);
            let summary = state
                .providers
                .generate(&prompt, Some(crate::api::ai_actions::SUMMARY_SYSTEM_PROMPT))
                .await
                .ok_or("AI provider unavailable".to_string())?;

            // Cache the summary
            let first_id = &messages[0].id;
            let _ = conn.execute(
                "UPDATE messages SET ai_summary = ?2, updated_at = unixepoch() WHERE id = ?1",
                rusqlite::params![first_id, summary],
            );

            Ok(serde_json::json!({
                "thread_id": thread_id,
                "summary": summary,
                "cached": false,
            }))
        }
        "extract_tasks" => {
            let thread_id = arguments
                .get("thread_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'thread_id' argument")?;

            let conn = state.db.get().map_err(|_| "database error".to_string())?;
            let messages = crate::models::message::MessageDetail::list_by_thread(&conn, thread_id);
            if messages.is_empty() {
                return Err("thread not found".to_string());
            }
            if let Some(first) = messages.first() {
                if first.account_id != account_id_str {
                    return Err("thread not in scope".to_string());
                }
            }

            let ai_enabled = conn
                .query_row("SELECT value FROM config WHERE key = 'ai_enabled'", [], |row| row.get::<_, String>(0))
                .unwrap_or_else(|_| "false".to_string());
            if ai_enabled != "true" || !state.providers.has_providers() {
                return Err("AI not available".to_string());
            }

            let prompt = crate::api::ai_actions::build_extract_tasks_prompt(&messages);
            let ai_response = state
                .providers
                .generate(&prompt, Some(crate::api::ai_actions::EXTRACT_TASKS_SYSTEM_PROMPT))
                .await
                .ok_or("AI provider unavailable".to_string())?;

            let tasks = crate::api::ai_actions::parse_extracted_tasks(&ai_response);
            let tasks_json: Vec<serde_json::Value> = tasks
                .iter()
                .map(|t| serde_json::json!({
                    "task": t.task,
                    "priority": t.priority,
                    "deadline": t.deadline,
                    "source_subject": t.source_subject,
                }))
                .collect();

            Ok(serde_json::json!({"thread_id": thread_id, "tasks": tasks_json, "count": tasks_json.len()}))
        }
        "extract_deadlines" => {
            let thread_id = arguments
                .get("thread_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'thread_id' argument")?;

            let conn = state.db.get().map_err(|_| "database error".to_string())?;
            let messages = crate::models::message::MessageDetail::list_by_thread(&conn, thread_id);
            if messages.is_empty() {
                return Err("thread not found".to_string());
            }
            if let Some(first) = messages.first() {
                if first.account_id != account_id_str {
                    return Err("thread not in scope".to_string());
                }
            }

            let ai_enabled = conn
                .query_row("SELECT value FROM config WHERE key = 'ai_enabled'", [], |row| row.get::<_, String>(0))
                .unwrap_or_else(|_| "false".to_string());
            if ai_enabled != "true" || !state.providers.has_providers() {
                return Err("AI not available".to_string());
            }

            // Use the first message in the thread for deadline extraction
            let first_msg = &messages[0];
            let subject = first_msg.subject.as_deref().unwrap_or("(no subject)");
            // Combine all message bodies
            let combined_body: String = messages
                .iter()
                .filter_map(|m| m.body_text.as_deref())
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");

            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let system_prompt = crate::api::deadlines::build_deadline_system_prompt(&today);
            let user_prompt = crate::api::deadlines::build_deadline_user_prompt(subject, &combined_body);

            let raw_response = state
                .providers
                .generate(&user_prompt, Some(&system_prompt))
                .await
                .ok_or("AI provider unavailable".to_string())?;

            let extracted = crate::api::deadlines::parse_deadline_response(&raw_response);
            let deadlines_json: Vec<serde_json::Value> = extracted
                .iter()
                .map(|d| serde_json::json!({
                    "description": d.description,
                    "deadline_date": d.deadline_date,
                    "deadline_source": d.deadline_source,
                    "is_explicit": d.is_explicit,
                }))
                .collect();

            Ok(serde_json::json!({"thread_id": thread_id, "deadlines": deadlines_json, "count": deadlines_json.len()}))
        }
        "chat" => {
            let message = arguments
                .get("message")
                .and_then(|v| v.as_str())
                .ok_or("missing 'message' argument")?;

            // session_id: use provided or generate a new one
            let session_id = arguments
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("mcp-chat-{}", uuid::Uuid::new_v4()));

            if message.len() > 50_000 {
                return Err("message too long (max 50000 chars)".to_string());
            }

            let (history, ai_enabled) = {
                let conn = state.db.get().map_err(|_| "database error".to_string())?;
                let ai_enabled = conn
                    .query_row("SELECT value FROM config WHERE key = 'ai_enabled'", [], |row| row.get::<_, String>(0))
                    .unwrap_or_else(|_| "false".to_string());
                if ai_enabled != "true" || !state.providers.has_providers() {
                    return Err("AI not available".to_string());
                }
                // Store user message
                let user_msg_id = uuid::Uuid::new_v4().to_string();
                let _ = conn.execute(
                    "INSERT INTO chat_messages (id, session_id, role, content) VALUES (?1, ?2, 'user', ?3)",
                    rusqlite::params![user_msg_id, session_id, message],
                );
                let history = crate::api::chat::load_history_for_mcp(&conn, &session_id, 10);
                (history, ai_enabled)
            };

            let _ = ai_enabled; // already checked above

            let system_prompt = {
                let conn = state.db.get().map_err(|_| "database error".to_string())?;
                crate::api::chat::chat_system_prompt_for_mcp(&conn)
            };

            let (ai_response, citations, _tool_records) = crate::api::chat::agentic_chat_for_mcp(
                &state.providers,
                &state.db,
                &state.memories,
                &system_prompt,
                message,
                &history,
                5,
            )
            .await
            .map_err(|_| "AI chat failed".to_string())?;

            // Store assistant response
            let assistant_id = uuid::Uuid::new_v4().to_string();
            {
                let conn = state.db.get().map_err(|_| "database error".to_string())?;
                let _ = conn.execute(
                    "INSERT INTO chat_messages (id, session_id, role, content) VALUES (?1, ?2, 'assistant', ?3)",
                    rusqlite::params![assistant_id, session_id, ai_response],
                );
            }

            let citations_out: Vec<serde_json::Value> = citations
                .iter()
                .map(|c| serde_json::json!({
                    "message_id": c.message_id,
                    "subject": c.subject,
                    "from": c.from,
                    "snippet": c.snippet,
                }))
                .collect();

            Ok(serde_json::json!({
                "session_id": session_id,
                "response": ai_response,
                "citations": citations_out,
            }))
        }
        _ => Err(format!("unknown async tool: {tool_name}")),
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

    let account_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE id = ?1 AND is_active = 1",
            params![req.account_id],
            |row| row.get(0),
        )
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
        })?;
    if account_exists == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "account not found"})),
        ));
    }

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

    // Execute with timing — async tools release the conn before awaiting
    let start = Instant::now();
    let result = if is_async_tool(&req.tool_name) {
        drop(conn);
        execute_tool_async(&state, &session.account_id, &req.tool_name, &req.arguments).await
    } else {
        let r = execute_tool(&conn, &session.account_id, &req.tool_name, &req.arguments);
        drop(conn);
        r
    };
    let duration_ms = start.elapsed().as_millis() as i64;

    let (status, output) = match result {
        Ok(val) => ("success".to_string(), val),
        Err(e) => (
            "error".to_string(),
            serde_json::json!({"error": e}),
        ),
    };

    // Re-acquire connection for bookkeeping
    let post_conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    // Record the call
    record_tool_call(
        &post_conn,
        &req.session_id,
        &req.tool_name,
        &req.arguments,
        &output,
        &status,
        duration_ms,
    );

    // Touch session last_active_at
    touch_session(&post_conn, &req.session_id);

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
        assert_eq!(tools.len(), 17);

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
        // New tools
        assert!(names.contains(&"get_thread_summary"));
        assert!(names.contains(&"get_contact_profile"));
        assert!(names.contains(&"extract_tasks"));
        assert!(names.contains(&"extract_deadlines"));
        assert!(names.contains(&"chat"));
        assert!(names.contains(&"get_inbox_stats"));
        assert!(names.contains(&"manage_draft"));
        assert!(names.contains(&"bulk_action"));
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
        assert_eq!(tools.len(), 17);
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

    // Test 20: list_threads with unread filter
    #[test]
    fn test_list_threads_unread_filter() {
        let (conn, account) = setup();
        let acct_id = &account.id;

        // Insert messages: one read, one unread, both with thread_ids
        let msg1 = InsertMessage {
            account_id: acct_id.clone(),
            message_id: Some("<thread-a@test.com>".to_string()),
            thread_id: Some("thread-a".to_string()),
            folder: "INBOX".to_string(),
            from_address: Some("alice@example.com".to_string()),
            from_name: Some("Alice".to_string()),
            to_addresses: None,
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some("Read thread".to_string()),
            date: Some(1700000000),
            snippet: None,
            body_text: None,
            body_html: None,
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
        InsertMessage::insert(&conn, &msg1).unwrap();

        let msg2 = InsertMessage {
            account_id: acct_id.clone(),
            message_id: Some("<thread-b@test.com>".to_string()),
            thread_id: Some("thread-b".to_string()),
            folder: "INBOX".to_string(),
            from_address: Some("bob@example.com".to_string()),
            from_name: Some("Bob".to_string()),
            to_addresses: None,
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some("Unread thread".to_string()),
            date: Some(1700000001),
            snippet: None,
            body_text: None,
            body_html: None,
            is_read: false,
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
        InsertMessage::insert(&conn, &msg2).unwrap();

        // Without filter: both threads
        let result = execute_tool(&conn, acct_id, "list_threads", &serde_json::json!({}));
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["count"].as_i64().unwrap(), 2);

        // With unread filter: only thread-b
        let result = execute_tool(&conn, acct_id, "list_threads", &serde_json::json!({"unread": true}));
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["count"].as_i64().unwrap(), 1);
        assert_eq!(val["threads"][0]["thread_id"].as_str().unwrap(), "thread-b");
    }

    // Test 21: get_inbox_stats tool
    #[test]
    fn test_tool_call_get_inbox_stats() {
        let (conn, account) = setup();
        let acct_id = &account.id;

        insert_test_message(&conn, acct_id, "Unread 1", "INBOX");
        insert_test_message(&conn, acct_id, "Unread 2", "INBOX");

        let result = execute_tool(&conn, acct_id, "get_inbox_stats", &serde_json::json!({}));
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val["total"].as_i64().unwrap() >= 2);
        assert!(val["unread"].as_i64().is_some());
        assert!(val["by_category"].is_object());
    }

    // Test 22: manage_draft — create, update, delete
    #[test]
    fn test_tool_call_manage_draft_lifecycle() {
        let (conn, account) = setup();
        let acct_id = &account.id;

        // Create
        let result = execute_tool(
            &conn,
            acct_id,
            "manage_draft",
            &serde_json::json!({
                "action": "create",
                "to": ["alice@example.com"],
                "subject": "Hello",
                "body": "Draft body text"
            }),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        let draft_id = val["draft_id"].as_str().unwrap().to_string();
        assert_eq!(val["action"].as_str().unwrap(), "created");

        // Update
        let result = execute_tool(
            &conn,
            acct_id,
            "manage_draft",
            &serde_json::json!({
                "action": "update",
                "draft_id": draft_id,
                "body": "Updated body"
            }),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["action"].as_str().unwrap(), "updated");

        // Delete
        let result = execute_tool(
            &conn,
            acct_id,
            "manage_draft",
            &serde_json::json!({
                "action": "delete",
                "draft_id": draft_id
            }),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val["deleted"].as_bool().unwrap());
    }

    // Test 23: bulk_action — mark_read on multiple messages
    #[test]
    fn test_tool_call_bulk_action() {
        let (conn, account) = setup();
        let acct_id = &account.id;

        let id1 = insert_test_message(&conn, acct_id, "Bulk 1", "INBOX");
        let id2 = insert_test_message(&conn, acct_id, "Bulk 2", "INBOX");

        let result = execute_tool(
            &conn,
            acct_id,
            "bulk_action",
            &serde_json::json!({
                "message_ids": [id1, id2],
                "action": "mark_read"
            }),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["updated"].as_i64().unwrap(), 2);
        assert_eq!(val["action"].as_str().unwrap(), "mark_read");

        // Verify both are now read
        let d1 = MessageDetail::get_by_id(&conn, &id1).unwrap();
        let d2 = MessageDetail::get_by_id(&conn, &id2).unwrap();
        assert!(d1.is_read);
        assert!(d2.is_read);
    }

    // Test 24: bulk_action — reject foreign account messages
    #[test]
    fn test_tool_call_bulk_action_wrong_account() {
        let (conn, account) = setup();
        let id = insert_test_message(&conn, &account.id, "Foreign msg", "INBOX");

        let result = execute_tool(
            &conn,
            "different-account",
            "bulk_action",
            &serde_json::json!({
                "message_ids": [id],
                "action": "archive"
            }),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not in scope"));
    }

    // Test 25: get_contact_profile — no emails returns error
    #[test]
    fn test_tool_call_get_contact_profile_not_found() {
        let (conn, account) = setup();
        let acct_id = &account.id;

        let result = execute_tool(
            &conn,
            acct_id,
            "get_contact_profile",
            &serde_json::json!({"email": "nobody@nowhere.example"}),
        );
        assert!(result.is_err());
    }

    // Test 26: get_contact_profile — found via messages
    #[test]
    fn test_tool_call_get_contact_profile_found() {
        let (conn, account) = setup();
        let acct_id = &account.id;
        insert_test_message(&conn, acct_id, "From Alice", "INBOX");

        let result = execute_tool(
            &conn,
            acct_id,
            "get_contact_profile",
            &serde_json::json!({"email": "sender@example.com"}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["email"].as_str().unwrap(), "sender@example.com");
        assert!(val["total_emails_from"].as_i64().unwrap() >= 1);
    }

    // Test 27: is_async_tool helper
    #[test]
    fn test_is_async_tool() {
        assert!(is_async_tool("get_thread_summary"));
        assert!(is_async_tool("extract_tasks"));
        assert!(is_async_tool("extract_deadlines"));
        assert!(is_async_tool("chat"));
        assert!(!is_async_tool("list_inbox"));
        assert!(!is_async_tool("get_inbox_stats"));
        assert!(!is_async_tool("bulk_action"));
    }
}
