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
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Citation {
    pub message_id: String,
    pub subject: Option<String>,
    pub from: Option<String>,
    pub snippet: String,
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

const CHAT_SYSTEM_PROMPT: &str = r#"You are Iris, an AI email assistant. You help users understand and manage their inbox through natural conversation.

You have access to the user's recent emails provided as context below. When answering:
- Reference specific emails by their [ID] markers when citing information
- Be concise and helpful
- If the user asks to perform an action (archive, delete, mark read, etc.), describe what you'd do and include ACTION_PROPOSAL at the end of your response in this exact format:
  ACTION_PROPOSAL:{"action":"archive","description":"Archive 3 emails from LinkedIn","message_ids":["id1","id2","id3"]}
- Valid actions: archive, delete, mark_read, mark_unread, star
- If you don't have enough context to answer, say so honestly
- For briefing requests, summarize the most important unread emails
- Do not make up information not present in the provided emails

## Email Composition
You can draft emails for the user. When they ask to "write an email to...", "draft a reply to...", "compose a message about...", include a COMPOSE_PROPOSAL at the end of your response in this exact format:
  COMPOSE_PROPOSAL:{"to":["recipient@example.com"],"cc":[],"subject":"Subject line","body":"<p>Email body in HTML</p>","reply_to_message_id":null,"tone":"formal"}
- Always confirm the recipients and subject before drafting
- Match the requested tone (formal by default, or casual/brief if asked)
- For replies, include the reply_to_message_id to get thread context
- The draft will be saved for user review before sending"#;

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
    let (history, fts_citations) = {
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
        let fts_citations = search_relevant_emails_fts(&conn, &input.message);

        (history, fts_citations)
    };

    // Phase 2: Semantic search via Memories (async)
    let semantic_results = state.memories.search(&input.message, 10, Some("iris/")).await;

    // Phase 3: Resolve semantic results to citations (needs DB)
    let citations = if !semantic_results.is_empty() {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut citations = Vec::new();
        for r in &semantic_results {
            let msg_id = r.source.rsplit('/').next().unwrap_or("").to_string();
            if msg_id.is_empty() {
                continue;
            }
            if let Ok(c) = conn.query_row(
                "SELECT subject, from_name, from_address FROM messages WHERE id = ?1",
                rusqlite::params![msg_id],
                |row| {
                    let from_name: Option<String> = row.get(1)?;
                    let from_addr: Option<String> = row.get(2)?;
                    Ok(Citation {
                        message_id: msg_id.clone(),
                        subject: row.get(0)?,
                        from: from_name.or(from_addr),
                        snippet: r.text.chars().take(100).collect(),
                    })
                },
            ) {
                citations.push(c);
            }
        }
        if citations.is_empty() { fts_citations } else { citations }
    } else {
        fts_citations
    };

    // Phase 3b: Load cross-session context from Memories
    let past_sessions = state.memories.search(&input.message, 3, Some("iris/chat/sessions/")).await;
    let user_prefs = state.memories.search("user email preferences", 1, Some("iris/user/preferences")).await;

    // Phase 4: Build prompt and call AI provider (async)
    let prompt = build_chat_prompt(&history, &citations, &input.message, &past_sessions, &user_prefs);

    let ai_response = state
        .providers
        .generate(&prompt, Some(CHAT_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

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
    {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let assistant_msg_id_inner = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content, citations, proposed_action) VALUES (?1, ?2, 'assistant', ?3, ?4, ?5)",
            rusqlite::params![assistant_msg_id_inner, input.session_id, clean_content, citations_json, action_json],
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
        id: uuid::Uuid::new_v4().to_string(),
        session_id: input.session_id,
        role: "assistant".to_string(),
        content: clean_content,
        citations: if referenced_citations.is_empty() { None } else { Some(referenced_citations) },
        proposed_action,
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

    // Map action to batch update
    let batch_action = match action.action.as_str() {
        "archive" | "delete" | "mark_read" | "mark_unread" | "star" => action.action.as_str(),
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    // Execute via the same batch update logic
    let placeholders: Vec<String> = (0..action.message_ids.len())
        .map(|i| format!("?{}", i + 2))
        .collect();

    let sql = match batch_action {
        "archive" => format!(
            "UPDATE messages SET folder = 'Archive', updated_at = unixepoch() WHERE id IN ({})",
            placeholders.join(",")
        ),
        "delete" => format!(
            "UPDATE messages SET folder = 'Trash', updated_at = unixepoch() WHERE id IN ({})",
            placeholders.join(",")
        ),
        "mark_read" => format!(
            "UPDATE messages SET is_read = 1, updated_at = unixepoch() WHERE id IN ({})",
            placeholders.join(",")
        ),
        "mark_unread" => format!(
            "UPDATE messages SET is_read = 0, updated_at = unixepoch() WHERE id IN ({})",
            placeholders.join(",")
        ),
        "star" => format!(
            "UPDATE messages SET is_starred = 1, updated_at = unixepoch() WHERE id IN ({})",
            placeholders.join(",")
        ),
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let params: Vec<Box<dyn rusqlite::types::ToSql>> = action
        .message_ids
        .iter()
        .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::types::ToSql>)
        .collect();

    let updated = conn
        .execute(&sql, rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
                snippet(fts_messages, -1, '', '', '...', 40) as snip
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
            prompt.push_str(&format!(
                "[{}] From: {} | Subject: {} | {}\n",
                &c.message_id[..8.min(c.message_id.len())],
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
        }];
        let prompt = build_chat_prompt(&[], &citations, "What's happening tomorrow?", &[], &[]);
        assert!(prompt.contains("=== Relevant Emails ==="));
        assert!(prompt.contains("[abcdef12]"));
        assert!(prompt.contains("Alice"));
        assert!(prompt.contains("Meeting tomorrow"));
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
        assert!(CHAT_SYSTEM_PROMPT.contains("Email Composition"));
        assert!(CHAT_SYSTEM_PROMPT.contains("COMPOSE_PROPOSAL"));
        assert!(CHAT_SYSTEM_PROMPT.contains("reply_to_message_id"));
        assert!(CHAT_SYSTEM_PROMPT.contains("draft"));
    }
}
