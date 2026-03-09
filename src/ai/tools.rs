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

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

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
        assert_eq!(tools.len(), 4);
        assert_eq!(tools[0].name, "inbox_stats");
        assert_eq!(tools[1].name, "search_emails");
        assert_eq!(tools[2].name, "read_email");
        assert_eq!(tools[3].name, "list_emails");
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
    fn test_all_tools_includes_list_emails() {
        let tools = all_tools();
        assert_eq!(tools.len(), 4);
        assert_eq!(tools[3].name, "list_emails");
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
}
