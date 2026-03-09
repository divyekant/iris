# V11-S2: Agentic Chat Core Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace single-shot RAG chat with an iterative tool-use loop where the LLM decides what information it needs by calling tools.

**Architecture:** Add `generate_with_tools()` to each provider (Anthropic/OpenAI native tool use, Ollama text fallback). Define 3 tools (inbox_stats, search_emails, read_email) in a new tool registry module. Refactor the chat handler into an agentic loop (max 5 iterations) that calls the LLM with tools, executes any tool calls, appends results, and repeats until the LLM produces a text response.

**Tech Stack:** Rust (Axum, reqwest, serde, serde_json, rusqlite, chrono)

---

### Task 1: Core types — Tool, ToolCall, LlmResponse, LlmMessage

**Files:**
- Create: `src/ai/tools.rs`
- Modify: `src/ai/mod.rs` (add `pub mod tools;`)

**Step 1: Create the types module with tests**

Create `src/ai/tools.rs` with:

```rust
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Tool definition — sent to LLM
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

// ---------------------------------------------------------------------------
// LLM conversation messages (provider-agnostic)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum LlmMessage {
    /// User text message
    User(String),
    /// Assistant text response (no tool calls)
    AssistantText(String),
    /// Assistant response that includes tool calls
    AssistantToolCalls {
        text: Option<String>,
        tool_calls: Vec<ToolCall>,
    },
    /// Result of executing a tool
    ToolResult {
        tool_call_id: String,
        content: String,
    },
}

// ---------------------------------------------------------------------------
// LLM response — either final text or tool call requests
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tool call result metadata (for tracking in chat response)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallRecord {
    pub name: String,
    pub arguments: serde_json::Value,
    pub result_preview: String,
}
```

Add `pub mod tools;` to `src/ai/mod.rs`.

**Step 2: Add tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_serialization() {
        let tool = Tool {
            name: "search_emails".to_string(),
            description: "Search emails".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
        };
        let json = serde_json::to_value(&tool).unwrap();
        assert_eq!(json["name"], "search_emails");
        assert!(json["input_schema"]["properties"]["query"].is_object());
    }

    #[test]
    fn test_tool_call_roundtrip() {
        let call = ToolCall {
            id: "call_123".to_string(),
            name: "read_email".to_string(),
            arguments: serde_json::json!({"message_id": "abc"}),
        };
        let json = serde_json::to_string(&call).unwrap();
        let parsed: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "read_email");
        assert_eq!(parsed.arguments["message_id"], "abc");
    }

    #[test]
    fn test_llm_response_variants() {
        let text = LlmResponse::Text("Hello".to_string());
        assert!(matches!(text, LlmResponse::Text(_)));

        let tool = LlmResponse::ToolCalls {
            text: Some("I'll search.".to_string()),
            calls: vec![ToolCall {
                id: "1".to_string(),
                name: "search_emails".to_string(),
                arguments: serde_json::json!({"query": "test"}),
            }],
        };
        assert!(matches!(tool, LlmResponse::ToolCalls { .. }));
    }
}
```

**Step 3: Run tests**

Run: `~/.cargo/bin/cargo test -p iris-server tools::tests -- --nocapture`
Expected: 3 tests PASS

**Step 4: Commit**

```bash
git add src/ai/tools.rs src/ai/mod.rs
git commit -m "feat(v11-s2): core types — Tool, ToolCall, LlmResponse, LlmMessage"
```

---

### Task 2: Tool handler implementations

**Files:**
- Modify: `src/ai/tools.rs` (add tool definitions + handlers)

The tool handlers need DB access. Each handler is a plain function `fn(conn, args) -> Result<serde_json::Value, String>`.

**Step 1: Add tool definitions and handlers**

Add to `src/ai/tools.rs`:

```rust
use rusqlite::Connection;

// ---------------------------------------------------------------------------
// Tool definitions (JSON schemas for LLM)
// ---------------------------------------------------------------------------

pub fn all_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "inbox_stats".to_string(),
            description: "Get inbox statistics: total emails, unread count, starred count, \
                          emails today/this week/this month, top senders, and category breakdown. \
                          Use for aggregate questions like 'how many emails' or 'who sends me the most'."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "search_emails".to_string(),
            description: "Search the user's emails by text query. Returns matching emails with \
                          ID, subject, sender, date, read status, and a text snippet. \
                          Use when the user asks about specific topics, senders, or email content."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (keywords, sender name, subject text)"
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "read_email".to_string(),
            description: "Read the full content of a specific email by its ID. Returns the complete \
                          body text, all headers, and attachment names. Use when you need details \
                          beyond the snippet returned by search_emails."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message_id": {
                        "type": "string",
                        "description": "The email message ID (from search_emails results)"
                    }
                },
                "required": ["message_id"]
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Tool executor — dispatches tool calls to handlers
// ---------------------------------------------------------------------------

pub fn execute_tool(
    conn: &Connection,
    memories: Option<&crate::ai::memories::MemoriesClient>,
    name: &str,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match name {
        "inbox_stats" => handle_inbox_stats(conn),
        "search_emails" => {
            let query = arguments.get("query")
                .and_then(|v| v.as_str())
                .ok_or("Missing required argument: query")?;
            handle_search_emails(conn, query)
        }
        "read_email" => {
            let msg_id = arguments.get("message_id")
                .and_then(|v| v.as_str())
                .ok_or("Missing required argument: message_id")?;
            handle_read_email(conn, msg_id)
        }
        _ => Err(format!("Unknown tool: {}", name)),
    }
}

// ---------------------------------------------------------------------------
// inbox_stats handler
// ---------------------------------------------------------------------------

fn handle_inbox_stats(conn: &Connection) -> Result<serde_json::Value, String> {
    let stats = crate::api::inbox_stats::get_all_stats(conn)
        .map_err(|e| format!("Failed to get inbox stats: {}", e))?;
    serde_json::to_value(&stats).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// search_emails handler — reuses FTS5 search from chat module
// ---------------------------------------------------------------------------

fn handle_search_emails(conn: &Connection, query: &str) -> Result<serde_json::Value, String> {
    // Build FTS5 query from user input (same logic as chat.rs search_relevant_emails_fts)
    let sanitized: String = query
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let terms: Vec<String> = sanitized
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .take(5)
        .map(|w| format!("\"{}\"", w))
        .collect();
    if terms.is_empty() {
        return Ok(serde_json::json!([]));
    }
    let fts_query = terms.join(" OR ");

    let mut stmt = conn.prepare(
        "SELECT m.id, m.subject, m.from_name, m.from_address,
                snippet(fts_messages, -1, '', '', '...', 40) as snip,
                m.date, m.is_read
         FROM fts_messages fts
         JOIN messages m ON m.rowid = fts.rowid
         WHERE fts_messages MATCH ?1
         ORDER BY rank
         LIMIT 10",
    ).map_err(|e| e.to_string())?;

    let results: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params![fts_query], |row| {
            let from_name: Option<String> = row.get(2)?;
            let from_addr: Option<String> = row.get(3)?;
            let date: Option<i64> = row.get(5)?;
            let is_read: Option<bool> = row.get::<_, Option<i32>>(6)?.map(|v| v != 0);
            let date_str = date
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .map(|dt| dt.with_timezone(&chrono::Local).format("%b %-d, %Y %H:%M").to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "subject": row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                "from": from_name.or(from_addr).unwrap_or_default(),
                "date": date_str,
                "is_read": is_read.unwrap_or(true),
                "snippet": row.get::<_, String>(4).unwrap_or_default(),
            }))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(serde_json::Value::Array(results))
}

// ---------------------------------------------------------------------------
// read_email handler — fetch full content by ID
// ---------------------------------------------------------------------------

fn handle_read_email(conn: &Connection, message_id: &str) -> Result<serde_json::Value, String> {
    // Support 8-char truncated IDs (from previous search results)
    let full_id = if message_id.len() < 36 {
        let pattern = format!("{}%", message_id);
        conn.query_row(
            "SELECT id FROM messages WHERE id LIKE ?1 LIMIT 1",
            rusqlite::params![pattern],
            |row| row.get::<_, String>(0),
        ).map_err(|_| format!("Email not found: {}", message_id))?
    } else {
        message_id.to_string()
    };

    conn.query_row(
        "SELECT id, subject, from_name, from_address, to_addresses, date, body_text,
                is_read, is_starred, ai_category, ai_summary, has_attachments, attachment_names
         FROM messages WHERE id = ?1",
        rusqlite::params![full_id],
        |row| {
            let from_name: Option<String> = row.get(2)?;
            let from_addr: Option<String> = row.get(3)?;
            let date: Option<i64> = row.get(5)?;
            let date_str = date
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .map(|dt| dt.with_timezone(&chrono::Local).format("%b %-d, %Y %H:%M").to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let body: String = row.get::<_, Option<String>>(6)?.unwrap_or_default();
            // Truncate very long bodies to stay within token budget
            let body_truncated = if body.len() > 4000 {
                format!("{}...[truncated]", &body[..4000])
            } else {
                body
            };

            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "subject": row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                "from": from_name.or(from_addr).unwrap_or_default(),
                "to": row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                "date": date_str,
                "body": body_truncated,
                "is_read": row.get::<_, Option<i32>>(7)?.map(|v| v != 0).unwrap_or(true),
                "is_starred": row.get::<_, Option<i32>>(8)?.map(|v| v != 0).unwrap_or(false),
                "category": row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                "summary": row.get::<_, Option<String>>(10)?.unwrap_or_default(),
                "has_attachments": row.get::<_, Option<i32>>(11)?.map(|v| v != 0).unwrap_or(false),
                "attachment_names": row.get::<_, Option<String>>(12)?.unwrap_or_default(),
            }))
        },
    ).map_err(|_| format!("Email not found: {}", message_id))
}
```

**Step 2: Add handler tests**

```rust
    #[test]
    fn test_all_tools_defined() {
        let tools = all_tools();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].name, "inbox_stats");
        assert_eq!(tools[1].name, "search_emails");
        assert_eq!(tools[2].name, "read_email");
    }

    #[test]
    fn test_execute_unknown_tool() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();
        let result = execute_tool(&conn, None, "unknown_tool", &serde_json::json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown tool"));
    }

    #[test]
    fn test_handle_inbox_stats() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();
        // Should succeed even with no data
        let result = execute_tool(&conn, None, "inbox_stats", &serde_json::json!({}));
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_search_emails() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();
        // Insert a test message
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('a1', 'gmail', 'test@test.com')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, from_address, from_name, subject, body_text, date, is_read, is_draft)
             VALUES ('msg1', 'a1', 'alice@test.com', 'Alice', 'Security Alert', 'Your account was accessed', strftime('%s','now'), 0, 0)",
            [],
        ).unwrap();
        // FTS5 triggers should auto-index
        let result = execute_tool(&conn, None, "search_emails", &serde_json::json!({"query": "security alert"}));
        assert!(result.is_ok());
        let arr = result.unwrap();
        assert!(arr.as_array().unwrap().len() >= 1);
        assert_eq!(arr[0]["subject"], "Security Alert");
    }

    #[test]
    fn test_handle_read_email_full_id() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('a1', 'gmail', 'test@test.com')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, from_address, subject, body_text, date, is_read, is_draft)
             VALUES ('abcdef12-3456-7890-abcd-ef1234567890', 'a1', 'bob@test.com', 'Test Email', 'Hello World', strftime('%s','now'), 0, 0)",
            [],
        ).unwrap();
        let result = execute_tool(&conn, None, "read_email", &serde_json::json!({"message_id": "abcdef12-3456-7890-abcd-ef1234567890"}));
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["body"], "Hello World");
    }

    #[test]
    fn test_handle_read_email_truncated_id() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('a1', 'gmail', 'test@test.com')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, from_address, subject, body_text, date, is_read, is_draft)
             VALUES ('abcdef12-3456-7890-abcd-ef1234567890', 'a1', 'bob@test.com', 'Test', 'Content', strftime('%s','now'), 0, 0)",
            [],
        ).unwrap();
        // 8-char truncated ID should resolve
        let result = execute_tool(&conn, None, "read_email", &serde_json::json!({"message_id": "abcdef12"}));
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["subject"], "Test");
    }
```

**Step 3: Run tests**

Run: `~/.cargo/bin/cargo test -p iris-server tools -- --nocapture`
Expected: All tests PASS (3 from Task 1 + 6 new = 9)

**Step 4: Commit**

```bash
git add src/ai/tools.rs
git commit -m "feat(v11-s2): tool handlers — inbox_stats, search_emails, read_email"
```

---

### Task 3: Provider generate_with_tools

**Files:**
- Modify: `src/ai/anthropic.rs` (add generate_with_tools)
- Modify: `src/ai/openai.rs` (add generate_with_tools)
- Modify: `src/ai/ollama.rs` (add generate_with_tools)
- Modify: `src/ai/provider.rs` (add dispatch + pool method)

**Step 1: Anthropic — native tool use**

In `src/ai/anthropic.rs`, add new request/response types alongside existing ones:

```rust
// --- Tool use types (alongside existing MessagesRequest) ---

#[derive(Debug, Serialize)]
struct ToolUseRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<ToolUseMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<AnthropicTool>,
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ToolUseMessage {
    role: String,
    content: serde_json::Value, // String or Array of content blocks
}

#[derive(Debug, Deserialize)]
struct ToolUseResponse {
    content: Vec<ToolUseContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolUseContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<serde_json::Value>,
}
```

Add the method to `AnthropicClient`:

```rust
    pub async fn generate_with_tools(
        &self,
        messages: &[crate::ai::tools::LlmMessage],
        system: Option<&str>,
        tools: &[crate::ai::tools::Tool],
    ) -> Option<crate::ai::tools::LlmResponse> {
        use crate::ai::tools::{LlmMessage, LlmResponse, ToolCall};

        // Convert LlmMessage to Anthropic format
        let api_messages: Vec<ToolUseMessage> = messages.iter().map(|m| match m {
            LlmMessage::User(text) => ToolUseMessage {
                role: "user".to_string(),
                content: serde_json::Value::String(text.clone()),
            },
            LlmMessage::AssistantText(text) => ToolUseMessage {
                role: "assistant".to_string(),
                content: serde_json::Value::String(text.clone()),
            },
            LlmMessage::AssistantToolCalls { text, tool_calls } => {
                let mut blocks = Vec::new();
                if let Some(t) = text {
                    blocks.push(serde_json::json!({"type": "text", "text": t}));
                }
                for tc in tool_calls {
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": tc.arguments,
                    }));
                }
                ToolUseMessage {
                    role: "assistant".to_string(),
                    content: serde_json::Value::Array(blocks),
                }
            },
            LlmMessage::ToolResult { tool_call_id, content } => ToolUseMessage {
                role: "user".to_string(),
                content: serde_json::json!([{
                    "type": "tool_result",
                    "tool_use_id": tool_call_id,
                    "content": content,
                }]),
            },
        }).collect();

        // Convert tools to Anthropic format
        let api_tools: Vec<AnthropicTool> = tools.iter().map(|t| AnthropicTool {
            name: t.name.clone(),
            description: t.description.clone(),
            input_schema: t.input_schema.clone(),
        }).collect();

        let body = ToolUseRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            system: system.map(|s| s.to_string()),
            messages: api_messages,
            tools: api_tools,
        };

        let url = if self.is_oauth {
            format!("{}?beta=true", ANTHROPIC_API_URL)
        } else {
            ANTHROPIC_API_URL.to_string()
        };

        let mut req = self.client.post(&url).json(&body);
        req = req.header("anthropic-version", "2023-06-01");
        if self.is_oauth {
            req = req
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("anthropic-beta", OAUTH_BETA_HEADER);
        } else {
            req = req.header("x-api-key", &self.api_key);
        }

        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ToolUseResponse>().await {
                    Ok(r) => {
                        let mut text_parts = Vec::new();
                        let mut tool_calls = Vec::new();
                        for block in &r.content {
                            match block.block_type.as_str() {
                                "text" => {
                                    if let Some(t) = &block.text {
                                        text_parts.push(t.clone());
                                    }
                                }
                                "tool_use" => {
                                    if let (Some(id), Some(name), Some(input)) =
                                        (&block.id, &block.name, &block.input) {
                                        tool_calls.push(ToolCall {
                                            id: id.clone(),
                                            name: name.clone(),
                                            arguments: input.clone(),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                        let text = if text_parts.is_empty() { None } else { Some(text_parts.join("\n")) };
                        if tool_calls.is_empty() {
                            Some(LlmResponse::Text(text.unwrap_or_default()))
                        } else {
                            Some(LlmResponse::ToolCalls { text, calls: tool_calls })
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse Anthropic tool use response: {e}");
                        None
                    }
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!("Anthropic tool use returned {}: {}", status, body);
                None
            }
            Err(e) => {
                tracing::warn!("Anthropic tool use request failed: {e}");
                None
            }
        }
    }
```

**Step 2: OpenAI — native function calling**

In `src/ai/openai.rs`, add alongside existing types:

```rust
#[derive(Debug, Serialize)]
struct ToolChatRequest {
    model: String,
    messages: Vec<ToolChatMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OpenAITool>,
}

#[derive(Debug, Serialize, Clone)]
struct ToolChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCallOut>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAIFunction,
}

#[derive(Debug, Serialize)]
struct OpenAIFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize, Clone)]
struct OpenAIToolCallOut {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAIFunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String, // JSON string
}

#[derive(Debug, Deserialize)]
struct ToolChatResponse {
    choices: Vec<ToolChoice>,
}

#[derive(Debug, Deserialize)]
struct ToolChoice {
    message: ToolChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ToolChoiceMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAIToolCallIn>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIToolCallIn {
    id: String,
    function: OpenAIFunctionCall,
}
```

Add the `generate_with_tools` method to `OpenAIClient` following the same pattern as Anthropic — convert `LlmMessage` to OpenAI format (system/user/assistant/tool roles), convert tools to OpenAI function format, parse response for either content or tool_calls.

**Step 3: Ollama — text-based fallback**

In `src/ai/ollama.rs`, add a `generate_with_tools` method that:
1. Builds a tool description block and prepends it to the system prompt
2. Calls the existing `generate()` method
3. Parses `TOOL_CALL:{"name":"...","arguments":{...}}` from the response text
4. Returns `LlmResponse::Text` or `LlmResponse::ToolCalls` accordingly

The tool description format:
```
You have access to these tools. To use a tool, output TOOL_CALL: followed by a JSON object.
Only output ONE tool call per response. After receiving results, continue your analysis.

Available tools:
- inbox_stats(): Get inbox statistics...
- search_emails(query: string): Search emails...
- read_email(message_id: string): Read full email...

Example: TOOL_CALL:{"name":"search_emails","arguments":{"query":"security"}}
```

**Step 4: Provider dispatch + Pool method**

In `src/ai/provider.rs`, add to `LlmProvider`:

```rust
    pub async fn generate_with_tools(
        &self,
        messages: &[crate::ai::tools::LlmMessage],
        system: Option<&str>,
        tools: &[crate::ai::tools::Tool],
    ) -> Option<crate::ai::tools::LlmResponse> {
        match self {
            Self::Ollama(c) => c.generate_with_tools(messages, system, tools).await,
            Self::Anthropic(c) => c.generate_with_tools(messages, system, tools).await,
            Self::OpenAI(c) => c.generate_with_tools(messages, system, tools).await,
        }
    }
```

Add to `ProviderPool`:

```rust
    pub async fn generate_with_tools(
        &self,
        messages: &[crate::ai::tools::LlmMessage],
        system: Option<&str>,
        tools: &[crate::ai::tools::Tool],
    ) -> Option<crate::ai::tools::LlmResponse> {
        let n = self.providers.len();
        if n == 0 { return None; }
        let start = self.counter.fetch_add(1, Ordering::Relaxed) % n;
        for i in 0..n {
            let idx = (start + i) % n;
            let provider = &self.providers[idx];
            if let Some(result) = provider.generate_with_tools(messages, system, tools).await {
                return Some(result);
            }
            tracing::debug!("Provider {} (tools) failed, trying next", provider.name());
        }
        None
    }
```

**Step 5: Run all tests**

Run: `~/.cargo/bin/cargo test -p iris-server -- --nocapture`
Expected: All PASS (existing tests unchanged, new methods don't break anything)

**Step 6: Commit**

```bash
git add src/ai/anthropic.rs src/ai/openai.rs src/ai/ollama.rs src/ai/provider.rs
git commit -m "feat(v11-s2): generate_with_tools — Anthropic native, OpenAI native, Ollama fallback"
```

---

### Task 4: Agentic chat handler

**Files:**
- Modify: `src/api/chat.rs` (refactor chat handler to use agentic loop)

This is the core refactor. Replace the single-shot prompt+generate flow with an iterative tool-use loop.

**Step 1: Add the agentic loop function**

Add to `src/api/chat.rs` a new function:

```rust
/// Run the agentic tool-use loop. Calls the LLM with tools, executes any tool calls,
/// appends results, and repeats until the LLM produces a text response or max iterations.
async fn agentic_chat(
    providers: &crate::ai::provider::ProviderPool,
    db: &crate::db::DbPool,
    memories: &crate::ai::memories::MemoriesClient,
    system_prompt: &str,
    initial_user_message: &str,
    history: &[ChatMessage],
    max_iterations: usize,
) -> Result<(String, Vec<Citation>, Vec<crate::ai::tools::ToolCallRecord>), StatusCode> {
    use crate::ai::tools::{self, LlmMessage, LlmResponse, ToolCallRecord};

    let tools = tools::all_tools();
    let mut tool_records: Vec<ToolCallRecord> = Vec::new();
    let mut all_citations: Vec<Citation> = Vec::new();

    // Build initial message list from conversation history
    let mut messages: Vec<LlmMessage> = Vec::new();
    for msg in history.iter().rev().take(6).collect::<Vec<_>>().into_iter().rev() {
        match msg.role.as_str() {
            "user" => messages.push(LlmMessage::User(msg.content.clone())),
            "assistant" => messages.push(LlmMessage::AssistantText(msg.content.clone())),
            _ => {}
        }
    }
    messages.push(LlmMessage::User(initial_user_message.to_string()));

    for iteration in 0..max_iterations {
        let response = providers
            .generate_with_tools(&messages, Some(system_prompt), &tools)
            .await
            .ok_or(StatusCode::BAD_GATEWAY)?;

        match response {
            LlmResponse::Text(text) => {
                return Ok((text, all_citations, tool_records));
            }
            LlmResponse::ToolCalls { text, calls } => {
                tracing::info!(iteration, num_calls = calls.len(), "Agentic loop: tool calls");

                // Append assistant message with tool calls
                messages.push(LlmMessage::AssistantToolCalls {
                    text: text.clone(),
                    tool_calls: calls.clone(),
                });

                // Execute each tool call
                let conn = db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                for call in &calls {
                    let result = tools::execute_tool(&conn, Some(memories), &call.name, &call.arguments);
                    let result_json = match &result {
                        Ok(v) => serde_json::to_string(v).unwrap_or_default(),
                        Err(e) => serde_json::json!({"error": e}).to_string(),
                    };

                    // Track for response metadata
                    tool_records.push(ToolCallRecord {
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                        result_preview: result_json.chars().take(200).collect(),
                    });

                    // Extract citations from search results
                    if call.name == "search_emails" {
                        if let Ok(ref v) = result {
                            if let Some(arr) = v.as_array() {
                                for item in arr {
                                    if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                                        all_citations.push(Citation {
                                            message_id: id.to_string(),
                                            subject: item.get("subject").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                            from: item.get("from").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                            snippet: item.get("snippet").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                            date: None, // Dates are formatted strings in tool results
                                            is_read: item.get("is_read").and_then(|v| v.as_bool()),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    if call.name == "read_email" {
                        if let Ok(ref v) = result {
                            if let Some(id) = v.get("id").and_then(|v| v.as_str()) {
                                // Add/update citation with full content info
                                if !all_citations.iter().any(|c| c.message_id == id) {
                                    all_citations.push(Citation {
                                        message_id: id.to_string(),
                                        subject: v.get("subject").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        from: v.get("from").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                        snippet: v.get("body").and_then(|v| v.as_str()).unwrap_or("").chars().take(100).collect(),
                                        date: None,
                                        is_read: v.get("is_read").and_then(|v| v.as_bool()),
                                    });
                                }
                            }
                        }
                    }

                    // Append tool result message
                    messages.push(LlmMessage::ToolResult {
                        tool_call_id: call.id.clone(),
                        content: result_json,
                    });
                }
            }
        }
    }

    // Max iterations reached — force a text response with no tools
    tracing::warn!("Agentic loop reached max iterations ({}), forcing text response", max_iterations);
    let final_response = providers
        .generate_with_tools(&messages, Some(system_prompt), &[])
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;
    match final_response {
        LlmResponse::Text(text) => Ok((text, all_citations, tool_records)),
        _ => Ok(("I apologize, I was unable to complete my analysis. Please try rephrasing your question.".to_string(), all_citations, tool_records)),
    }
}
```

**Step 2: Refactor the chat handler to use agentic_chat**

Replace the current Phase 2-4 in the `chat()` handler with:

```rust
    // Phase 2-4: Agentic tool-use loop (replaces single-shot RAG)
    let system_prompt = {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        chat_system_prompt_with_stats(&conn)
    };

    let (ai_response, citations, tool_records) = agentic_chat(
        &state.providers,
        &state.db,
        &state.memories,
        &system_prompt,
        &input.message,
        &history,
        5, // max iterations
    ).await?;

    let (clean_content, proposed_action) = parse_action_proposal(&ai_response);

    // Filter citations to only those referenced in the response
    let referenced_citations: Vec<Citation> = citations
        .iter()
        .filter(|c| ai_response.contains(&c.message_id[..8.min(c.message_id.len())]))
        .cloned()
        .collect();
```

This removes the FTS5 search, Memories search, citation resolution, prompt building, and single generate() call — all of that is now inside the tools called by the agentic loop.

Keep Phase 5 (store + summarization trigger) and the response construction unchanged.

**Step 3: Update the system prompt**

In `chat_system_prompt_with_stats_str()`, update the prompt to reflect tool-based retrieval:

Replace:
```
You have access to the user's recent emails provided as context below.
```

With:
```
You have access to tools that let you search and read the user's emails. Use them to find relevant information before answering.
```

Remove the bullets about `[ID] markers` (the LLM now gets structured tool results, not pre-formatted context). Add:
```
- When you need information, use the available tools (search_emails, read_email, inbox_stats)
- Search first, then read specific emails for details
- Cite emails by referencing their ID from tool results
```

**Step 4: Add a test for agentic_chat**

Since agentic_chat requires actual LLM providers (which we can't mock easily in unit tests), add an integration-level test that verifies the tool execution path:

```rust
    #[test]
    fn test_tool_records_serialization() {
        let record = crate::ai::tools::ToolCallRecord {
            name: "search_emails".to_string(),
            arguments: serde_json::json!({"query": "test"}),
            result_preview: "[{\"id\":\"m1\"}]".to_string(),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("search_emails"));
    }
```

**Step 5: Add `tool_calls_made` to ChatResponse**

Add an optional field to `ChatMessage` to expose tool call metadata:

```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls_made: Option<Vec<crate::ai::tools::ToolCallRecord>>,
```

Set it in the response construction:
```rust
    let msg = ChatMessage {
        // ... existing fields ...
        tool_calls_made: if tool_records.is_empty() { None } else { Some(tool_records) },
    };
```

**Step 6: Run all tests**

Run: `~/.cargo/bin/cargo test -p iris-server -- --nocapture`
Expected: All PASS

**Step 7: Commit**

```bash
git add src/api/chat.rs
git commit -m "feat(v11-s2): agentic chat loop — LLM calls tools iteratively to answer questions"
```
