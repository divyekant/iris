use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub citations: Option<Vec<Citation>>,
    pub proposed_action: Option<ProposedAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls_made: Option<Vec<crate::ai::tools::ToolCallRecord>>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Citation {
    pub message_id: String,
    pub subject: Option<String>,
    pub from: Option<String>,
    pub snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_read: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProposedAction {
    pub action: String,
    pub description: String,
    pub message_ids: Vec<String>,
    /// Extra structured data for tool-specific proposals (e.g. compose_email).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub session_id: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub message: ChatMessage,
}

#[derive(Debug, Deserialize)]
pub struct ConfirmActionRequest {
    pub session_id: String,
    pub message_id: String,
}

#[derive(Debug, Serialize)]
pub struct ConfirmActionResponse {
    pub executed: bool,
    pub updated: usize,
    /// Draft ID returned when confirming a compose_email action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<String>,
}

// ---------------------------------------------------------------------------
// System prompt
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn chat_system_prompt() -> String {
    chat_system_prompt_with_stats_str("")
}

fn chat_system_prompt_with_stats(conn: &rusqlite::Connection) -> String {
    let stats_block = crate::api::inbox_stats::format_stats_for_prompt(conn);
    chat_system_prompt_with_stats_str(&stats_block)
}

fn chat_system_prompt_with_stats_str(stats_block: &str) -> String {
    let today = chrono::Local::now().format("%A, %B %-d, %Y").to_string();
    format!(
        r#"You are Iris, an AI email assistant. You help users understand and manage their inbox through natural conversation.

Today's date is {today}.
{stats_block}
You have access to tools that let you search, list, read, and bulk-update the user's emails. Use them to find relevant information before answering. When answering:
- When you need information, use the available tools (search_emails, list_emails, read_email, inbox_stats)
- Use list_emails for browsing/filtering by metadata (date, sender, category, read status)
- Use search_emails for keyword searches with optional filters
- Read specific emails for full details
- Cite emails by referencing their ID from tool results
- Be concise and helpful
- Use the date and read/unread status to answer questions about "today's emails", "unread emails", "this week", etc.
- Use inbox_stats to answer aggregate questions like "how many emails", "who sends me the most", etc.

## Bulk Operations
When a user asks to perform actions on multiple emails (e.g. "archive all emails from LinkedIn", "mark all newsletters as read", "delete old emails from noreply@example.com"):
1. First use list_emails or search_emails to find matching messages
2. Tell the user how many messages match and show a sample of up to 5 subject lines
3. ALWAYS include an ACTION_PROPOSAL so the user can confirm before execution
4. Only after the user confirms should you call bulk_update_emails
5. Available bulk actions: archive, mark_read, mark_unread, trash, star, unstar, move_to_category
6. Maximum 500 messages per bulk operation

## Action Proposals
- If the user asks to perform an action (archive, delete, mark read, bulk operations, etc.), describe what you'd do and include ACTION_PROPOSAL at the end of your response in this exact format:
  ACTION_PROPOSAL:{{"action":"archive","description":"Archive 3 emails from LinkedIn","message_ids":["id1","id2","id3"]}}
- Valid actions: archive, delete, mark_read, mark_unread, star, unstar, trash, move_to_category
- If you don't have enough context to answer, say so honestly
- For briefing requests, summarize the most important unread emails first
- Do not make up information not present in the provided emails

## Email Composition
You can draft emails for the user. When they ask to "write an email to...", "draft a reply to...", "compose a message about...", include a COMPOSE_PROPOSAL at the end of your response in this exact format:
  COMPOSE_PROPOSAL:{{"to":["recipient@example.com"],"cc":[],"subject":"Subject line","body":"<p>Email body in HTML</p>","reply_to_message_id":null,"tone":"formal"}}
- Always confirm the recipients and subject before drafting
- Match the requested tone (formal by default, or casual/brief if asked)
- For replies, include the reply_to_message_id to get thread context
- The draft will be saved for user review before sending"#
    )
}

// ---------------------------------------------------------------------------
// Agentic tool-use loop
// ---------------------------------------------------------------------------

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
                    if call.name == "bulk_update_emails" {
                        let deferred = build_bulk_update_confirmation(&conn, call, text.as_deref());
                        tool_records.push(ToolCallRecord {
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                            result_preview: "Deferred pending user confirmation".to_string(),
                        });
                        return Ok((deferred, all_citations, tool_records));
                    }

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

                    // Extract citations from search results (deduplicate by message_id)
                    if call.name == "search_emails" {
                        if let Ok(ref v) = result {
                            if let Some(arr) = v.as_array() {
                                for item in arr {
                                    if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                                        if !all_citations.iter().any(|c| c.message_id == id) {
                                            all_citations.push(Citation {
                                                message_id: id.to_string(),
                                                subject: item.get("subject").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                                from: item.get("from").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                                snippet: item.get("snippet").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                                date: None,
                                                is_read: item.get("is_read").and_then(|v| v.as_bool()),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if call.name == "read_email" {
                        if let Ok(ref v) = result {
                            if let Some(id) = v.get("id").and_then(|v| v.as_str()) {
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
                    if call.name == "list_emails" {
                        if let Ok(ref v) = result {
                            if let Some(emails) = v.get("emails").and_then(|v| v.as_array()) {
                                for item in emails {
                                    if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                                        if !all_citations.iter().any(|c| c.message_id == id) {
                                            all_citations.push(Citation {
                                                message_id: id.to_string(),
                                                subject: item.get("subject").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                                from: item.get("from").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                                snippet: item.get("snippet").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                                date: None,
                                                is_read: item.get("is_read").and_then(|v| v.as_bool()),
                                            });
                                        }
                                    }
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

    // Max iterations reached -- force a text response with no tools
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

fn build_bulk_update_confirmation(
    conn: &rusqlite::Connection,
    call: &crate::ai::tools::ToolCall,
    assistant_text: Option<&str>,
) -> String {
    let requested_ids: Vec<String> = call
        .arguments
        .get("message_ids")
        .and_then(|v| v.as_array())
        .map(|ids| {
            ids.iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default();

    let resolved_ids: Vec<String> = requested_ids
        .iter()
        .filter_map(|id| crate::utils::resolve_message_id(conn, id))
        .collect();

    let sample_subjects = sample_subjects_for_ids(conn, &resolved_ids, 5);
    let action = call
        .arguments
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("update");
    let category = call
        .arguments
        .get("category")
        .and_then(|v| v.as_str());

    let description = bulk_action_description(action, resolved_ids.len(), category);
    let proposed_action = ProposedAction {
        action: action.to_string(),
        description,
        message_ids: if resolved_ids.is_empty() {
            requested_ids.clone()
        } else {
            resolved_ids.clone()
        },
        data: None,
    };

    let mut content = assistant_text.unwrap_or("").trim().to_string();
    if content.is_empty() {
        content = format!(
            "I found {} email{} for this action.",
            resolved_ids.len(),
            if resolved_ids.len() == 1 { "" } else { "s" }
        );
    }

    if !sample_subjects.is_empty() {
        let subject_list = sample_subjects
            .iter()
            .map(|subject| format!("- {}", subject))
            .collect::<Vec<_>>()
            .join("\n");
        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str("Here are a few of the affected messages:\n");
        content.push_str(&subject_list);
    }

    if !content.to_lowercase().contains("confirm") {
        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str("Review the proposed action below and confirm before anything is changed.");
    }

    let action_json = serde_json::to_string(&proposed_action).unwrap_or_else(|_| "{}".to_string());
    format!("{content}\n\nACTION_PROPOSAL:{action_json}")
}

fn sample_subjects_for_ids(
    conn: &rusqlite::Connection,
    ids: &[String],
    limit: usize,
) -> Vec<String> {
    ids.iter()
        .take(limit)
        .filter_map(|id| {
            conn.query_row(
                "SELECT subject FROM messages WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
        })
        .collect()
}

fn bulk_action_description(action: &str, count: usize, category: Option<&str>) -> String {
    match action {
        "archive" => format!("Archive {count} email{}", if count == 1 { "" } else { "s" }),
        "delete" | "trash" => format!("Move {count} email{} to Trash", if count == 1 { "" } else { "s" }),
        "mark_read" => format!("Mark {count} email{} as read", if count == 1 { "" } else { "s" }),
        "mark_unread" => format!("Mark {count} email{} as unread", if count == 1 { "" } else { "s" }),
        "star" => format!("Star {count} email{}", if count == 1 { "" } else { "s" }),
        "unstar" => format!("Remove stars from {count} email{}", if count == 1 { "" } else { "s" }),
        "move_to_category" => format!(
            "Move {count} email{} to {}",
            if count == 1 { "" } else { "s" },
            category.unwrap_or("primary")
        ),
        other => format!("Apply '{other}' to {count} email{}", if count == 1 { "" } else { "s" }),
    }
}

// ---------------------------------------------------------------------------
// POST /api/ai/chat — send a message and get AI response
// ---------------------------------------------------------------------------

pub async fn chat(
    State(state): State<Arc<AppState>>,
    Json(input): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    // Validate session_id format (max 100 chars, alphanumeric + hyphens)
    if input.session_id.is_empty() || input.session_id.len() > 100 {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Cap message length to prevent abuse
    if input.message.len() > 50_000 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    // Phase 1: DB reads (no await across conn)
    let history = {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let ai_enabled = conn
            .query_row("SELECT value FROM config WHERE key = 'ai_enabled'", [], |row| row.get::<_, String>(0))
            .unwrap_or_else(|_| "false".to_string());

        if ai_enabled != "true" || !state.providers.has_providers() {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }

        // Store the user message
        let user_msg_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content) VALUES (?1, ?2, 'user', ?3)",
            rusqlite::params![user_msg_id, input.session_id, input.message],
        ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let history = load_history(&conn, &input.session_id, 10);
        history
    };

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

    let referenced_citations: Vec<Citation> = citations
        .iter()
        .filter(|c| ai_response.contains(&c.message_id[..8.min(c.message_id.len())]))
        .cloned()
        .collect();

    let citations_json = if referenced_citations.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&referenced_citations).ok())
    }.flatten();

    let action_json = proposed_action
        .as_ref()
        .and_then(|a| serde_json::to_string(a).ok());

    // Phase 5: Store assistant message and check for summary trigger (DB write, no await)
    let assistant_msg_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content, citations, proposed_action) VALUES (?1, ?2, 'assistant', ?3, ?4, ?5)",
            rusqlite::params![assistant_msg_id, input.session_id, clean_content, citations_json, action_json],
        ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // Trigger chat summarization every 10 messages
        let msg_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?1",
                rusqlite::params![input.session_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if msg_count > 0 && msg_count % 10 == 0 {
            crate::jobs::queue::enqueue_chat_summarize(&conn, &input.session_id);
        }
    }

    let msg = ChatMessage {
        id: assistant_msg_id.clone(),
        session_id: input.session_id,
        role: "assistant".to_string(),
        content: clean_content,
        citations: if referenced_citations.is_empty() { None } else { Some(referenced_citations) },
        proposed_action,
        tool_calls_made: if tool_records.is_empty() { None } else { Some(tool_records) },
        created_at: chrono::Utc::now().timestamp(),
    };

    Ok(Json(ChatResponse { message: msg }))
}

// ---------------------------------------------------------------------------
// GET /api/ai/chat/:session_id — get conversation history
// ---------------------------------------------------------------------------

pub async fn get_history(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<ChatMessage>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let messages = load_history(&conn, &session_id, 50);
    Ok(Json(messages))
}

// ---------------------------------------------------------------------------
// POST /api/ai/chat/confirm — execute a proposed action
// ---------------------------------------------------------------------------

pub async fn confirm_action(
    State(state): State<Arc<AppState>>,
    Json(input): Json<ConfirmActionRequest>,
) -> Result<Json<ConfirmActionResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Load the message with the proposed action
    let action_json: Option<String> = conn
        .query_row(
            "SELECT proposed_action FROM chat_messages WHERE id = ?1 AND session_id = ?2",
            rusqlite::params![input.message_id, input.session_id],
            |row| row.get(0),
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let action_json = action_json.ok_or(StatusCode::BAD_REQUEST)?;
    let action: ProposedAction =
        serde_json::from_str(&action_json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Handle compose_email action — save draft and return draft_id
    if action.action == "compose_email" {
        if let Some(ref data_val) = action.data {
            let compose_data: crate::ai::tools::ComposeEmailData =
                serde_json::from_value(data_val.clone())
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            // Find the first active account to associate the draft with
            let account_id: String = conn
                .query_row(
                    "SELECT id FROM accounts WHERE is_active = 1 ORDER BY created_at ASC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .map_err(|_| StatusCode::BAD_REQUEST)?;

            let draft_id = crate::ai::tools::execute_compose_email(&*conn, &compose_data, &account_id)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            return Ok(Json(ConfirmActionResponse {
                executed: true,
                updated: 1,
                draft_id: Some(draft_id),
            }));
        }
        return Err(StatusCode::BAD_REQUEST);
    }

    if action.message_ids.is_empty() {
        return Ok(Json(ConfirmActionResponse {
            executed: false,
            updated: 0,
            draft_id: None,
        }));
    }

    // Resolve truncated IDs (8-char prefixes from AI) to full UUIDs
    let resolved_ids: Vec<String> = action
        .message_ids
        .iter()
        .filter_map(|id| crate::utils::resolve_message_id(&conn, id))
        .collect();

    if resolved_ids.is_empty() {
        return Ok(Json(ConfirmActionResponse {
            executed: false,
            updated: 0,
            draft_id: None,
        }));
    }

    // Map action to batch update
    let batch_action = match action.action.as_str() {
        "archive" | "delete" | "mark_read" | "mark_unread" | "star" | "unstar" | "trash"
        | "move_to_category" => action.action.as_str(),
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let placeholders: Vec<String> = (0..resolved_ids.len())
        .map(|i| format!("?{}", i + 1))
        .collect();

    // For move_to_category, we need an extra parameter for the category
    let (sql, params): (String, Vec<rusqlite::types::Value>) = match batch_action {
        "archive" => (
            format!(
                "UPDATE OR IGNORE messages SET folder = 'Archive', updated_at = unixepoch() WHERE id IN ({})",
                placeholders.join(",")
            ),
            resolved_ids.iter().map(|id| rusqlite::types::Value::Text(id.clone())).collect(),
        ),
        "delete" | "trash" => (
            format!(
                "UPDATE OR IGNORE messages SET folder = 'Trash', updated_at = unixepoch() WHERE id IN ({})",
                placeholders.join(",")
            ),
            resolved_ids.iter().map(|id| rusqlite::types::Value::Text(id.clone())).collect(),
        ),
        "mark_read" => (
            format!(
                "UPDATE messages SET is_read = 1, updated_at = unixepoch() WHERE id IN ({})",
                placeholders.join(",")
            ),
            resolved_ids.iter().map(|id| rusqlite::types::Value::Text(id.clone())).collect(),
        ),
        "mark_unread" => (
            format!(
                "UPDATE messages SET is_read = 0, updated_at = unixepoch() WHERE id IN ({})",
                placeholders.join(",")
            ),
            resolved_ids.iter().map(|id| rusqlite::types::Value::Text(id.clone())).collect(),
        ),
        "star" => (
            format!(
                "UPDATE messages SET is_starred = 1, updated_at = unixepoch() WHERE id IN ({})",
                placeholders.join(",")
            ),
            resolved_ids.iter().map(|id| rusqlite::types::Value::Text(id.clone())).collect(),
        ),
        "unstar" => (
            format!(
                "UPDATE messages SET is_starred = 0, updated_at = unixepoch() WHERE id IN ({})",
                placeholders.join(",")
            ),
            resolved_ids.iter().map(|id| rusqlite::types::Value::Text(id.clone())).collect(),
        ),
        "move_to_category" => {
            // Extract category from the description (format: "Move N emails to <category>")
            // Default to "primary" if not found
            let category = action
                .description
                .rsplit(" to ")
                .next()
                .unwrap_or("primary")
                .trim()
                .to_lowercase();
            let cat_placeholders: Vec<String> = (0..resolved_ids.len())
                .map(|i| format!("?{}", i + 2))
                .collect();
            let mut p: Vec<rusqlite::types::Value> = Vec::new();
            p.push(rusqlite::types::Value::Text(category));
            for id in &resolved_ids {
                p.push(rusqlite::types::Value::Text(id.clone()));
            }
            (
                format!(
                    "UPDATE messages SET ai_category = ?1, updated_at = unixepoch() WHERE id IN ({})",
                    cat_placeholders.join(",")
                ),
                p,
            )
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let updated = conn
        .execute(&sql, rusqlite::params_from_iter(params.iter()))
        .map_err(|e| {
            tracing::error!("confirm_action execute failed: {:?} sql={}", e, sql);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(ConfirmActionResponse {
        executed: true,
        updated,
        draft_id: None,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/ai/chat/memory — return stored chat summaries and preferences
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ChatMemoryResponse {
    pub summaries: Vec<MemoryEntry>,
    pub preferences: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MemoryEntry {
    pub source: String,
    pub text: String,
    pub score: f64,
}

pub async fn get_chat_memory(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ChatMemoryResponse>, StatusCode> {
    let summaries = state
        .memories
        .search("chat conversation summary", 10, Some("iris/chat/sessions/"))
        .await;
    let prefs = state
        .memories
        .search("user email preferences", 1, Some("iris/user/preferences"))
        .await;

    Ok(Json(ChatMemoryResponse {
        summaries: summaries
            .into_iter()
            .map(|r| MemoryEntry {
                source: r.source,
                text: r.text,
                score: r.score,
            })
            .collect(),
        preferences: prefs.first().map(|p| p.text.clone()),
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_history(conn: &rusqlite::Connection, session_id: &str, limit: usize) -> Vec<ChatMessage> {
    let mut stmt = match conn.prepare(
        "SELECT id, session_id, role, content, citations, proposed_action, created_at
         FROM chat_messages
         WHERE session_id = ?1
         ORDER BY created_at ASC
         LIMIT ?2",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to prepare chat history query: {e}");
            return Vec::new();
        }
    };

    match stmt.query_map(rusqlite::params![session_id, limit as i64], |row| {
        let citations_json: Option<String> = row.get(4)?;
        let action_json: Option<String> = row.get(5)?;

        Ok(ChatMessage {
            id: row.get(0)?,
            session_id: row.get(1)?,
            role: row.get(2)?,
            content: row.get(3)?,
            citations: citations_json.and_then(|s| serde_json::from_str(&s).ok()),
            proposed_action: action_json.and_then(|s| serde_json::from_str(&s).ok()),
            tool_calls_made: None,
            created_at: row.get(6)?,
        })
    }) {
        Ok(rows) => rows
            .filter_map(|r| r.map_err(|e| tracing::warn!("Chat history row skip: {e}")).ok())
            .collect(),
        Err(e) => {
            tracing::error!("Failed to query chat history: {e}");
            Vec::new()
        }
    }
}

#[allow(dead_code)]
fn search_relevant_emails_fts(conn: &rusqlite::Connection, query: &str) -> Vec<Citation> {
    // Sanitize query for FTS5 — wrap each word in quotes
    let terms: Vec<String> = query
        .split_whitespace()
        .filter(|w| w.len() > 2) // skip short words
        .take(5) // limit terms
        .map(|w| format!("\"{}\"", w.replace('"', "")))
        .collect();

    if terms.is_empty() {
        return Vec::new();
    }

    let fts_query = terms.join(" OR ");

    let mut stmt = match conn.prepare(
        "SELECT m.id, m.subject, m.from_name, m.from_address,
                snippet(fts_messages, -1, '', '', '...', 40) as snip,
                m.date, m.is_read
         FROM fts_messages fts
         JOIN messages m ON m.rowid = fts.rowid
         WHERE fts_messages MATCH ?1
         ORDER BY rank
         LIMIT 10",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to prepare FTS search: {e}");
            return Vec::new();
        }
    };

    match stmt.query_map(rusqlite::params![fts_query], |row| {
        let from_name: Option<String> = row.get(2)?;
        let from_addr: Option<String> = row.get(3)?;
        Ok(Citation {
            message_id: row.get(0)?,
            subject: row.get(1)?,
            from: from_name.or(from_addr),
            snippet: row.get::<_, String>(4).unwrap_or_default(),
            date: row.get(5)?,
            is_read: row.get::<_, Option<i32>>(6)?.map(|v| v != 0),
        })
    }) {
        Ok(rows) => rows
            .filter_map(|r| r.map_err(|e| tracing::warn!("FTS row skip: {e}")).ok())
            .collect(),
        Err(e) => {
            tracing::error!("Failed to execute FTS search: {e}");
            Vec::new()
        }
    }
}

#[allow(dead_code)]
fn build_chat_prompt(
    history: &[ChatMessage],
    citations: &[Citation],
    current_message: &str,
    past_sessions: &[crate::ai::memories::MemoryResult],
    user_prefs: &[crate::ai::memories::MemoryResult],
) -> String {
    let mut prompt = String::new();

    // Add user preferences context
    if let Some(prefs) = user_prefs.first() {
        prompt.push_str("=== User Preferences ===\n");
        prompt.push_str(&prefs.text);
        prompt.push_str("\n\n");
    }

    // Add past conversation summaries
    if !past_sessions.is_empty() {
        prompt.push_str("=== Past Conversations ===\n");
        for s in past_sessions {
            prompt.push_str(&format!("- {}\n", s.text));
        }
        prompt.push_str("\n");
    }

    // Add email context
    if !citations.is_empty() {
        prompt.push_str("=== Relevant Emails ===\n");
        for c in citations {
            let from = c.from.as_deref().unwrap_or("Unknown");
            let subject = c.subject.as_deref().unwrap_or("(no subject)");
            let date_str = c.date
                .map(|ts| {
                    chrono::DateTime::from_timestamp(ts, 0)
                        .map(|dt| dt.with_timezone(&chrono::Local).format("%b %-d, %Y %H:%M").to_string())
                        .unwrap_or_else(|| "Unknown date".to_string())
                })
                .unwrap_or_else(|| "Unknown date".to_string());
            let read_status = match c.is_read {
                Some(true) => "read",
                Some(false) => "UNREAD",
                None => "unknown",
            };
            prompt.push_str(&format!(
                "[{}] {} | {} | From: {} | Subject: {} | {}\n",
                &c.message_id[..8.min(c.message_id.len())],
                date_str,
                read_status,
                from,
                subject,
                c.snippet
            ));
        }
        prompt.push_str("\n");
    }

    // Add recent conversation history
    if history.len() > 1 {
        prompt.push_str("=== Recent Conversation ===\n");
        // Skip the last message (it's the current user message we just stored)
        for msg in history.iter().rev().skip(1).take(6).collect::<Vec<_>>().into_iter().rev() {
            let role = if msg.role == "user" { "User" } else { "Iris" };
            prompt.push_str(&format!("{}: {}\n", role, msg.content));
        }
        prompt.push_str("\n");
    }

    prompt.push_str(&format!("User: {}\n\nIris:", current_message));
    prompt
}

/// Parse AI response to extract action proposals (ACTION_PROPOSAL or COMPOSE_PROPOSAL).
/// Returns (clean_content, optional_action).
pub fn parse_action_proposal(response: &str) -> (String, Option<ProposedAction>) {
    // Check for COMPOSE_PROPOSAL first (more specific)
    if let Some(idx) = response.find("COMPOSE_PROPOSAL:") {
        let clean = response[..idx].trim().to_string();
        let json_str = &response[idx + 17..];
        let json_str = json_str.trim();

        if let Some(start) = json_str.find('{') {
            if let Some(end) = json_str.rfind('}') {
                if let Ok(compose_data) = serde_json::from_str::<crate::ai::tools::ComposeEmailArgs>(&json_str[start..=end]) {
                    let recipients = compose_data.to.join(", ");
                    let description = format!(
                        "Draft email to {} with subject '{}'",
                        recipients, compose_data.subject
                    );
                    return (
                        clean,
                        Some(ProposedAction {
                            action: "compose_email".to_string(),
                            description,
                            message_ids: Vec::new(),
                            data: serde_json::to_value(&compose_data).ok(),
                        }),
                    );
                }
            }
        }
        (clean, None)
    } else if let Some(idx) = response.find("ACTION_PROPOSAL:") {
        let clean = response[..idx].trim().to_string();
        let json_str = &response[idx + 16..];
        let json_str = json_str.trim();

        // Try to extract JSON object
        if let Some(start) = json_str.find('{') {
            if let Some(end) = json_str.rfind('}') {
                if let Ok(action) = serde_json::from_str::<ProposedAction>(&json_str[start..=end]) {
                    return (clean, Some(action));
                }
            }
        }
        (clean, None)
    } else {
        (response.to_string(), None)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_action_proposal_none() {
        let (content, action) = parse_action_proposal("Here's a summary of your emails.");
        assert_eq!(content, "Here's a summary of your emails.");
        assert!(action.is_none());
    }

    #[test]
    fn test_parse_action_proposal_valid() {
        let response = "I'll archive those 3 emails for you.\n\nACTION_PROPOSAL:{\"action\":\"archive\",\"description\":\"Archive 3 LinkedIn emails\",\"message_ids\":[\"a1\",\"a2\",\"a3\"]}";
        let (content, action) = parse_action_proposal(response);
        assert_eq!(content, "I'll archive those 3 emails for you.");
        let action = action.unwrap();
        assert_eq!(action.action, "archive");
        assert_eq!(action.message_ids.len(), 3);
    }

    #[test]
    fn test_parse_action_proposal_malformed() {
        let response = "I'll do that.\nACTION_PROPOSAL:not json";
        let (content, action) = parse_action_proposal(response);
        assert_eq!(content, "I'll do that.");
        assert!(action.is_none());
    }

    #[test]
    fn test_build_chat_prompt_with_citations() {
        let citations = vec![Citation {
            message_id: "abcdef1234".to_string(),
            subject: Some("Meeting tomorrow".to_string()),
            from: Some("Alice".to_string()),
            snippet: "Let's meet at 3pm".to_string(),
            date: Some(1741392000), // Mar 8, 2025
            is_read: Some(false),
        }];
        let prompt = build_chat_prompt(&[], &citations, "What's happening tomorrow?", &[], &[]);
        assert!(prompt.contains("=== Relevant Emails ==="));
        assert!(prompt.contains("[abcdef12]"));
        assert!(prompt.contains("Alice"));
        assert!(prompt.contains("Meeting tomorrow"));
        assert!(prompt.contains("UNREAD"));
        assert!(prompt.contains("What's happening tomorrow?"));
    }

    #[test]
    fn test_build_chat_prompt_no_citations() {
        let prompt = build_chat_prompt(&[], &[], "Hello", &[], &[]);
        assert!(!prompt.contains("=== Relevant Emails ==="));
        assert!(prompt.contains("User: Hello"));
    }

    #[test]
    fn test_build_chat_prompt_with_past_sessions() {
        use crate::ai::memories::MemoryResult;
        let past = vec![MemoryResult {
            id: 1,
            text: "User asked about quarterly reports".to_string(),
            source: "iris/chat/sessions/abc".to_string(),
            score: 0.9,
        }];
        let prompt = build_chat_prompt(&[], &[], "Hello", &past, &[]);
        assert!(prompt.contains("=== Past Conversations ==="));
        assert!(prompt.contains("quarterly reports"));
    }

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

    #[test]
    fn test_system_prompt_with_stats() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();

        // Insert test account and messages
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('a1', 'gmail', 'test@test.com')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, from_address, subject, date, is_read, is_starred, is_draft)
             VALUES ('m1', 'a1', 'x@y.com', 'Test', strftime('%s','now'), 0, 0, 0)",
            [],
        ).unwrap();

        crate::api::inbox_stats::compute_and_store(&conn, "a1").unwrap();

        let prompt = chat_system_prompt_with_stats(&conn);
        assert!(prompt.contains("Inbox Overview"));
        assert!(prompt.contains("Unread: 1"));
    }

    #[test]
    fn test_parse_compose_proposal_valid() {
        let response = "I'll draft that email for you.\n\nCOMPOSE_PROPOSAL:{\"to\":[\"alice@example.com\"],\"cc\":[],\"subject\":\"Meeting Tomorrow\",\"body\":\"<p>Hi Alice</p>\",\"reply_to_message_id\":null,\"tone\":\"formal\"}";
        let (content, action) = parse_action_proposal(response);
        assert_eq!(content, "I'll draft that email for you.");
        let action = action.unwrap();
        assert_eq!(action.action, "compose_email");
        assert!(action.description.contains("alice@example.com"));
        assert!(action.description.contains("Meeting Tomorrow"));
        assert!(action.data.is_some());
    }

    #[test]
    fn test_parse_compose_proposal_malformed() {
        let response = "Here it is.\nCOMPOSE_PROPOSAL:bad json";
        let (content, action) = parse_action_proposal(response);
        assert_eq!(content, "Here it is.");
        assert!(action.is_none());
    }

    #[test]
    fn test_system_prompt_contains_compose_section() {
        let prompt = chat_system_prompt();
        assert!(prompt.contains("Email Composition"));
        assert!(prompt.contains("COMPOSE_PROPOSAL"));
        assert!(prompt.contains("reply_to_message_id"));
        assert!(prompt.contains("draft"));
    }
}
