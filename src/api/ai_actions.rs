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
// POST /api/ai/multi-reply
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MultiReplyRequest {
    pub thread_id: String,
    pub message_id: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReplyOption {
    pub tone: String,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Serialize)]
pub struct MultiReplyResponse {
    pub options: Vec<ReplyOption>,
}

const MULTI_REPLY_SYSTEM_PROMPT: &str = "Return ONLY a valid JSON array with no surrounding text. Each element must have \"tone\" (one of \"formal\", \"casual\", \"brief\"), \"subject\" (reply subject line), and \"body\" (the email body text). Generate exactly 3 options: one formal (professional, complete sentences), one casual (friendly, conversational), and one brief (1-3 sentences max, direct).";

/// Build the prompt for multi-reply generation from thread messages.
pub fn build_multi_reply_prompt(
    subject: &str,
    messages: &[MessageDetail],
    context: Option<&str>,
) -> String {
    let mut prompt = String::from("Generate 3 reply options (formal, casual, brief) for the following email thread.\n\n");

    if let Some(ctx) = context {
        prompt.push_str(&format!("User's intent/instruction: {}\n\n", ctx));
    }

    prompt.push_str(&format!("Thread subject: {}\n\n", subject));

    // Include up to the most recent 5 messages for context
    let recent: Vec<&MessageDetail> = messages.iter().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect();
    let max_total = 3000;

    for (i, msg) in recent.iter().enumerate() {
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

    prompt.push_str("Reply to the most recent message. Generate a reply subject and body for each of the 3 tones.");

    prompt
}

/// Parse the AI response into reply options. Tries JSON array first, then
/// best-effort extraction.
pub fn parse_multi_reply_response(raw: &str) -> Option<Vec<ReplyOption>> {
    // Try to extract JSON array from the response (may be wrapped in markdown code blocks)
    let trimmed = raw.trim();
    let json_str = if trimmed.starts_with("```") {
        // Strip markdown code fences
        trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        trimmed
    };

    // Try direct JSON parse
    if let Ok(options) = serde_json::from_str::<Vec<ReplyOption>>(json_str) {
        if options.len() == 3 {
            return Some(options);
        }
        if !options.is_empty() {
            return Some(options);
        }
    }

    // Try to find a JSON array within the text
    if let Some(start) = json_str.find('[') {
        if let Some(end) = json_str.rfind(']') {
            let slice = &json_str[start..=end];
            if let Ok(options) = serde_json::from_str::<Vec<ReplyOption>>(slice) {
                if !options.is_empty() {
                    return Some(options);
                }
            }
        }
    }

    None
}

pub async fn multi_reply(
    State(state): State<Arc<AppState>>,
    Json(input): Json<MultiReplyRequest>,
) -> Result<Json<MultiReplyResponse>, StatusCode> {
    // Validate thread_id
    if input.thread_id.trim().is_empty() {
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

    // Look up thread messages
    let messages = MessageDetail::list_by_thread(&conn, &input.thread_id);
    if messages.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    let subject = messages.last()
        .and_then(|m| m.subject.as_deref())
        .unwrap_or("(no subject)");

    let prompt = build_multi_reply_prompt(
        subject,
        &messages,
        input.context.as_deref(),
    );

    let raw = state
        .providers
        .generate(&prompt, Some(MULTI_REPLY_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let options = parse_multi_reply_response(&raw)
        .ok_or(StatusCode::BAD_GATEWAY)?;

    Ok(Json(MultiReplyResponse { options }))
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

    // ---- Multi-reply tests ----

    #[test]
    fn test_build_multi_reply_prompt_basic() {
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
            subject: Some("Meeting Tomorrow".to_string()),
            snippet: None,
            date: Some(1709500800),
            body_text: Some("Can we meet at 3pm?".to_string()),
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

        let prompt = build_multi_reply_prompt("Meeting Tomorrow", &[msg], None);
        assert!(prompt.contains("Meeting Tomorrow"));
        assert!(prompt.contains("Can we meet at 3pm?"));
        assert!(prompt.contains("3 reply options"));
    }

    #[test]
    fn test_build_multi_reply_prompt_with_context() {
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
            subject: Some("Request".to_string()),
            snippet: None,
            date: None,
            body_text: Some("Please review.".to_string()),
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

        let prompt = build_multi_reply_prompt("Request", &[msg], Some("Decline politely"));
        assert!(prompt.contains("Decline politely"));
    }

    #[test]
    fn test_parse_multi_reply_response_valid_json() {
        let json = r#"[
            {"tone": "formal", "subject": "Re: Hello", "body": "Dear Sir, ..."},
            {"tone": "casual", "subject": "Re: Hello", "body": "Hey! ..."},
            {"tone": "brief", "subject": "Re: Hello", "body": "Got it."}
        ]"#;
        let result = parse_multi_reply_response(json);
        assert!(result.is_some());
        let options = result.unwrap();
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].tone, "formal");
        assert_eq!(options[1].tone, "casual");
        assert_eq!(options[2].tone, "brief");
    }

    #[test]
    fn test_parse_multi_reply_response_markdown_wrapped() {
        let json = "```json\n[\n{\"tone\": \"formal\", \"subject\": \"Re: Test\", \"body\": \"Formal reply\"}, {\"tone\": \"casual\", \"subject\": \"Re: Test\", \"body\": \"Casual reply\"}, {\"tone\": \"brief\", \"subject\": \"Re: Test\", \"body\": \"Brief reply\"}\n]\n```";
        let result = parse_multi_reply_response(json);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn test_parse_multi_reply_response_embedded_json() {
        let raw = "Here are your reply options:\n[{\"tone\": \"formal\", \"subject\": \"Re: X\", \"body\": \"A\"}, {\"tone\": \"casual\", \"subject\": \"Re: X\", \"body\": \"B\"}, {\"tone\": \"brief\", \"subject\": \"Re: X\", \"body\": \"C\"}]\nHope these help!";
        let result = parse_multi_reply_response(raw);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn test_parse_multi_reply_response_malformed() {
        let bad = "I can't generate replies right now.";
        assert!(parse_multi_reply_response(bad).is_none());
    }

    #[test]
    fn test_parse_multi_reply_response_empty_array() {
        let empty = "[]";
        assert!(parse_multi_reply_response(empty).is_none());
    }
}
