use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::message::MessageDetail;
use crate::AppState;

// ---------------------------------------------------------------------------
// POST /api/threads/{id}/summarize
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct SummarizeResponse {
    pub summary: String,
    pub cached: bool,
}

const SUMMARY_SYSTEM_PROMPT: &str = "You are an email thread summarizer. Given a thread of emails, produce a concise 2-4 sentence summary covering: the key topic or decision being discussed, the current status or outcome, and any action items or next steps. Be factual and concise. Do not use bullet points. Respond with plain text only.";

/// Build a summarization prompt from a list of thread messages.
pub fn build_summary_prompt(subject: &str, messages: &[MessageDetail]) -> String {
    let mut prompt = format!("Thread: {}\n\n", subject);
    let max_total = 3000;

    for (i, msg) in messages.iter().enumerate() {
        let from = msg.from_name.as_deref().unwrap_or(
            msg.from_address.as_deref().unwrap_or("Unknown"),
        );
        let date = msg
            .date
            .map(|d| {
                chrono::DateTime::from_timestamp(d, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        let body = msg.body_text.as_deref().unwrap_or("");
        let body_truncated: String = if body.chars().count() > 500 {
            let mut s: String = body.chars().take(500).collect();
            s.push_str("...");
            s
        } else {
            body.to_string()
        };

        let section = format!(
            "Message {} (from {}, {}):\n{}\n\n",
            i + 1,
            from,
            date,
            body_truncated
        );

        if prompt.len() + section.len() > max_total {
            prompt.push_str("...(remaining messages truncated)\n");
            break;
        }
        prompt.push_str(&section);
    }

    prompt
}

pub async fn summarize_thread(
    State(state): State<Arc<AppState>>,
    Path(thread_id): Path<String>,
) -> Result<Json<SummarizeResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let messages = MessageDetail::list_by_thread(&conn, &thread_id);
    if messages.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Check cache — use first message's ai_summary
    if let Some(ref cached_summary) = messages[0].ai_summary {
        if !cached_summary.is_empty() {
            return Ok(Json(SummarizeResponse {
                summary: cached_summary.clone(),
                cached: true,
            }));
        }
    }

    // Check AI is enabled
    let ai_enabled = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'ai_enabled'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "false".to_string());

    if ai_enabled != "true" || !state.providers.has_providers() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    let subject = messages[0].subject.as_deref().unwrap_or("(no subject)");
    let prompt = build_summary_prompt(subject, &messages);

    let summary = state
        .providers
        .generate(&prompt, Some(SUMMARY_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    // Cache the summary in the first message
    let first_id = &messages[0].id;
    conn.execute(
        "UPDATE messages SET ai_summary = ?2, updated_at = unixepoch() WHERE id = ?1",
        rusqlite::params![first_id, summary],
    )
    .ok();

    Ok(Json(SummarizeResponse {
        summary,
        cached: false,
    }))
}

// ---------------------------------------------------------------------------
// POST /api/ai/assist
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AiAssistRequest {
    pub action: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct AiAssistResponse {
    pub result: String,
}

/// Get the system prompt for a given AI assist action.
pub fn get_assist_system_prompt(action: &str) -> Option<&'static str> {
    match action {
        "rewrite" => Some("Rewrite the following text to be clearer and more professional. Preserve the original meaning. Return only the rewritten text, no explanation."),
        "formal" => Some("Rewrite the following text in a formal, professional tone. Return only the rewritten text."),
        "casual" => Some("Rewrite the following text in a casual, friendly tone. Return only the rewritten text."),
        "shorter" => Some("Condense the following text to be more concise while preserving key points. Return only the shortened text."),
        "longer" => Some("Expand the following text with more detail and elaboration. Return only the expanded text."),
        _ => None,
    }
}

pub async fn ai_assist(
    State(state): State<Arc<AppState>>,
    Json(input): Json<AiAssistRequest>,
) -> Result<Json<AiAssistResponse>, StatusCode> {
    // Cap content length to prevent abuse
    if input.content.len() > 50_000 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let system_prompt = get_assist_system_prompt(&input.action)
        .ok_or(StatusCode::BAD_REQUEST)?;

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let ai_enabled = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'ai_enabled'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "false".to_string());

    if ai_enabled != "true" || !state.providers.has_providers() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    let result = state
        .providers
        .generate(&input.content, Some(system_prompt))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    Ok(Json(AiAssistResponse { result }))
}

// ---------------------------------------------------------------------------
// POST /api/ai/extract-tasks
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ExtractTasksRequest {
    pub message_id: Option<String>,
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedTask {
    pub task: String,
    pub priority: String,
    pub deadline: Option<String>,
    pub source_subject: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExtractTasksResponse {
    pub tasks: Vec<ExtractedTask>,
}

const EXTRACT_TASKS_SYSTEM_PROMPT: &str = "Extract action items from the email(s). Return ONLY valid JSON array. Each element: {\"task\": \"description\", \"priority\": \"high|medium|low\", \"deadline\": \"date or null\", \"source_subject\": \"email subject\"}. If there are no action items, return an empty array []. Do not include any text outside the JSON array.";

/// Build a prompt for task extraction from one or more messages.
pub fn build_extract_tasks_prompt(messages: &[MessageDetail]) -> String {
    let mut prompt = String::from("Extract all action items, tasks, and to-dos from the following email(s):\n\n");
    let max_total = 4000;

    for (i, msg) in messages.iter().enumerate() {
        let from = msg.from_name.as_deref().unwrap_or(
            msg.from_address.as_deref().unwrap_or("Unknown"),
        );
        let subject = msg.subject.as_deref().unwrap_or("(no subject)");
        let date = msg
            .date
            .map(|d| {
                chrono::DateTime::from_timestamp(d, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        let body = msg.body_text.as_deref().unwrap_or("");
        let body_truncated: String = if body.chars().count() > 800 {
            let mut s: String = body.chars().take(800).collect();
            s.push_str("...");
            s
        } else {
            body.to_string()
        };

        let section = format!(
            "--- Email {} ---\nFrom: {}\nSubject: {}\nDate: {}\n\n{}\n\n",
            i + 1,
            from,
            subject,
            date,
            body_truncated
        );

        if prompt.len() + section.len() > max_total {
            prompt.push_str("...(remaining emails truncated)\n");
            break;
        }
        prompt.push_str(&section);
    }

    prompt
}

/// Parse AI response into extracted tasks, handling malformed JSON gracefully.
pub fn parse_extracted_tasks(response: &str) -> Vec<ExtractedTask> {
    let trimmed = response.trim();

    // Try to find a JSON array in the response
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            let json_str = &trimmed[start..=end];
            if let Ok(tasks) = serde_json::from_str::<Vec<ExtractedTask>>(json_str) {
                return tasks;
            }
        }
    }

    // If JSON parsing fails, return empty
    Vec::new()
}

pub async fn extract_tasks(
    State(state): State<Arc<AppState>>,
    Json(input): Json<ExtractTasksRequest>,
) -> Result<Json<ExtractTasksResponse>, StatusCode> {
    // Validate: at least one ID must be provided
    if input.message_id.is_none() && input.thread_id.is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Check AI is enabled
    let ai_enabled = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'ai_enabled'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "false".to_string());

    if ai_enabled != "true" || !state.providers.has_providers() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    // Fetch messages
    let messages = if let Some(ref thread_id) = input.thread_id {
        let msgs = MessageDetail::list_by_thread(&conn, thread_id);
        if msgs.is_empty() {
            return Err(StatusCode::NOT_FOUND);
        }
        msgs
    } else if let Some(ref message_id) = input.message_id {
        let msg = MessageDetail::get_by_id(&conn, message_id)
            .ok_or(StatusCode::NOT_FOUND)?;
        vec![msg]
    } else {
        return Err(StatusCode::BAD_REQUEST);
    };

    let prompt = build_extract_tasks_prompt(&messages);

    let ai_response = state
        .providers
        .generate(&prompt, Some(EXTRACT_TASKS_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let tasks = parse_extracted_tasks(&ai_response);

    Ok(Json(ExtractTasksResponse { tasks }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_summary_prompt_single_message() {
        let msg = MessageDetail {
            id: "m1".to_string(),
            message_id: None,
            account_id: "a1".to_string(),
            thread_id: Some("t1".to_string()),
            folder: "INBOX".to_string(),
            from_address: Some("alice@example.com".to_string()),
            from_name: Some("Alice".to_string()),
            to_addresses: None,
            cc_addresses: None,
            subject: Some("Project Update".to_string()),
            snippet: None,
            date: Some(1709500800), // 2024-03-04
            body_text: Some("The project is on track.".to_string()),
            body_html: None,
            is_read: true,
            is_starred: false,
            has_attachments: false,
            attachments: vec![],
            ai_intent: None,
            ai_priority_score: None,
            ai_priority_label: None,
            ai_category: None,
            ai_summary: None,
        };

        let prompt = build_summary_prompt("Project Update", &[msg]);
        assert!(prompt.contains("Thread: Project Update"));
        assert!(prompt.contains("Message 1 (from Alice,"));
        assert!(prompt.contains("The project is on track."));
    }

    #[test]
    fn test_build_summary_prompt_truncates_long_body() {
        let long_body = "x".repeat(1000);
        let msg = MessageDetail {
            id: "m1".to_string(),
            message_id: None,
            account_id: "a1".to_string(),
            thread_id: Some("t1".to_string()),
            folder: "INBOX".to_string(),
            from_address: Some("bob@example.com".to_string()),
            from_name: None,
            to_addresses: None,
            cc_addresses: None,
            subject: None,
            snippet: None,
            date: None,
            body_text: Some(long_body),
            body_html: None,
            is_read: true,
            is_starred: false,
            has_attachments: false,
            attachments: vec![],
            ai_intent: None,
            ai_priority_score: None,
            ai_priority_label: None,
            ai_category: None,
            ai_summary: None,
        };

        let prompt = build_summary_prompt("Test", &[msg]);
        // Body should be truncated to 500 chars + "..."
        assert!(prompt.contains("..."));
        // Body should be truncated — prompt should contain "..." truncation marker
        // and not the full 1000 chars of body
        assert!(prompt.len() < 1000);
    }

    #[test]
    fn test_build_summary_prompt_caps_total_length() {
        let messages: Vec<MessageDetail> = (0..20)
            .map(|i| MessageDetail {
                id: format!("m{}", i),
                message_id: None,
                account_id: "a1".to_string(),
                thread_id: Some("t1".to_string()),
                folder: "INBOX".to_string(),
                from_address: Some(format!("user{}@example.com", i)),
                from_name: None,
                to_addresses: None,
                cc_addresses: None,
                subject: None,
                snippet: None,
                date: None,
                body_text: Some("a".repeat(400)),
                body_html: None,
                is_read: true,
                is_starred: false,
                has_attachments: false,
                attachments: vec![],
                ai_intent: None,
                ai_priority_score: None,
                ai_priority_label: None,
                ai_category: None,
                ai_summary: None,
            })
            .collect();

        let prompt = build_summary_prompt("Long Thread", &messages);
        assert!(prompt.len() <= 3100); // small buffer for truncation message
        assert!(prompt.contains("truncated"));
    }

    #[test]
    fn test_get_assist_prompt_rewrite() {
        let prompt = get_assist_system_prompt("rewrite").unwrap();
        assert!(prompt.contains("clearer"));
    }

    #[test]
    fn test_get_assist_prompt_formal() {
        let prompt = get_assist_system_prompt("formal").unwrap();
        assert!(prompt.contains("formal"));
    }

    #[test]
    fn test_get_assist_prompt_casual() {
        let prompt = get_assist_system_prompt("casual").unwrap();
        assert!(prompt.contains("casual"));
    }

    #[test]
    fn test_get_assist_prompt_shorter() {
        let prompt = get_assist_system_prompt("shorter").unwrap();
        assert!(prompt.contains("concise"));
    }

    #[test]
    fn test_get_assist_prompt_longer() {
        let prompt = get_assist_system_prompt("longer").unwrap();
        assert!(prompt.contains("Expand"));
    }

    #[test]
    fn test_get_assist_prompt_invalid_action() {
        assert!(get_assist_system_prompt("invalid").is_none());
        assert!(get_assist_system_prompt("").is_none());
    }

    // -----------------------------------------------------------------------
    // Task extraction tests
    // -----------------------------------------------------------------------

    fn make_test_message(id: &str, subject: &str, body: &str, from: &str) -> MessageDetail {
        MessageDetail {
            id: id.to_string(),
            message_id: None,
            account_id: "a1".to_string(),
            thread_id: Some("t1".to_string()),
            folder: "INBOX".to_string(),
            from_address: Some(format!("{from}@example.com")),
            from_name: Some(from.to_string()),
            to_addresses: None,
            cc_addresses: None,
            subject: Some(subject.to_string()),
            snippet: None,
            date: Some(1709500800),
            body_text: Some(body.to_string()),
            body_html: None,
            is_read: true,
            is_starred: false,
            has_attachments: false,
            attachments: vec![],
            ai_intent: None,
            ai_priority_score: None,
            ai_priority_label: None,
            ai_category: None,
            ai_summary: None,
        }
    }

    #[test]
    fn test_build_extract_tasks_prompt_single() {
        let msg = make_test_message("m1", "Project Update", "Please review the PR by Friday.", "Alice");
        let prompt = build_extract_tasks_prompt(&[msg]);
        assert!(prompt.contains("Extract all action items"));
        assert!(prompt.contains("Email 1"));
        assert!(prompt.contains("Alice"));
        assert!(prompt.contains("Project Update"));
        assert!(prompt.contains("review the PR by Friday"));
    }

    #[test]
    fn test_build_extract_tasks_prompt_multiple() {
        let msg1 = make_test_message("m1", "Meeting", "Schedule the meeting.", "Alice");
        let msg2 = make_test_message("m2", "Review", "Complete code review.", "Bob");
        let prompt = build_extract_tasks_prompt(&[msg1, msg2]);
        assert!(prompt.contains("Email 1"));
        assert!(prompt.contains("Email 2"));
        assert!(prompt.contains("Alice"));
        assert!(prompt.contains("Bob"));
    }

    #[test]
    fn test_build_extract_tasks_prompt_truncates() {
        let long_body = "x".repeat(2000);
        let msg = make_test_message("m1", "Long", &long_body, "Alice");
        let prompt = build_extract_tasks_prompt(&[msg]);
        // Body should be truncated at 800 chars
        assert!(prompt.contains("..."));
        assert!(prompt.len() < 2000);
    }

    #[test]
    fn test_parse_extracted_tasks_valid_json() {
        let response = r#"[{"task":"Review PR","priority":"high","deadline":"2024-03-15","source_subject":"Code Review"},{"task":"Update docs","priority":"low","deadline":null,"source_subject":"Docs Update"}]"#;
        let tasks = parse_extracted_tasks(response);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].task, "Review PR");
        assert_eq!(tasks[0].priority, "high");
        assert_eq!(tasks[0].deadline.as_deref(), Some("2024-03-15"));
        assert_eq!(tasks[0].source_subject.as_deref(), Some("Code Review"));
        assert_eq!(tasks[1].task, "Update docs");
        assert_eq!(tasks[1].priority, "low");
        assert!(tasks[1].deadline.is_none());
    }

    #[test]
    fn test_parse_extracted_tasks_with_surrounding_text() {
        let response = "Here are the tasks:\n[{\"task\":\"Send report\",\"priority\":\"medium\",\"deadline\":null,\"source_subject\":\"Weekly Report\"}]\nThat's all.";
        let tasks = parse_extracted_tasks(response);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].task, "Send report");
    }

    #[test]
    fn test_parse_extracted_tasks_empty_array() {
        let response = "[]";
        let tasks = parse_extracted_tasks(response);
        assert!(tasks.is_empty());
    }

    #[test]
    fn test_parse_extracted_tasks_malformed_json() {
        let response = "not json at all";
        let tasks = parse_extracted_tasks(response);
        assert!(tasks.is_empty());
    }

    #[test]
    fn test_parse_extracted_tasks_partial_json() {
        let response = "[{\"task\":\"incomplete";
        let tasks = parse_extracted_tasks(response);
        assert!(tasks.is_empty());
    }

    #[test]
    fn test_extract_tasks_system_prompt_exists() {
        assert!(EXTRACT_TASKS_SYSTEM_PROMPT.contains("action items"));
        assert!(EXTRACT_TASKS_SYSTEM_PROMPT.contains("JSON"));
    }
}
