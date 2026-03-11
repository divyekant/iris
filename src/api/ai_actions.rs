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
// POST /api/ai/suggest-subject
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SuggestSubjectRequest {
    pub body: String,
    pub current_subject: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SuggestSubjectResponse {
    pub suggestions: Vec<String>,
}

const SUGGEST_SUBJECT_SYSTEM_PROMPT: &str = "You are an email subject line assistant. Given the email body, suggest 3 concise, clear subject lines. Respond with ONLY a JSON array of 3 strings, no explanation or markdown.";

/// Build prompt for subject line suggestion.
pub fn build_suggest_subject_prompt(body: &str, current_subject: Option<&str>) -> String {
    let truncated: String = if body.chars().count() > 3000 {
        let mut s: String = body.chars().take(3000).collect();
        s.push_str("...");
        s
    } else {
        body.to_string()
    };

    let mut prompt = format!("Email body:\n{}\n", truncated);
    if let Some(subj) = current_subject {
        if !subj.is_empty() {
            prompt.push_str(&format!(
                "\nThe current subject is: '{}'. Improve it or suggest alternatives.",
                subj
            ));
        }
    }
    prompt
}

// ---------------------------------------------------------------------------
// POST /api/ai/draft-from-intent
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DraftFromIntentRequest {
    pub intent: String,
    pub context: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DraftFromIntentResponse {
    pub subject: String,
    pub body: String,
    pub suggested_to: Vec<String>,
}

const DRAFT_INTENT_SYSTEM_PROMPT: &str = "You are an email drafting assistant. Given the user's intent, generate a professional email. Respond with ONLY a JSON object (no markdown fences, no extra text):\n{\"subject\": \"...\", \"body\": \"...\", \"suggested_to\": [\"email@example.com\"]}\nIf recipients can be inferred from the intent, include them in suggested_to. Otherwise use an empty array.\nThe body should be a complete, polished email text ready to send. Do not include a subject line in the body.";

/// Build the user prompt for draft-from-intent, optionally including reply context.
pub fn build_draft_intent_prompt(intent: &str, context: Option<&str>) -> String {
    let mut prompt = format!("Draft an email for this intent: {}", intent);
    if let Some(ctx) = context {
        prompt.push_str(&format!("\n\nReply context:\n{}", ctx));
    }
    prompt
}

/// Parse LLM response into a vec of subject suggestions.
fn parse_subject_suggestions(raw: &str) -> Vec<String> {
    // Try JSON array first
    if let Ok(arr) = serde_json::from_str::<Vec<String>>(raw) {
        return arr.into_iter().filter(|s| !s.is_empty()).collect();
    }

    // Try to extract a JSON array from within the response (LLM may wrap with text)
    if let Some(start) = raw.find('[') {
        if let Some(end) = raw.rfind(']') {
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(&raw[start..=end]) {
                return arr.into_iter().filter(|s| !s.is_empty()).collect();
            }
        }
    }

    // Fallback: split by newlines, strip numbering
    raw.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| {
            // Strip leading "1. ", "2) ", "- ", etc.
            let stripped = l
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')' || c == '-')
                .trim_start_matches(|c: char| c == ' ' || c == '"')
                .trim_end_matches('"');
            stripped.to_string()
        })
        .filter(|s| !s.is_empty())
        .take(3)
        .collect()
}

pub async fn suggest_subject(
    State(state): State<Arc<AppState>>,
    Json(input): Json<SuggestSubjectRequest>,
) -> Result<Json<SuggestSubjectResponse>, StatusCode> {
    if input.body.len() > 50_000 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    if input.body.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
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

    let prompt = build_suggest_subject_prompt(&input.body, input.current_subject.as_deref());

    let raw = state
        .providers
        .generate(&prompt, Some(SUGGEST_SUBJECT_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let suggestions = parse_subject_suggestions(&raw);

    if suggestions.is_empty() {
        return Err(StatusCode::BAD_GATEWAY);
    }

    Ok(Json(SuggestSubjectResponse { suggestions }))
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

/// Parse the AI response JSON into a DraftFromIntentResponse, handling common issues.
pub fn parse_draft_response(raw: &str) -> Option<DraftFromIntentResponse> {
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

    // Try parsing as our response type
    if let Ok(resp) = serde_json::from_str::<DraftFromIntentResponse>(cleaned) {
        return Some(resp);
    }

    // Fallback: try to extract JSON object from the string
    if let Some(start) = cleaned.find('{') {
        if let Some(end) = cleaned.rfind('}') {
            let json_str = &cleaned[start..=end];
            if let Ok(resp) = serde_json::from_str::<DraftFromIntentResponse>(json_str) {
                return Some(resp);
            }
        }
    }

    None
}

pub async fn draft_from_intent(
    State(state): State<Arc<AppState>>,
    Json(input): Json<DraftFromIntentRequest>,
) -> Result<Json<DraftFromIntentResponse>, StatusCode> {
    // Validate intent
    let intent = input.intent.trim();
    if intent.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if intent.len() > 2000 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    // Check AI is enabled
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

    let prompt = build_draft_intent_prompt(intent, input.context.as_deref());

    let raw = state
        .providers
        .generate(&prompt, Some(DRAFT_INTENT_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let response = parse_draft_response(&raw).ok_or(StatusCode::BAD_GATEWAY)?;

    Ok(Json(response))
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
    if input.message_id.is_none() && input.thread_id.is_none() {
        return Err(StatusCode::BAD_REQUEST);
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

/// Parse the AI response into reply options.
pub fn parse_multi_reply_response(raw: &str) -> Option<Vec<ReplyOption>> {
    let trimmed = raw.trim();
    let json_str = if trimmed.starts_with("```") {
        trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        trimmed
    };

    if let Ok(options) = serde_json::from_str::<Vec<ReplyOption>>(json_str) {
        if !options.is_empty() {
            return Some(options);
        }
    }

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
    if input.thread_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
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
            ai_sentiment: None,
            ai_needs_reply: false,
            list_unsubscribe: None,
            list_unsubscribe_post: false,
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
            ai_sentiment: None,
            ai_needs_reply: false,
            list_unsubscribe: None,
            list_unsubscribe_post: false,
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
                ai_sentiment: None,
                ai_needs_reply: false,
                list_unsubscribe: None,
                list_unsubscribe_post: false,
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

    #[test]
    fn test_build_suggest_subject_prompt_without_current() {
        let prompt = build_suggest_subject_prompt("Hello, let's schedule a meeting.", None);
        assert!(prompt.contains("Email body:"));
        assert!(prompt.contains("Hello, let's schedule a meeting."));
        assert!(!prompt.contains("current subject"));
    }

    #[test]
    fn test_build_suggest_subject_prompt_with_current() {
        let prompt = build_suggest_subject_prompt(
            "Please review the attached Q3 report.",
            Some("stuff"),
        );
        assert!(prompt.contains("Email body:"));
        assert!(prompt.contains("current subject is: 'stuff'"));
    }

    #[test]
    fn test_build_suggest_subject_prompt_empty_current_ignored() {
        let prompt = build_suggest_subject_prompt("body text", Some(""));
        assert!(!prompt.contains("current subject"));
    }

    #[test]
    fn test_build_suggest_subject_prompt_truncates_long_body() {
        let long_body = "x".repeat(5000);
        let prompt = build_suggest_subject_prompt(&long_body, None);
        // Should be truncated to ~3000 chars + overhead
        assert!(prompt.len() < 3200);
        assert!(prompt.contains("..."));
    }

    #[test]
    fn test_parse_subject_suggestions_json_array() {
        let raw = r#"["Meeting Tomorrow", "Q3 Review", "Follow-up"]"#;
        let suggestions = parse_subject_suggestions(raw);
        assert_eq!(suggestions.len(), 3);
        assert_eq!(suggestions[0], "Meeting Tomorrow");
        assert_eq!(suggestions[1], "Q3 Review");
        assert_eq!(suggestions[2], "Follow-up");
    }

    #[test]
    fn test_parse_subject_suggestions_json_embedded() {
        let raw = r#"Here are some suggestions: ["Option A", "Option B", "Option C"] hope that helps!"#;
        let suggestions = parse_subject_suggestions(raw);
        assert_eq!(suggestions.len(), 3);
        assert_eq!(suggestions[0], "Option A");
    }

    #[test]
    fn test_parse_subject_suggestions_numbered_fallback() {
        let raw = "1. Meeting Tomorrow\n2. Q3 Review\n3. Follow-up";
        let suggestions = parse_subject_suggestions(raw);
        assert_eq!(suggestions.len(), 3);
        assert_eq!(suggestions[0], "Meeting Tomorrow");
        assert_eq!(suggestions[1], "Q3 Review");
        assert_eq!(suggestions[2], "Follow-up");
    }

    #[test]
    fn test_parse_subject_suggestions_filters_empty() {
        let raw = r#"["Good Subject", "", "Another"]"#;
        let suggestions = parse_subject_suggestions(raw);
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0], "Good Subject");
        assert_eq!(suggestions[1], "Another");
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

    // Draft-from-intent tests

    #[test]
    fn test_build_draft_intent_prompt_without_context() {
        let prompt = build_draft_intent_prompt("tell Bob I can't make Tuesday", None);
        assert!(prompt.contains("tell Bob I can't make Tuesday"));
        assert!(!prompt.contains("Reply context"));
    }

    #[test]
    fn test_build_draft_intent_prompt_with_context() {
        let prompt = build_draft_intent_prompt(
            "decline the meeting",
            Some("From: Bob\nSubject: Meeting Tuesday\nHey, can you make it?"),
        );
        assert!(prompt.contains("decline the meeting"));
        assert!(prompt.contains("Reply context:"));
        assert!(prompt.contains("From: Bob"));
    }

    #[test]
    fn test_parse_draft_response_valid_json() {
        let raw = r#"{"subject": "Meeting Reschedule", "body": "Hi Bob,\n\nI can't make Tuesday.", "suggested_to": ["bob@example.com"]}"#;
        let resp = parse_draft_response(raw).unwrap();
        assert_eq!(resp.subject, "Meeting Reschedule");
        assert!(resp.body.contains("I can't make Tuesday"));
        assert_eq!(resp.suggested_to, vec!["bob@example.com"]);
    }

    #[test]
    fn test_parse_draft_response_with_markdown_fences() {
        let raw = "```json\n{\"subject\": \"Hello\", \"body\": \"World\", \"suggested_to\": []}\n```";
        let resp = parse_draft_response(raw).unwrap();
        assert_eq!(resp.subject, "Hello");
        assert_eq!(resp.body, "World");
        assert!(resp.suggested_to.is_empty());
    }

    #[test]
    fn test_parse_draft_response_empty_suggested_to() {
        let raw = r#"{"subject": "Test", "body": "Body text", "suggested_to": []}"#;
        let resp = parse_draft_response(raw).unwrap();
        assert!(resp.suggested_to.is_empty());
    }

    #[test]
    fn test_parse_draft_response_with_preamble() {
        let raw = "Here is your draft:\n{\"subject\": \"Test\", \"body\": \"Hello\", \"suggested_to\": []}";
        let resp = parse_draft_response(raw).unwrap();
        assert_eq!(resp.subject, "Test");
    }

    #[test]
    fn test_parse_draft_response_invalid_json() {
        let raw = "This is not JSON at all";
        assert!(parse_draft_response(raw).is_none());
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
        assert!(parse_extracted_tasks("[]").is_empty());
    }

    #[test]
    fn test_parse_extracted_tasks_malformed_json() {
        assert!(parse_extracted_tasks("not json at all").is_empty());
    }

    #[test]
    fn test_parse_extracted_tasks_partial_json() {
        assert!(parse_extracted_tasks("[{\"task\":\"incomplete").is_empty());
    }

    #[test]
    fn test_extract_tasks_system_prompt_exists() {
        assert!(EXTRACT_TASKS_SYSTEM_PROMPT.contains("action items"));
        assert!(EXTRACT_TASKS_SYSTEM_PROMPT.contains("JSON"));
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
        assert!(parse_multi_reply_response("I can't generate replies right now.").is_none());
    }

    #[test]
    fn test_parse_multi_reply_response_empty_array() {
        assert!(parse_multi_reply_response("[]").is_none());
    }
}
