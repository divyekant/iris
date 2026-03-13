use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

// ── Core types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum LlmMessage {
    User(String),
    AssistantText(String),
    AssistantToolCalls {
        text: Option<String>,
        tool_calls: Vec<ToolCall>,
    },
    ToolResult {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Debug, Clone)]
pub enum LlmResponse {
    Text(String),
    ToolCalls {
        text: Option<String>,
        calls: Vec<ToolCall>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub name: String,
    pub arguments: serde_json::Value,
    pub result_preview: String,
}

// ── Tool definitions ────────────────────────────────────────────────

pub fn all_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "inbox_stats".to_string(),
            description: "Get aggregate inbox statistics including total, unread, starred counts, \
                          category breakdown, and top senders across all accounts."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "search_emails".to_string(),
            description: "Search emails by keyword using full-text search, with optional filters. \
                          Returns matching messages with subject, sender, date, and a text snippet."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query — keywords to find in email subject, body, or sender"
                    },
                    "date_from": {
                        "type": "string",
                        "description": "Start date filter (ISO format, e.g. 2026-03-01)"
                    },
                    "date_to": {
                        "type": "string",
                        "description": "End date filter (ISO format, e.g. 2026-03-08)"
                    },
                    "sender": {
                        "type": "string",
                        "description": "Filter by sender (email or name substring)"
                    },
                    "is_read": {
                        "type": "boolean",
                        "description": "Filter by read status"
                    },
                    "category": {
                        "type": "string",
                        "description": "Filter by AI category"
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "read_email".to_string(),
            description: "Read the full content of a specific email by its message ID. \
                          Supports both full UUIDs and 8-character truncated IDs."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message_id": {
                        "type": "string",
                        "description": "The message ID (full UUID or 8-char prefix)"
                    }
                },
                "required": ["message_id"]
            }),
        },
        Tool {
            name: "list_emails".to_string(),
            description: "List emails with optional filters. Returns summary info (no body). \
                          Use for browsing, counting filtered results, or finding emails by metadata \
                          like date range, sender, read status, or category.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "date_from": {
                        "type": "string",
                        "description": "Start date (ISO format, e.g. 2026-03-01)"
                    },
                    "date_to": {
                        "type": "string",
                        "description": "End date (ISO format, e.g. 2026-03-08)"
                    },
                    "sender": {
                        "type": "string",
                        "description": "Filter by sender email address or name (substring match)"
                    },
                    "is_read": {
                        "type": "boolean",
                        "description": "Filter by read status (true=read, false=unread)"
                    },
                    "category": {
                        "type": "string",
                        "description": "Filter by AI category: primary, social, promotions, updates"
                    },
                    "folder": {
                        "type": "string",
                        "description": "Filter by IMAP folder name"
                    },
                    "sort": {
                        "type": "string",
                        "enum": ["newest", "oldest"],
                        "description": "Sort order (default: newest)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results to return (default: 20, max: 50)"
                    }
                },
                "required": []
            }),
        },
        Tool {
            name: "bulk_update_emails".to_string(),
            description: "Execute a batch operation on multiple emails matching criteria. \
                          Use this after finding emails with list_emails or search_emails. \
                          Actions: archive, mark_read, mark_unread, trash, star, unstar, \
                          move_to_category. IMPORTANT: Always show the user how many emails \
                          will be affected and what action will be taken BEFORE executing. \
                          The user must confirm the action proposal before you call this tool."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["archive", "mark_read", "mark_unread", "trash", "star", "unstar", "move_to_category"],
                        "description": "The batch action to perform"
                    },
                    "message_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of message IDs to act on (from list_emails/search_emails results). Max 500."
                    },
                    "category": {
                        "type": "string",
                        "description": "Target category (only required for move_to_category action)"
                    }
                },
                "required": ["action", "message_ids"]
            }),
        },
        Tool {
            name: "compose_email".to_string(),
            description: "Draft a new email or reply based on the user's instructions. Use this when the user asks to write, draft, compose, or reply to an email via chat.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "to": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Recipient email addresses"
                    },
                    "cc": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "CC recipients (optional)"
                    },
                    "subject": {
                        "type": "string",
                        "description": "Email subject line"
                    },
                    "body": {
                        "type": "string",
                        "description": "Email body in HTML format"
                    },
                    "reply_to_message_id": {
                        "type": "string",
                        "description": "If replying, the message_id to reply to (optional)"
                    },
                    "tone": {
                        "type": "string",
                        "enum": ["formal", "casual", "brief"],
                        "description": "Writing tone (optional, defaults to formal)"
                    }
                },
                "required": ["to", "subject", "body"]
            }),
        },
    ]
}

// ── Tool execution ──────────────────────────────────────────────────

pub fn execute_tool(
    conn: &Connection,
    _memories: Option<&crate::ai::memories::MemoriesClient>,
    name: &str,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match name {
        "inbox_stats" => handle_inbox_stats(conn),
        "search_emails" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter: query".to_string())?;
            handle_search_emails(conn, query, arguments)
        }
        "read_email" => {
            let message_id = arguments
                .get("message_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter: message_id".to_string())?;
            handle_read_email(conn, message_id)
        }
        "list_emails" => handle_list_emails(conn, arguments),
        "bulk_update_emails" => handle_bulk_update_emails(conn, arguments),
        "compose_email" => {
            let tool_call = ToolCall {
                id: String::new(),
                name: name.to_string(),
                arguments: arguments.clone(),
            };
            let result = handle_compose_email(conn, &tool_call);
            if result.success {
                Ok(serde_json::json!({
                    "success": true,
                    "message": result.message,
                    "data": result.data,
                }))
            } else {
                Err(result.message)
            }
        }
        _ => Err(format!("Unknown tool: {}", name)),
    }
}

fn handle_inbox_stats(conn: &Connection) -> Result<serde_json::Value, String> {
    let stats =
        crate::api::inbox_stats::get_all_stats(conn).map_err(|e| format!("DB error: {}", e))?;
    serde_json::to_value(&stats).map_err(|e| format!("Serialization error: {}", e))
}

fn handle_search_emails(
    conn: &Connection,
    query: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    // Sanitize: keep only alphanumeric + whitespace
    let sanitized: String = query
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();

    // Build FTS5 OR query from terms >2 chars, max 5 terms
    let terms: Vec<&str> = sanitized
        .split_whitespace()
        .filter(|t| t.len() > 2)
        .take(5)
        .collect();

    if terms.is_empty() {
        return Ok(serde_json::json!([]));
    }

    let fts_query = terms.join(" OR ");

    // Build additional filter conditions
    let mut extra_conditions = Vec::new();
    let mut extra_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    // FTS5 MATCH is param ?1, so extra params start at ?2
    let mut pidx = 2;

    if let Some(date_from) = args.get("date_from").and_then(|v| v.as_str()) {
        if let Ok(dt) = chrono::NaiveDate::parse_from_str(date_from, "%Y-%m-%d") {
            let ts = dt.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
            extra_conditions.push(format!("m.date >= ?{}", pidx));
            extra_params.push(Box::new(ts));
            pidx += 1;
        }
    }

    if let Some(date_to) = args.get("date_to").and_then(|v| v.as_str()) {
        if let Ok(dt) = chrono::NaiveDate::parse_from_str(date_to, "%Y-%m-%d") {
            let ts = dt.and_hms_opt(23, 59, 59).unwrap().and_utc().timestamp();
            extra_conditions.push(format!("m.date <= ?{}", pidx));
            extra_params.push(Box::new(ts));
            pidx += 1;
        }
    }

    if let Some(sender) = args.get("sender").and_then(|v| v.as_str()) {
        if !sender.is_empty() {
            let escaped = sender.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            let pattern = format!("%{}%", escaped);
            extra_conditions.push(format!(
                "(m.from_address LIKE ?{p} ESCAPE '\\' OR m.from_name LIKE ?{p} ESCAPE '\\')",
                p = pidx
            ));
            extra_params.push(Box::new(pattern));
            pidx += 1;
        }
    }

    if let Some(is_read) = args.get("is_read").and_then(|v| v.as_bool()) {
        extra_conditions.push(format!("m.is_read = ?{}", pidx));
        extra_params.push(Box::new(if is_read { 1i32 } else { 0i32 }));
        pidx += 1;
    }

    if let Some(category) = args.get("category").and_then(|v| v.as_str()) {
        if !category.is_empty() {
            extra_conditions.push(format!("LOWER(m.ai_category) = LOWER(?{})", pidx));
            extra_params.push(Box::new(category.to_string()));
            pidx += 1;
        }
    }

    let _ = pidx; // suppress unused variable warning

    let filter_clause = if extra_conditions.is_empty() {
        String::new()
    } else {
        format!(" AND {}", extra_conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT m.id, m.subject, m.from_name, m.from_address, m.date, m.is_read, \
         snippet(fts_messages, -1, '', '', '...', 40) \
         FROM fts_messages fts JOIN messages m ON m.rowid = fts.rowid \
         WHERE fts_messages MATCH ?1 AND m.is_draft = 0{} ORDER BY rank LIMIT 10",
        filter_clause
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Query prepare error: {}", e))?;

    // Build combined params: FTS query first, then extra filter params
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    all_params.push(Box::new(fts_query));
    all_params.extend(extra_params);
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        all_params.iter().map(|p| p.as_ref()).collect();

    let rows: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params_from_iter(param_refs), |row| {
            let id: String = row.get(0)?;
            let subject: Option<String> = row.get(1)?;
            let from_name: Option<String> = row.get(2)?;
            let from_address: Option<String> = row.get(3)?;
            let date: Option<i64> = row.get(4)?;
            let is_read: i32 = row.get(5)?;
            let snippet: Option<String> = row.get(6)?;

            let date_str = date
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .map(|dt| {
                    dt.with_timezone(&chrono::Local)
                        .format("%b %-d, %Y %H:%M")
                        .to_string()
                })
                .unwrap_or_default();

            let from = match (&from_name, &from_address) {
                (Some(name), Some(addr)) if !name.is_empty() => format!("{} <{}>", name, addr),
                (_, Some(addr)) => addr.clone(),
                _ => String::new(),
            };

            Ok(serde_json::json!({
                "id": id,
                "subject": subject.unwrap_or_default(),
                "from": from,
                "date": date_str,
                "is_read": is_read != 0,
                "snippet": snippet.unwrap_or_default(),
            }))
        })
        .map_err(|e| format!("Query error: {}", e))?
        .filter_map(|r| r.map_err(|e| tracing::warn!("Search row skip: {e}")).ok())
        .collect();

    Ok(serde_json::Value::Array(rows))
}

fn handle_read_email(conn: &Connection, message_id: &str) -> Result<serde_json::Value, String> {
    // Resolve potentially truncated ID (8-char prefix)
    // Strip LIKE wildcards to prevent matching arbitrary messages
    let sanitized_id: String = message_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let full_id = if sanitized_id.len() < 36 {
        conn.query_row(
            "SELECT id FROM messages WHERE id LIKE ?1 LIMIT 1",
            params![format!("{}%", sanitized_id)],
            |row| row.get::<_, String>(0),
        )
        .map_err(|_| format!("No message found matching ID: {}", message_id))?
    } else {
        message_id.to_string()
    };

    let result = conn
        .query_row(
            "SELECT id, subject, from_name, from_address, to_addresses, date, body_text, \
             is_read, is_starred, ai_category, ai_summary, has_attachments, attachment_names \
             FROM messages WHERE id = ?1",
            params![full_id],
            |row| {
                let id: String = row.get(0)?;
                let subject: Option<String> = row.get(1)?;
                let from_name: Option<String> = row.get(2)?;
                let from_address: Option<String> = row.get(3)?;
                let to_addresses: Option<String> = row.get(4)?;
                let date: Option<i64> = row.get(5)?;
                let body_text: Option<String> = row.get(6)?;
                let is_read: i32 = row.get(7)?;
                let is_starred: i32 = row.get(8)?;
                let category: Option<String> = row.get(9)?;
                let summary: Option<String> = row.get(10)?;
                let has_attachments: i32 = row.get(11)?;
                let attachment_names: Option<String> = row.get(12)?;

                let date_str = date
                    .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                    .map(|dt| {
                        dt.with_timezone(&chrono::Local)
                            .format("%b %-d, %Y %H:%M")
                            .to_string()
                    })
                    .unwrap_or_default();

                let from = match (&from_name, &from_address) {
                    (Some(name), Some(addr)) if !name.is_empty() => {
                        format!("{} <{}>", name, addr)
                    }
                    (_, Some(addr)) => addr.clone(),
                    _ => String::new(),
                };

                let body = body_text.unwrap_or_default();
                let body_truncated = if body.len() > 4000 {
                    // Safe char-boundary truncation to avoid panic on multi-byte UTF-8
                    let end = body.char_indices().nth(4000).map(|(i, _)| i).unwrap_or(body.len());
                    format!("{}...[truncated]", &body[..end])
                } else {
                    body
                };

                Ok(serde_json::json!({
                    "id": id,
                    "subject": subject.unwrap_or_default(),
                    "from": from,
                    "to": to_addresses.unwrap_or_default(),
                    "date": date_str,
                    "body": body_truncated,
                    "is_read": is_read != 0,
                    "is_starred": is_starred != 0,
                    "category": category.unwrap_or_default(),
                    "summary": summary.unwrap_or_default(),
                    "has_attachments": has_attachments != 0,
                    "attachment_names": attachment_names.unwrap_or_default(),
                }))
            },
        )
        .map_err(|_| format!("No message found with ID: {}", full_id))?;

    Ok(result)
}

fn handle_list_emails(
    conn: &Connection,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut conditions = vec!["is_draft = 0".to_string()];
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    // date_from: parse ISO date to epoch timestamp
    if let Some(date_from) = args.get("date_from").and_then(|v| v.as_str()) {
        if let Ok(dt) = chrono::NaiveDate::parse_from_str(date_from, "%Y-%m-%d") {
            let ts = dt
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp();
            conditions.push(format!("date >= ?{}", param_idx));
            param_values.push(Box::new(ts));
            param_idx += 1;
        }
    }

    // date_to: parse ISO date to epoch timestamp (end of day)
    if let Some(date_to) = args.get("date_to").and_then(|v| v.as_str()) {
        if let Ok(dt) = chrono::NaiveDate::parse_from_str(date_to, "%Y-%m-%d") {
            let ts = dt
                .and_hms_opt(23, 59, 59)
                .unwrap()
                .and_utc()
                .timestamp();
            conditions.push(format!("date <= ?{}", param_idx));
            param_values.push(Box::new(ts));
            param_idx += 1;
        }
    }

    // sender: LIKE match on from_address or from_name
    if let Some(sender) = args.get("sender").and_then(|v| v.as_str()) {
        if !sender.is_empty() {
            let escaped = sender.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            let pattern = format!("%{}%", escaped);
            conditions.push(format!(
                "(from_address LIKE ?{p} ESCAPE '\\' OR from_name LIKE ?{p} ESCAPE '\\')",
                p = param_idx
            ));
            param_values.push(Box::new(pattern));
            param_idx += 1;
        }
    }

    // is_read: boolean filter
    if let Some(is_read) = args.get("is_read").and_then(|v| v.as_bool()) {
        conditions.push(format!("is_read = ?{}", param_idx));
        param_values.push(Box::new(if is_read { 1i32 } else { 0i32 }));
        param_idx += 1;
    }

    // category: case-insensitive match on ai_category
    if let Some(category) = args.get("category").and_then(|v| v.as_str()) {
        if !category.is_empty() {
            conditions.push(format!("LOWER(ai_category) = LOWER(?{})", param_idx));
            param_values.push(Box::new(category.to_string()));
            param_idx += 1;
        }
    }

    // folder: exact match
    if let Some(folder) = args.get("folder").and_then(|v| v.as_str()) {
        if !folder.is_empty() {
            conditions.push(format!("folder = ?{}", param_idx));
            param_values.push(Box::new(folder.to_string()));
            param_idx += 1;
        }
    }

    let _ = param_idx; // suppress unused variable warning

    // sort
    let sort_order = match args.get("sort").and_then(|v| v.as_str()) {
        Some("oldest") => "ASC",
        _ => "DESC",
    };

    // limit (capped at 50)
    let limit = args
        .get("limit")
        .and_then(|v| v.as_i64())
        .map(|l| l.min(50).max(1))
        .unwrap_or(20);

    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT id, subject, from_name, from_address, date, is_read, is_starred, ai_category, snippet \
         FROM messages WHERE {} ORDER BY date {} LIMIT {}",
        where_clause, sort_order, limit
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Query prepare error: {}", e))?;

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let rows: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params_from_iter(param_refs), |row| {
            let id: String = row.get(0)?;
            let subject: Option<String> = row.get(1)?;
            let from_name: Option<String> = row.get(2)?;
            let from_address: Option<String> = row.get(3)?;
            let date: Option<i64> = row.get(4)?;
            let is_read: i32 = row.get(5)?;
            let is_starred: i32 = row.get(6)?;
            let category: Option<String> = row.get(7)?;
            let snippet: Option<String> = row.get(8)?;

            let date_str = date
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .map(|dt| {
                    dt.with_timezone(&chrono::Local)
                        .format("%b %-d, %Y %H:%M")
                        .to_string()
                })
                .unwrap_or_default();

            let from = match (&from_name, &from_address) {
                (Some(name), Some(addr)) if !name.is_empty() => format!("{} <{}>", name, addr),
                (_, Some(addr)) => addr.clone(),
                _ => String::new(),
            };

            Ok(serde_json::json!({
                "id": id,
                "subject": subject.unwrap_or_default(),
                "from": from,
                "date": date_str,
                "is_read": is_read != 0,
                "is_starred": is_starred != 0,
                "category": category.unwrap_or_default(),
                "snippet": snippet.unwrap_or_default(),
            }))
        })
        .map_err(|e| format!("Query error: {}", e))?
        .filter_map(|r| r.map_err(|e| tracing::warn!("List row skip: {e}")).ok())
        .collect();

    Ok(serde_json::json!({
        "count": rows.len(),
        "emails": rows,
    }))
}

/// Maximum number of messages per bulk operation for safety.
const BULK_UPDATE_MAX: usize = 500;

fn handle_bulk_update_emails(
    conn: &Connection,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: action".to_string())?;

    let message_ids: Vec<String> = args
        .get("message_ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if message_ids.is_empty() {
        return Ok(serde_json::json!({
            "error": "No message IDs provided. Use list_emails or search_emails first to find messages.",
            "updated": 0
        }));
    }

    if message_ids.len() > BULK_UPDATE_MAX {
        return Ok(serde_json::json!({
            "error": format!("Too many messages ({}). Maximum is {} per bulk operation.", message_ids.len(), BULK_UPDATE_MAX),
            "updated": 0
        }));
    }

    // Validate action
    let valid_actions = [
        "archive",
        "mark_read",
        "mark_unread",
        "trash",
        "star",
        "unstar",
        "move_to_category",
    ];
    if !valid_actions.contains(&action) {
        return Ok(serde_json::json!({
            "error": format!("Unknown action: {}. Valid actions: {}", action, valid_actions.join(", ")),
            "updated": 0
        }));
    }

    // Resolve truncated IDs (8-char prefixes) to full UUIDs
    let resolved_ids: Vec<String> = message_ids
        .iter()
        .filter_map(|id| {
            let sanitized: String = id
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            if sanitized.len() >= 36 {
                Some(sanitized)
            } else {
                conn.query_row(
                    "SELECT id FROM messages WHERE id LIKE ?1 LIMIT 1",
                    params![format!("{}%", sanitized)],
                    |row| row.get::<_, String>(0),
                )
                .ok()
            }
        })
        .collect();

    if resolved_ids.is_empty() {
        return Ok(serde_json::json!({
            "error": "None of the provided message IDs could be resolved.",
            "updated": 0
        }));
    }

    // Build and execute batch update
    let placeholders: Vec<String> = (0..resolved_ids.len())
        .map(|i| format!("?{}", i + 1))
        .collect();
    let in_clause = placeholders.join(",");

    let sql = match action {
        "archive" => format!(
            "UPDATE messages SET folder = 'Archive', updated_at = unixepoch() WHERE id IN ({})",
            in_clause
        ),
        "mark_read" => format!(
            "UPDATE messages SET is_read = 1, updated_at = unixepoch() WHERE id IN ({})",
            in_clause
        ),
        "mark_unread" => format!(
            "UPDATE messages SET is_read = 0, updated_at = unixepoch() WHERE id IN ({})",
            in_clause
        ),
        "trash" => format!(
            "UPDATE messages SET folder = 'Trash', updated_at = unixepoch() WHERE id IN ({})",
            in_clause
        ),
        "star" => format!(
            "UPDATE messages SET is_starred = 1, updated_at = unixepoch() WHERE id IN ({})",
            in_clause
        ),
        "unstar" => format!(
            "UPDATE messages SET is_starred = 0, updated_at = unixepoch() WHERE id IN ({})",
            in_clause
        ),
        "move_to_category" => {
            let _cat = args
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("primary");
            // For move_to_category, the category param goes first, then the IDs
            let id_placeholders: Vec<String> = (0..resolved_ids.len())
                .map(|i| format!("?{}", i + 2))
                .collect();
            format!(
                "UPDATE messages SET ai_category = ?1, updated_at = unixepoch() WHERE id IN ({})",
                id_placeholders.join(",")
            )
        }
        _ => unreachable!(), // validated above
    };

    let updated = if action == "move_to_category" {
        let cat = args
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("primary")
            .to_string();
        let mut sql_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        sql_params.push(Box::new(cat));
        for id in &resolved_ids {
            sql_params.push(Box::new(id.clone()));
        }
        conn.execute(
            &sql,
            rusqlite::params_from_iter(sql_params.iter().map(|p| p.as_ref())),
        )
        .map_err(|e| format!("DB error: {}", e))?
    } else {
        let sql_params: Vec<Box<dyn rusqlite::types::ToSql>> = resolved_ids
            .iter()
            .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        conn.execute(
            &sql,
            rusqlite::params_from_iter(sql_params.iter().map(|p| p.as_ref())),
        )
        .map_err(|e| format!("DB error: {}", e))?
    };

    let status = if updated == resolved_ids.len() {
        "all successful"
    } else {
        "some messages may not exist"
    };

    Ok(serde_json::json!({
        "action": action,
        "requested": message_ids.len(),
        "resolved": resolved_ids.len(),
        "updated": updated,
        "status": status
    }))
}

// ── Compose email types ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeEmailArgs {
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub reply_to_message_id: Option<String>,
    pub tone: Option<String>,
}

/// Data stored in ProposedAction for compose_email confirmations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeEmailData {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub reply_to_message_id: Option<String>,
    pub tone: Option<String>,
    /// Thread context fetched at compose time (if replying).
    pub thread_subject: Option<String>,
}

// ── Compose email handler ───────────────────────────────────────────

/// Handle a compose_email tool call.
///
/// Validates the arguments and returns a summary message. The actual draft is
/// NOT created here -- it is deferred to the confirm_action flow so the user
/// can review first.
pub fn handle_compose_email(conn: &Connection, tool_call: &ToolCall) -> ComposeToolResult {
    // Parse arguments
    let args: ComposeEmailArgs = match serde_json::from_value(tool_call.arguments.clone()) {
        Ok(a) => a,
        Err(e) => {
            return ComposeToolResult {
                success: false,
                message: format!("Invalid compose_email arguments: {e}"),
                data: None,
            };
        }
    };

    // Validate: at least one recipient
    if args.to.is_empty() {
        return ComposeToolResult {
            success: false,
            message: "At least one recipient is required in the 'to' field.".to_string(),
            data: None,
        };
    }

    // Validate: non-empty subject
    if args.subject.trim().is_empty() {
        return ComposeToolResult {
            success: false,
            message: "Subject cannot be empty.".to_string(),
            data: None,
        };
    }

    // Validate: non-empty body
    if args.body.trim().is_empty() {
        return ComposeToolResult {
            success: false,
            message: "Email body cannot be empty.".to_string(),
            data: None,
        };
    }

    // Validate email addresses (basic check)
    for addr in &args.to {
        if !addr.contains('@') || addr.len() < 3 {
            return ComposeToolResult {
                success: false,
                message: format!("Invalid email address: {addr}"),
                data: None,
            };
        }
    }
    for addr in &args.cc {
        if !addr.contains('@') || addr.len() < 3 {
            return ComposeToolResult {
                success: false,
                message: format!("Invalid CC email address: {addr}"),
                data: None,
            };
        }
    }

    // If replying, look up the original message for thread context
    let thread_subject = if let Some(ref reply_id) = args.reply_to_message_id {
        conn.query_row(
            "SELECT subject FROM messages WHERE id = ?1",
            rusqlite::params![reply_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
    } else {
        None
    };

    // Build the compose data that will be stored in the proposed_action
    let compose_data = ComposeEmailData {
        to: args.to.clone(),
        cc: args.cc.clone(),
        subject: args.subject.clone(),
        body: args.body.clone(),
        reply_to_message_id: args.reply_to_message_id.clone(),
        tone: args.tone.clone(),
        thread_subject,
    };

    // Build a human-readable summary
    let recipients = args.to.join(", ");
    let tone_label = args.tone.as_deref().unwrap_or("formal");

    let mut summary = format!(
        "Drafted email to {recipients} with subject '{}' (tone: {tone_label}).",
        args.subject
    );
    if args.reply_to_message_id.is_some() {
        summary.push_str(" This is a reply to an existing thread.");
    }
    summary.push_str(" Review and send from the compose panel.");

    ComposeToolResult {
        success: true,
        message: summary,
        data: Some(serde_json::to_value(&compose_data).unwrap_or_default()),
    }
}

/// Result returned by the compose email handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeToolResult {
    pub success: bool,
    pub message: String,
    /// Optional structured data for frontend rendering (e.g. compose card).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Execute a confirmed compose_email action: save the draft to the database.
///
/// Returns the draft ID on success. Accepts any type that derefs to
/// `rusqlite::Connection` (both `PooledConnection` and plain `Connection`).
pub fn execute_compose_email(
    conn: &Connection,
    compose_data: &ComposeEmailData,
    account_id: &str,
) -> Result<String, String> {
    let to_json = serde_json::to_string(&compose_data.to).ok();
    let cc_json = if compose_data.cc.is_empty() {
        None
    } else {
        serde_json::to_string(&compose_data.cc).ok()
    };

    // Strip HTML tags for body_text (plain-text version)
    let body_text = strip_html_tags(&compose_data.body);
    let snippet: String = body_text.chars().take(200).collect();

    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO messages (
            id, account_id, folder, to_addresses, cc_addresses, bcc_addresses,
            subject, body_text, body_html, snippet,
            is_read, is_starred, is_draft, date, from_address, from_name, has_attachments
        ) VALUES (
            ?1, ?2, 'Drafts', ?3, ?4, NULL,
            ?5, ?6, ?7, ?8,
            1, 0, 1, unixepoch(), NULL, NULL, 0
        )",
        rusqlite::params![
            id,
            account_id,
            to_json,
            cc_json,
            compose_data.subject,
            body_text,
            compose_data.body,
            snippet,
        ],
    )
    .map_err(|e| format!("Failed to save draft: {e}"))?;

    Ok(id)
}

/// Simple HTML tag stripper for generating plain text from HTML body.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    fn make_test_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL DEFAULT '',
                folder TEXT NOT NULL DEFAULT 'INBOX',
                from_address TEXT,
                from_name TEXT,
                to_addresses TEXT,
                cc_addresses TEXT,
                bcc_addresses TEXT,
                subject TEXT,
                snippet TEXT,
                body_text TEXT,
                body_html TEXT,
                date INTEGER,
                is_read INTEGER NOT NULL DEFAULT 0,
                is_starred INTEGER NOT NULL DEFAULT 0,
                is_draft INTEGER NOT NULL DEFAULT 0,
                has_attachments INTEGER NOT NULL DEFAULT 0,
                labels TEXT,
                ai_priority_label TEXT,
                ai_category TEXT,
                thread_id TEXT,
                message_id TEXT,
                uid INTEGER,
                modseq INTEGER,
                raw_headers TEXT,
                attachment_names TEXT,
                size_bytes INTEGER,
                updated_at INTEGER DEFAULT (unixepoch()),
                created_at INTEGER DEFAULT (unixepoch())
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_tool_serialization() {
        let tool = Tool {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_value(&tool).unwrap();
        assert_eq!(json["name"], "test_tool");
        assert_eq!(json["description"], "A test tool");
        assert!(json["input_schema"].is_object());
    }

    #[test]
    fn test_tool_call_roundtrip() {
        let call = ToolCall {
            id: "call_123".to_string(),
            name: "search_emails".to_string(),
            arguments: serde_json::json!({"query": "budget"}),
        };
        let serialized = serde_json::to_string(&call).unwrap();
        let deserialized: ToolCall = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.id, "call_123");
        assert_eq!(deserialized.name, "search_emails");
        assert_eq!(deserialized.arguments["query"], "budget");
    }

    #[test]
    fn test_llm_response_variants() {
        let text_resp = LlmResponse::Text("Hello".to_string());
        match &text_resp {
            LlmResponse::Text(t) => assert_eq!(t, "Hello"),
            _ => panic!("Expected Text variant"),
        }

        let tool_resp = LlmResponse::ToolCalls {
            text: Some("Let me check".to_string()),
            calls: vec![ToolCall {
                id: "c1".to_string(),
                name: "inbox_stats".to_string(),
                arguments: serde_json::json!({}),
            }],
        };
        match &tool_resp {
            LlmResponse::ToolCalls { text, calls } => {
                assert_eq!(text.as_deref(), Some("Let me check"));
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "inbox_stats");
            }
            _ => panic!("Expected ToolCalls variant"),
        }
    }

    #[test]
    fn test_all_tools_defined() {
        let tools = all_tools();
        assert_eq!(tools.len(), 6);
        assert_eq!(tools[0].name, "inbox_stats");
        assert_eq!(tools[1].name, "search_emails");
        assert_eq!(tools[2].name, "read_email");
        assert_eq!(tools[3].name, "list_emails");
        assert_eq!(tools[4].name, "bulk_update_emails");
        assert_eq!(tools[5].name, "compose_email");
    }

    #[test]
    fn test_execute_unknown_tool() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let result = execute_tool(&conn, None, "nonexistent", &serde_json::json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown tool"));
    }

    #[test]
    fn test_handle_inbox_stats() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        // Should succeed even with empty DB (returns empty array)
        let result = execute_tool(&conn, None, "inbox_stats", &serde_json::json!({}));
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val.is_array());
    }

    #[test]
    fn test_handle_search_emails() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Insert test account
        conn.execute(
            "INSERT INTO accounts (id, provider, email, display_name) \
             VALUES ('acct1', 'gmail', 'test@example.com', 'Test User')",
            [],
        )
        .unwrap();

        let now = chrono::Utc::now().timestamp();
        // Insert test message
        conn.execute(
            "INSERT INTO messages (id, account_id, message_id, subject, body_text, \
             from_name, from_address, date, is_read, is_draft) \
             VALUES ('msg-001-uuid', 'acct1', '<msg1@example.com>', 'Budget Report Q4', \
             'The quarterly budget report shows expenses increased by 15 percent.', \
             'Alice Smith', 'alice@example.com', ?1, 0, 0)",
            params![now],
        )
        .unwrap();

        let result = execute_tool(
            &conn,
            None,
            "search_emails",
            &serde_json::json!({"query": "budget report"}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        let arr = val.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "msg-001-uuid");
        assert_eq!(arr[0]["subject"], "Budget Report Q4");
        assert_eq!(arr[0]["is_read"], false);
    }

    #[test]
    fn test_handle_read_email_full_id() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO accounts (id, provider, email, display_name) \
             VALUES ('acct1', 'gmail', 'test@example.com', 'Test User')",
            [],
        )
        .unwrap();

        let now = chrono::Utc::now().timestamp();
        let msg_id = "abcdef01-2345-6789-abcd-ef0123456789";
        conn.execute(
            "INSERT INTO messages (id, account_id, message_id, subject, body_text, \
             from_name, from_address, to_addresses, date, is_read, is_starred, is_draft, \
             ai_category, ai_summary, has_attachments, attachment_names) \
             VALUES (?1, 'acct1', '<msg@example.com>', 'Test Subject', 'Hello world body text', \
             'Bob Jones', 'bob@example.com', '[\"test@example.com\"]', ?2, 1, 0, 0, \
             'primary', 'A test email', 0, '[]')",
            params![msg_id, now],
        )
        .unwrap();

        let result = execute_tool(
            &conn,
            None,
            "read_email",
            &serde_json::json!({"message_id": msg_id}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["id"], msg_id);
        assert_eq!(val["subject"], "Test Subject");
        assert_eq!(val["body"], "Hello world body text");
        assert_eq!(val["is_read"], true);
        assert_eq!(val["category"], "primary");
    }

    #[test]
    fn test_handle_read_email_truncated_id() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO accounts (id, provider, email, display_name) \
             VALUES ('acct1', 'gmail', 'test@example.com', 'Test User')",
            [],
        )
        .unwrap();

        let now = chrono::Utc::now().timestamp();
        let msg_id = "abcdef01-2345-6789-abcd-ef0123456789";
        conn.execute(
            "INSERT INTO messages (id, account_id, message_id, subject, body_text, \
             from_name, from_address, date, is_read, is_draft) \
             VALUES (?1, 'acct1', '<msg@example.com>', 'Truncated ID Test', 'Body content here', \
             'Carol', 'carol@example.com', ?2, 0, 0)",
            params![msg_id, now],
        )
        .unwrap();

        // Use only 8-char prefix
        let truncated = &msg_id[..8]; // "abcdef01"
        let result = execute_tool(
            &conn,
            None,
            "read_email",
            &serde_json::json!({"message_id": truncated}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["id"], msg_id);
        assert_eq!(val["subject"], "Truncated ID Test");
    }

    #[test]
    fn test_all_tools_includes_list_emails_and_bulk() {
        let tools = all_tools();
        assert!(tools.len() >= 5);
        assert_eq!(tools[3].name, "list_emails");
        assert_eq!(tools[4].name, "bulk_update_emails");
    }

    #[test]
    fn test_handle_list_emails_no_filters() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO accounts (id, provider, email, display_name) \
             VALUES ('acct1', 'gmail', 'test@example.com', 'Test')",
            [],
        )
        .unwrap();

        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO messages (id, account_id, subject, from_address, date, is_read, is_draft) \
             VALUES ('m1', 'acct1', 'Test Email', 'alice@test.com', ?1, 0, 0)",
            params![now],
        )
        .unwrap();

        let result = execute_tool(&conn, None, "list_emails", &serde_json::json!({}));
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["count"], 1);
        assert_eq!(val["emails"][0]["subject"], "Test Email");
    }

    #[test]
    fn test_handle_list_emails_with_filters() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO accounts (id, provider, email, display_name) \
             VALUES ('acct1', 'gmail', 'test@example.com', 'Test')",
            [],
        )
        .unwrap();

        let now = chrono::Utc::now().timestamp();
        // Insert one read and one unread message
        conn.execute(
            "INSERT INTO messages (id, account_id, subject, from_address, from_name, date, is_read, is_draft, ai_category) \
             VALUES ('m1', 'acct1', 'Read Email', 'alice@test.com', 'Alice', ?1, 1, 0, 'primary')",
            params![now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, subject, from_address, from_name, date, is_read, is_draft, ai_category) \
             VALUES ('m2', 'acct1', 'Unread Email', 'bob@test.com', 'Bob', ?1, 0, 0, 'updates')",
            params![now],
        )
        .unwrap();

        // Filter by is_read=false
        let result = execute_tool(
            &conn,
            None,
            "list_emails",
            &serde_json::json!({"is_read": false}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["count"], 1);
        assert_eq!(val["emails"][0]["subject"], "Unread Email");

        // Filter by category
        let result = execute_tool(
            &conn,
            None,
            "list_emails",
            &serde_json::json!({"category": "primary"}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["count"], 1);
        assert_eq!(val["emails"][0]["subject"], "Read Email");

        // Filter by sender
        let result = execute_tool(
            &conn,
            None,
            "list_emails",
            &serde_json::json!({"sender": "alice"}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["count"], 1);
        assert_eq!(val["emails"][0]["from"], "Alice <alice@test.com>");
    }

    #[test]
    fn test_handle_list_emails_sort_and_limit() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO accounts (id, provider, email, display_name) \
             VALUES ('acct1', 'gmail', 'test@example.com', 'Test')",
            [],
        )
        .unwrap();

        let now = chrono::Utc::now().timestamp();
        for i in 0..5 {
            conn.execute(
                "INSERT INTO messages (id, account_id, subject, from_address, date, is_read, is_draft) \
                 VALUES (?1, 'acct1', ?2, 'x@test.com', ?3, 0, 0)",
                params![format!("m{}", i), format!("Email {}", i), now + i * 100],
            )
            .unwrap();
        }

        // Limit to 2, newest first
        let result = execute_tool(
            &conn,
            None,
            "list_emails",
            &serde_json::json!({"limit": 2, "sort": "newest"}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["count"], 2);
        assert_eq!(val["emails"][0]["subject"], "Email 4"); // newest

        // Oldest first
        let result = execute_tool(
            &conn,
            None,
            "list_emails",
            &serde_json::json!({"sort": "oldest", "limit": 2}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["emails"][0]["subject"], "Email 0"); // oldest
    }

    #[test]
    fn test_search_emails_with_filters() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO accounts (id, provider, email, display_name) \
             VALUES ('acct1', 'gmail', 'test@example.com', 'Test')",
            [],
        )
        .unwrap();

        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO messages (id, account_id, subject, body_text, from_address, from_name, date, is_read, is_draft, ai_category) \
             VALUES ('m1', 'acct1', 'Security Alert Read', 'Your account was accessed', 'security@google.com', 'Google', ?1, 1, 0, 'updates')",
            params![now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, subject, body_text, from_address, from_name, date, is_read, is_draft, ai_category) \
             VALUES ('m2', 'acct1', 'Security Alert Unread', 'New sign-in detected', 'security@google.com', 'Google', ?1, 0, 0, 'updates')",
            params![now],
        )
        .unwrap();

        // Search with is_read filter
        let result = execute_tool(
            &conn,
            None,
            "search_emails",
            &serde_json::json!({"query": "security alert", "is_read": false}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        let arr = val.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["subject"], "Security Alert Unread");
    }

    // ── Bulk Update Tests ────────────────────────────────────────

    fn setup_bulk_test_db() -> (crate::db::DbPool, Vec<String>) {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO accounts (id, provider, email, display_name) \
             VALUES ('acct1', 'gmail', 'test@example.com', 'Test')",
            [],
        )
        .unwrap();

        let now = chrono::Utc::now().timestamp();
        let ids: Vec<String> = (0..5)
            .map(|i| {
                let id = format!("bulk-msg-{:04}-0000-0000-000000000000", i);
                conn.execute(
                    "INSERT INTO messages (id, account_id, subject, from_address, date, is_read, is_starred, is_draft, folder, ai_category) \
                     VALUES (?1, 'acct1', ?2, 'sender@test.com', ?3, 0, 0, 0, 'INBOX', 'primary')",
                    params![id, format!("Bulk Email {}", i), now + i as i64 * 100],
                )
                .unwrap();
                id
            })
            .collect();

        (pool, ids)
    }

    #[test]
    fn test_bulk_update_tool_in_all_tools() {
        let tools = all_tools();
        let bulk = tools.iter().find(|t| t.name == "bulk_update_emails");
        assert!(bulk.is_some());
        let schema = &bulk.unwrap().input_schema;
        let props = schema.get("properties").unwrap();
        assert!(props.get("action").is_some());
        assert!(props.get("message_ids").is_some());
        assert!(props.get("category").is_some());
    }

    #[test]
    fn test_bulk_update_empty_ids() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let result = execute_tool(
            &conn,
            None,
            "bulk_update_emails",
            &serde_json::json!({"action": "archive", "message_ids": []}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val.get("error").is_some());
        assert_eq!(val["updated"], 0);
    }

    #[test]
    fn test_bulk_update_unknown_action() {
        let (pool, ids) = setup_bulk_test_db();
        let conn = pool.get().unwrap();
        let result = execute_tool(
            &conn,
            None,
            "bulk_update_emails",
            &serde_json::json!({"action": "explode", "message_ids": [ids[0]]}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val["error"].as_str().unwrap().contains("Unknown action"));
    }

    #[test]
    fn test_bulk_update_archive() {
        let (pool, ids) = setup_bulk_test_db();
        let conn = pool.get().unwrap();
        let result = execute_tool(
            &conn,
            None,
            "bulk_update_emails",
            &serde_json::json!({"action": "archive", "message_ids": [ids[0], ids[1]]}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["action"], "archive");
        assert_eq!(val["updated"], 2);
        assert_eq!(val["status"], "all successful");

        // Verify DB state
        let folder: String = conn
            .query_row("SELECT folder FROM messages WHERE id = ?1", params![ids[0]], |row| row.get(0))
            .unwrap();
        assert_eq!(folder, "Archive");
    }

    #[test]
    fn test_bulk_update_mark_read() {
        let (pool, ids) = setup_bulk_test_db();
        let conn = pool.get().unwrap();
        let result = execute_tool(
            &conn,
            None,
            "bulk_update_emails",
            &serde_json::json!({"action": "mark_read", "message_ids": [ids[0], ids[1], ids[2]]}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["updated"], 3);

        let is_read: i32 = conn
            .query_row("SELECT is_read FROM messages WHERE id = ?1", params![ids[0]], |row| row.get(0))
            .unwrap();
        assert_eq!(is_read, 1);
    }

    #[test]
    fn test_bulk_update_mark_unread() {
        let (pool, ids) = setup_bulk_test_db();
        let conn = pool.get().unwrap();

        // First mark as read
        conn.execute("UPDATE messages SET is_read = 1 WHERE id = ?1", params![ids[0]]).unwrap();

        let result = execute_tool(
            &conn,
            None,
            "bulk_update_emails",
            &serde_json::json!({"action": "mark_unread", "message_ids": [ids[0]]}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["updated"], 1);

        let is_read: i32 = conn
            .query_row("SELECT is_read FROM messages WHERE id = ?1", params![ids[0]], |row| row.get(0))
            .unwrap();
        assert_eq!(is_read, 0);
    }

    #[test]
    fn test_bulk_update_trash() {
        let (pool, ids) = setup_bulk_test_db();
        let conn = pool.get().unwrap();
        let result = execute_tool(
            &conn,
            None,
            "bulk_update_emails",
            &serde_json::json!({"action": "trash", "message_ids": [ids[0]]}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["updated"], 1);

        let folder: String = conn
            .query_row("SELECT folder FROM messages WHERE id = ?1", params![ids[0]], |row| row.get(0))
            .unwrap();
        assert_eq!(folder, "Trash");
    }

    #[test]
    fn test_bulk_update_star_unstar() {
        let (pool, ids) = setup_bulk_test_db();
        let conn = pool.get().unwrap();

        // Star
        let result = execute_tool(
            &conn,
            None,
            "bulk_update_emails",
            &serde_json::json!({"action": "star", "message_ids": [ids[0], ids[1]]}),
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["updated"], 2);

        let starred: i32 = conn
            .query_row("SELECT is_starred FROM messages WHERE id = ?1", params![ids[0]], |row| row.get(0))
            .unwrap();
        assert_eq!(starred, 1);

        // Unstar
        let result = execute_tool(
            &conn,
            None,
            "bulk_update_emails",
            &serde_json::json!({"action": "unstar", "message_ids": [ids[0]]}),
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["updated"], 1);

        let starred: i32 = conn
            .query_row("SELECT is_starred FROM messages WHERE id = ?1", params![ids[0]], |row| row.get(0))
            .unwrap();
        assert_eq!(starred, 0);
    }

    #[test]
    fn test_bulk_update_move_to_category() {
        let (pool, ids) = setup_bulk_test_db();
        let conn = pool.get().unwrap();
        let result = execute_tool(
            &conn,
            None,
            "bulk_update_emails",
            &serde_json::json!({
                "action": "move_to_category",
                "message_ids": [ids[0], ids[1]],
                "category": "promotions"
            }),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["updated"], 2);

        let category: String = conn
            .query_row("SELECT ai_category FROM messages WHERE id = ?1", params![ids[0]], |row| row.get(0))
            .unwrap();
        assert_eq!(category, "promotions");
    }

    #[test]
    fn test_bulk_update_nonexistent_ids() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let result = execute_tool(
            &conn,
            None,
            "bulk_update_emails",
            &serde_json::json!({"action": "archive", "message_ids": ["nonexistent-id-here"]}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val.get("error").is_some());
        assert!(val["error"].as_str().unwrap().contains("could be resolved"));
    }

    #[test]
    fn test_bulk_update_truncated_ids() {
        let (pool, ids) = setup_bulk_test_db();
        let conn = pool.get().unwrap();

        // Use 8-char prefix
        let truncated = &ids[0][..10]; // "bulk-msg-0"
        let result = execute_tool(
            &conn,
            None,
            "bulk_update_emails",
            &serde_json::json!({"action": "mark_read", "message_ids": [truncated]}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["updated"], 1);
    }

    #[test]
    fn test_bulk_update_message_count_in_response() {
        let (pool, ids) = setup_bulk_test_db();
        let conn = pool.get().unwrap();
        let result = execute_tool(
            &conn,
            None,
            "bulk_update_emails",
            &serde_json::json!({"action": "star", "message_ids": [ids[0], ids[1], ids[2]]}),
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["requested"], 3);
        assert_eq!(val["resolved"], 3);
        assert_eq!(val["updated"], 3);
    }

    // ── Compose Email Tests ────────────────────────────────────────

    #[test]
    fn test_compose_email_in_all_tools() {
        let tools = all_tools();
        assert!(
            tools.iter().any(|t| t.name == "compose_email"),
            "compose_email should be in all_tools()"
        );
    }

    #[test]
    fn test_compose_email_tool_definition() {
        let tools = all_tools();
        let compose = tools.iter().find(|t| t.name == "compose_email").unwrap();
        let required = compose.input_schema["required"].as_array().unwrap();
        let required_names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_names.contains(&"to"));
        assert!(required_names.contains(&"subject"));
        assert!(required_names.contains(&"body"));
    }

    #[test]
    fn test_handle_compose_all_fields() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            id: String::new(),
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["alice@example.com"],
                "cc": ["bob@example.com"],
                "subject": "Project Update",
                "body": "<p>Here is the update.</p>",
                "tone": "formal"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(result.success);
        assert!(result.message.contains("alice@example.com"));
        assert!(result.message.contains("Project Update"));
        assert!(result.message.contains("formal"));
        assert!(result.data.is_some());

        let data: ComposeEmailData =
            serde_json::from_value(result.data.unwrap()).unwrap();
        assert_eq!(data.to, vec!["alice@example.com"]);
        assert_eq!(data.cc, vec!["bob@example.com"]);
        assert_eq!(data.subject, "Project Update");
    }

    #[test]
    fn test_handle_compose_minimal_fields() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            id: String::new(),
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["alice@example.com"],
                "subject": "Hello",
                "body": "Hi there"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(result.success);
        assert!(result.message.contains("alice@example.com"));
        assert!(result.message.contains("Hello"));
        // Default tone should be formal
        assert!(result.message.contains("formal"));

        let data: ComposeEmailData =
            serde_json::from_value(result.data.unwrap()).unwrap();
        assert!(data.cc.is_empty());
        assert!(data.tone.is_none());
        assert!(data.reply_to_message_id.is_none());
    }

    #[test]
    fn test_handle_compose_reply() {
        let conn = make_test_db();
        // Insert a message to reply to
        conn.execute(
            "INSERT INTO messages (id, account_id, subject, folder) VALUES ('msg-123', 'acc-1', 'Original Subject', 'INBOX')",
            [],
        )
        .unwrap();

        let tool_call = ToolCall {
            id: String::new(),
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["sender@example.com"],
                "subject": "Re: Original Subject",
                "body": "Thanks for your email.",
                "reply_to_message_id": "msg-123"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(result.success);
        assert!(result.message.contains("reply"));

        let data: ComposeEmailData =
            serde_json::from_value(result.data.unwrap()).unwrap();
        assert_eq!(data.reply_to_message_id.as_deref(), Some("msg-123"));
        assert_eq!(data.thread_subject.as_deref(), Some("Original Subject"));
    }

    #[test]
    fn test_handle_compose_reply_nonexistent_message() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            id: String::new(),
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["sender@example.com"],
                "subject": "Re: Something",
                "body": "Reply text",
                "reply_to_message_id": "nonexistent-id"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        // Should still succeed -- just no thread context
        assert!(result.success);
        let data: ComposeEmailData =
            serde_json::from_value(result.data.unwrap()).unwrap();
        assert!(data.thread_subject.is_none());
    }

    #[test]
    fn test_handle_compose_missing_to() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            id: String::new(),
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": [],
                "subject": "Hello",
                "body": "Content"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(!result.success);
        assert!(result.message.contains("recipient"));
    }

    #[test]
    fn test_handle_compose_missing_subject() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            id: String::new(),
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["a@b.com"],
                "subject": "",
                "body": "Content"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(!result.success);
        assert!(result.message.contains("Subject"));
    }

    #[test]
    fn test_handle_compose_missing_body() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            id: String::new(),
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["a@b.com"],
                "subject": "Hello",
                "body": "   "
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(!result.success);
        assert!(result.message.contains("body"));
    }

    #[test]
    fn test_handle_compose_invalid_email() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            id: String::new(),
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["not-an-email"],
                "subject": "Hello",
                "body": "Content"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(!result.success);
        assert!(result.message.contains("Invalid email"));
    }

    #[test]
    fn test_handle_compose_invalid_cc_email() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            id: String::new(),
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["valid@example.com"],
                "cc": ["bad"],
                "subject": "Hello",
                "body": "Content"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(!result.success);
        assert!(result.message.contains("Invalid CC"));
    }

    #[test]
    fn test_handle_compose_tone_handling() {
        let conn = make_test_db();

        for tone in &["formal", "casual", "brief"] {
            let tool_call = ToolCall {
                id: String::new(),
                name: "compose_email".to_string(),
                arguments: serde_json::json!({
                    "to": ["a@b.com"],
                    "subject": "Test",
                    "body": "Content",
                    "tone": tone
                }),
            };
            let result = handle_compose_email(&conn, &tool_call);
            assert!(result.success, "Should succeed with tone {tone}");
            assert!(
                result.message.contains(tone),
                "Summary should mention tone {tone}"
            );
            let data: ComposeEmailData =
                serde_json::from_value(result.data.unwrap()).unwrap();
            assert_eq!(data.tone.as_deref(), Some(*tone));
        }
    }

    #[test]
    fn test_handle_compose_invalid_arguments() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            id: String::new(),
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": "not-an-array",
                "subject": "Hello"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(!result.success);
        assert!(result.message.contains("Invalid"));
    }

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<p>Hello <b>world</b></p>"), "Hello world");
        assert_eq!(strip_html_tags("no tags"), "no tags");
        assert_eq!(strip_html_tags("<br/>line<br/>break"), "linebreak");
        assert_eq!(strip_html_tags(""), "");
    }

    #[test]
    fn test_execute_compose_email() {
        let conn = make_test_db();
        let data = ComposeEmailData {
            to: vec!["alice@example.com".to_string()],
            cc: vec![],
            subject: "Test Draft".to_string(),
            body: "<p>Draft body</p>".to_string(),
            reply_to_message_id: None,
            tone: Some("casual".to_string()),
            thread_subject: None,
        };
        let result = execute_compose_email(&conn, &data, "acc-1");
        assert!(result.is_ok());
        let draft_id = result.unwrap();
        assert!(!draft_id.is_empty());

        // Verify draft was saved
        let is_draft: bool = conn
            .query_row(
                "SELECT is_draft FROM messages WHERE id = ?1",
                rusqlite::params![draft_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(is_draft);
    }

    #[test]
    fn test_execute_compose_email_with_cc() {
        let conn = make_test_db();
        let data = ComposeEmailData {
            to: vec!["alice@example.com".to_string()],
            cc: vec!["bob@example.com".to_string()],
            subject: "CC Test".to_string(),
            body: "Body".to_string(),
            reply_to_message_id: None,
            tone: None,
            thread_subject: None,
        };
        let result = execute_compose_email(&conn, &data, "acc-1");
        assert!(result.is_ok());

        let draft_id = result.unwrap();
        let cc_json: Option<String> = conn
            .query_row(
                "SELECT cc_addresses FROM messages WHERE id = ?1",
                rusqlite::params![draft_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(cc_json.is_some());
        assert!(cc_json.unwrap().contains("bob@example.com"));
    }
}
