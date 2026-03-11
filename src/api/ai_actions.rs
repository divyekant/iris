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
// POST /api/ai/grammar-check
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct GrammarCheckRequest {
    pub content: String,
    pub subject: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct GrammarIssue {
    pub kind: String,
    pub description: String,
    pub suggestion: String,
}

#[derive(Debug, Serialize)]
pub struct GrammarCheckResponse {
    pub score: u8,
    pub tone: String,
    pub issues: Vec<GrammarIssue>,
    pub improved_content: Option<String>,
}

const GRAMMAR_SYSTEM_PROMPT: &str = r#"You are a professional email editor. Analyze the following email for grammar, spelling, tone, and clarity. Respond with ONLY a JSON object (no markdown, no code fences):
{"score": <0-100 quality score>, "tone": "<detected tone: formal, casual, mixed, aggressive, neutral, friendly, etc.>", "issues": [{"kind": "<grammar|spelling|tone|clarity|punctuation>", "description": "<what's wrong>", "suggestion": "<how to fix it>"}], "improved_content": "<full corrected email text>"}
Be constructive. Only flag real issues. If the email is perfect, return an empty issues array and a score of 100."#;

/// Build the grammar check prompt from the email content and optional subject.
pub fn build_grammar_prompt(content: &str, subject: Option<&str>) -> String {
    let mut prompt = String::new();
    if let Some(subj) = subject {
        prompt.push_str(&format!("Subject: {}\n\n", subj));
    }
    prompt.push_str(content);
    prompt
}

/// Parse a grammar check JSON response from the AI provider.
pub fn parse_grammar_response(raw: &str) -> Option<GrammarCheckResponse> {
    // Strip markdown code fences if present
    let cleaned = raw
        .trim()
        .strip_prefix("```json")
        .or_else(|| raw.trim().strip_prefix("```"))
        .unwrap_or(raw.trim());
    let cleaned = cleaned
        .strip_suffix("```")
        .unwrap_or(cleaned)
        .trim();

    #[derive(Deserialize)]
    struct RawResponse {
        score: u8,
        tone: String,
        issues: Vec<RawIssue>,
        improved_content: Option<String>,
    }

    #[derive(Deserialize)]
    struct RawIssue {
        kind: String,
        description: String,
        suggestion: String,
    }

    let parsed: RawResponse = serde_json::from_str(cleaned).ok()?;

    Some(GrammarCheckResponse {
        score: parsed.score.min(100),
        tone: parsed.tone,
        issues: parsed
            .issues
            .into_iter()
            .map(|i| GrammarIssue {
                kind: i.kind,
                description: i.description,
                suggestion: i.suggestion,
            })
            .collect(),
        improved_content: parsed.improved_content,
    })
}

pub async fn grammar_check(
    State(state): State<Arc<AppState>>,
    Json(input): Json<GrammarCheckRequest>,
) -> Result<Json<GrammarCheckResponse>, StatusCode> {
    // Validate content
    if input.content.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if input.content.len() > 50_000 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

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

    let prompt = build_grammar_prompt(&input.content, input.subject.as_deref());

    let raw = state
        .providers
        .generate(&prompt, Some(GRAMMAR_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let response = parse_grammar_response(&raw).ok_or(StatusCode::BAD_GATEWAY)?;

    Ok(Json(response))
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

    // Grammar check tests

    #[test]
    fn test_build_grammar_prompt_without_subject() {
        let prompt = build_grammar_prompt("Hello world", None);
        assert_eq!(prompt, "Hello world");
        assert!(!prompt.contains("Subject:"));
    }

    #[test]
    fn test_build_grammar_prompt_with_subject() {
        let prompt = build_grammar_prompt("Hello world", Some("Meeting Tomorrow"));
        assert!(prompt.contains("Subject: Meeting Tomorrow"));
        assert!(prompt.contains("Hello world"));
    }

    #[test]
    fn test_parse_grammar_response_with_issues() {
        let raw = r#"{"score": 72, "tone": "casual", "issues": [{"kind": "grammar", "description": "Subject-verb disagreement", "suggestion": "Use 'are' instead of 'is'"}], "improved_content": "They are ready."}"#;
        let res = parse_grammar_response(raw).unwrap();
        assert_eq!(res.score, 72);
        assert_eq!(res.tone, "casual");
        assert_eq!(res.issues.len(), 1);
        assert_eq!(res.issues[0].kind, "grammar");
        assert_eq!(res.issues[0].description, "Subject-verb disagreement");
        assert_eq!(res.issues[0].suggestion, "Use 'are' instead of 'is'");
        assert_eq!(res.improved_content.as_deref(), Some("They are ready."));
    }

    #[test]
    fn test_parse_grammar_response_perfect_email() {
        let raw = r#"{"score": 100, "tone": "formal", "issues": [], "improved_content": null}"#;
        let res = parse_grammar_response(raw).unwrap();
        assert_eq!(res.score, 100);
        assert_eq!(res.tone, "formal");
        assert!(res.issues.is_empty());
        assert!(res.improved_content.is_none());
    }

    #[test]
    fn test_parse_grammar_response_with_code_fences() {
        let raw = "```json\n{\"score\": 85, \"tone\": \"neutral\", \"issues\": [], \"improved_content\": \"Hello.\"}\n```";
        let res = parse_grammar_response(raw).unwrap();
        assert_eq!(res.score, 85);
        assert_eq!(res.tone, "neutral");
    }

    #[test]
    fn test_parse_grammar_response_invalid_json() {
        let raw = "This is not JSON";
        assert!(parse_grammar_response(raw).is_none());
    }

    #[test]
    fn test_parse_grammar_response_score_capped_at_100() {
        // Score > 100 should be capped
        let raw = r#"{"score": 255, "tone": "formal", "issues": [], "improved_content": null}"#;
        let res = parse_grammar_response(raw).unwrap();
        assert_eq!(res.score, 100);
    }

    #[test]
    fn test_parse_grammar_response_multiple_issues() {
        let raw = r#"{"score": 45, "tone": "aggressive", "issues": [
            {"kind": "tone", "description": "Aggressive language", "suggestion": "Soften the tone"},
            {"kind": "spelling", "description": "Misspelled word", "suggestion": "Use 'their' instead of 'there'"},
            {"kind": "punctuation", "description": "Missing period", "suggestion": "Add period at end"}
        ], "improved_content": "Fixed version."}"#;
        let res = parse_grammar_response(raw).unwrap();
        assert_eq!(res.score, 45);
        assert_eq!(res.issues.len(), 3);
        assert_eq!(res.issues[0].kind, "tone");
        assert_eq!(res.issues[1].kind, "spelling");
        assert_eq!(res.issues[2].kind, "punctuation");
    }
}
