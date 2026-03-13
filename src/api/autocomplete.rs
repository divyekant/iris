use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::message::MessageDetail;
use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AutocompleteRequest {
    pub thread_id: Option<String>,
    pub partial_text: String,
    pub cursor_position: usize,
    pub compose_mode: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct AutocompleteSuggestion {
    pub text: String,
    pub full_sentence: String,
    pub confidence: f64,
}

#[derive(Debug, Serialize)]
pub struct AutocompleteResponse {
    pub suggestions: Vec<AutocompleteSuggestion>,
    pub debounce_ms: u32,
}

// ---------------------------------------------------------------------------
// System prompt
// ---------------------------------------------------------------------------

const AUTOCOMPLETE_SYSTEM_PROMPT: &str = r#"You are an email autocomplete assistant. Given the thread context and partial text being typed, suggest 3 natural completions.

Rules:
- Complete the current sentence/thought naturally
- Use context from the thread (names, topics, dates mentioned)
- Match the formality level of the thread
- Keep completions concise (5-15 words each)
- Don't repeat what's already typed

Respond with JSON only:
{"suggestions": [{"text": "completion text here", "confidence": 0.0-1.0}, ...]}"#;

// ---------------------------------------------------------------------------
// POST /api/ai/autocomplete
// ---------------------------------------------------------------------------

pub async fn autocomplete(
    State(state): State<Arc<AppState>>,
    Json(input): Json<AutocompleteRequest>,
) -> Result<Json<AutocompleteResponse>, StatusCode> {
    // Validate input
    if input.partial_text.is_empty() || input.partial_text.len() > 10_000 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let valid_modes = ["new", "reply", "reply_all", "forward"];
    if !valid_modes.contains(&input.compose_mode.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Check AI is enabled
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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

    // Fetch thread context (last 3 messages, truncated)
    let thread_context = if let Some(ref thread_id) = input.thread_id {
        let messages = MessageDetail::list_by_thread(&conn, thread_id);
        build_thread_context(&messages)
    } else {
        String::new()
    };

    // Build the user prompt
    let prompt = build_autocomplete_prompt(&thread_context, &input.partial_text, &input.compose_mode);

    let ai_response = state
        .providers
        .generate(&prompt, Some(AUTOCOMPLETE_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let suggestions = parse_suggestions(&ai_response, &input.partial_text);

    Ok(Json(AutocompleteResponse {
        suggestions,
        debounce_ms: 300,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build thread context from the last 3 messages, each truncated to 500 chars.
pub fn build_thread_context(messages: &[MessageDetail]) -> String {
    if messages.is_empty() {
        return String::new();
    }

    let mut context = String::from("Thread context:\n");
    // Take the last 3 messages (most recent)
    let recent: Vec<&MessageDetail> = messages.iter().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();

    for msg in recent {
        let from = msg
            .from_name
            .as_deref()
            .or(msg.from_address.as_deref())
            .unwrap_or("Unknown");
        let subject = msg.subject.as_deref().unwrap_or("(no subject)");
        let body = msg.body_text.as_deref().unwrap_or("");
        let body_truncated: String = if body.chars().count() > 500 {
            let mut s: String = body.chars().take(500).collect();
            s.push_str("...");
            s
        } else {
            body.to_string()
        };

        context.push_str(&format!(
            "From: {} | Subject: {}\n{}\n---\n",
            from, subject, body_truncated
        ));
    }

    context
}

/// Build the full prompt sent to the AI provider.
pub fn build_autocomplete_prompt(thread_context: &str, partial_text: &str, compose_mode: &str) -> String {
    let mut prompt = String::new();

    let mode_description = match compose_mode {
        "reply" => "replying to a message",
        "reply_all" => "replying to all recipients",
        "forward" => "forwarding a message",
        _ => "composing a new message",
    };

    prompt.push_str(&format!("The user is {}.\n\n", mode_description));

    if !thread_context.is_empty() {
        prompt.push_str(thread_context);
        prompt.push('\n');
    }

    prompt.push_str(&format!("Partial text being typed:\n\"{}\"\n\nSuggest 3 completions:", partial_text));

    prompt
}

/// Parse AI response into suggestions.
/// Expects JSON: {"suggestions": [{"text": "...", "confidence": 0.0-1.0}, ...]}
pub fn parse_suggestions(response: &str, partial_text: &str) -> Vec<AutocompleteSuggestion> {
    // Try to extract JSON from the response (AI may include extra text)
    let json_str = extract_json_object(response);
    let json_str = json_str.unwrap_or(response);

    #[derive(Deserialize)]
    struct RawResponse {
        suggestions: Vec<RawSuggestion>,
    }

    #[derive(Deserialize)]
    struct RawSuggestion {
        text: String,
        confidence: Option<f64>,
    }

    let parsed: Result<RawResponse, _> = serde_json::from_str(json_str);

    match parsed {
        Ok(raw) => {
            let mut suggestions: Vec<AutocompleteSuggestion> = raw
                .suggestions
                .into_iter()
                .filter(|s| !s.text.trim().is_empty())
                .take(3)
                .map(|s| {
                    let confidence = s.confidence.unwrap_or(0.5).clamp(0.0, 1.0);
                    let full_sentence = format!("{}{}", partial_text, s.text);
                    AutocompleteSuggestion {
                        text: s.text,
                        full_sentence,
                        confidence,
                    }
                })
                .collect();

            // Sort by confidence descending
            suggestions.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
            suggestions
        }
        Err(_) => Vec::new(),
    }
}

/// Extract a JSON object from a string that may contain surrounding text.
fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end >= start {
        Some(&s[start..=end])
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_autocomplete_prompt_with_thread_context() {
        let context = "Thread context:\nFrom: Alice | Subject: Meeting\nLet's meet at 3pm tomorrow.\n---\n";
        let prompt = build_autocomplete_prompt(context, "I wanted to follow up on", "reply");
        assert!(prompt.contains("replying to a message"));
        assert!(prompt.contains("Thread context:"));
        assert!(prompt.contains("Alice"));
        assert!(prompt.contains("I wanted to follow up on"));
        assert!(prompt.contains("Suggest 3 completions"));
    }

    #[test]
    fn test_build_autocomplete_prompt_new_compose() {
        let prompt = build_autocomplete_prompt("", "Hello, I am writing to", "new");
        assert!(prompt.contains("composing a new message"));
        assert!(!prompt.contains("Thread context"));
        assert!(prompt.contains("Hello, I am writing to"));
    }

    #[test]
    fn test_build_autocomplete_prompt_forward() {
        let prompt = build_autocomplete_prompt("", "Please see the attached", "forward");
        assert!(prompt.contains("forwarding a message"));
    }

    #[test]
    fn test_build_autocomplete_prompt_reply_all() {
        let prompt = build_autocomplete_prompt("", "Thanks everyone", "reply_all");
        assert!(prompt.contains("replying to all recipients"));
    }

    #[test]
    fn test_parse_suggestions_valid() {
        let response = r#"{"suggestions": [{"text": "the proposal you mentioned", "confidence": 0.88}, {"text": "our meeting last week", "confidence": 0.75}, {"text": "the budget review", "confidence": 0.60}]}"#;
        let suggestions = parse_suggestions(response, "I wanted to follow up on ");
        assert_eq!(suggestions.len(), 3);
        assert_eq!(suggestions[0].text, "the proposal you mentioned");
        assert_eq!(suggestions[0].confidence, 0.88);
        assert!(suggestions[0].full_sentence.contains("I wanted to follow up on the proposal you mentioned"));
        // Should be sorted by confidence descending
        assert!(suggestions[0].confidence >= suggestions[1].confidence);
        assert!(suggestions[1].confidence >= suggestions[2].confidence);
    }

    #[test]
    fn test_parse_suggestions_empty_response() {
        let response = r#"{"suggestions": []}"#;
        let suggestions = parse_suggestions(response, "Hello");
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_parse_suggestions_malformed_json() {
        let response = "This is not JSON at all";
        let suggestions = parse_suggestions(response, "Hello");
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_parse_suggestions_with_surrounding_text() {
        let response = r#"Here are the suggestions: {"suggestions": [{"text": "the project timeline", "confidence": 0.9}]} Hope that helps!"#;
        let suggestions = parse_suggestions(response, "Let me check ");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].text, "the project timeline");
        assert_eq!(suggestions[0].full_sentence, "Let me check the project timeline");
    }

    #[test]
    fn test_parse_suggestions_missing_confidence() {
        let response = r#"{"suggestions": [{"text": "some completion"}]}"#;
        let suggestions = parse_suggestions(response, "test ");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].confidence, 0.5); // default
    }

    #[test]
    fn test_parse_suggestions_caps_at_three() {
        let response = r#"{"suggestions": [{"text": "one", "confidence": 0.9}, {"text": "two", "confidence": 0.8}, {"text": "three", "confidence": 0.7}, {"text": "four", "confidence": 0.6}]}"#;
        let suggestions = parse_suggestions(response, "");
        assert_eq!(suggestions.len(), 3);
    }

    #[test]
    fn test_parse_suggestions_filters_empty() {
        let response = r#"{"suggestions": [{"text": "", "confidence": 0.9}, {"text": "valid completion", "confidence": 0.8}]}"#;
        let suggestions = parse_suggestions(response, "test ");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].text, "valid completion");
    }

    #[test]
    fn test_parse_suggestions_clamps_confidence() {
        let response = r#"{"suggestions": [{"text": "over", "confidence": 1.5}, {"text": "under", "confidence": -0.3}]}"#;
        let suggestions = parse_suggestions(response, "");
        assert_eq!(suggestions[0].confidence, 1.0);
        assert_eq!(suggestions[1].confidence, 0.0);
    }

    #[test]
    fn test_build_thread_context_empty() {
        let context = build_thread_context(&[]);
        assert!(context.is_empty());
    }

    #[test]
    fn test_build_thread_context_single_message() {
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
            subject: Some("Quarterly Review".to_string()),
            snippet: None,
            date: Some(1709500800),
            body_text: Some("Please review the Q4 numbers before our meeting.".to_string()),
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

        let context = build_thread_context(&[msg]);
        assert!(context.contains("Thread context:"));
        assert!(context.contains("Alice"));
        assert!(context.contains("Quarterly Review"));
        assert!(context.contains("Q4 numbers"));
    }

    #[test]
    fn test_build_thread_context_truncates_long_body() {
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

        let context = build_thread_context(&[msg]);
        // Body should be truncated to 500 chars + "..."
        assert!(context.contains("..."));
        // The context should not contain the full 1000-char body
        assert!(context.len() < 700);
    }

    #[test]
    fn test_build_thread_context_takes_last_three() {
        let messages: Vec<MessageDetail> = (0..5)
            .map(|i| MessageDetail {
                id: format!("m{}", i),
                message_id: None,
                account_id: "a1".to_string(),
                thread_id: Some("t1".to_string()),
                folder: "INBOX".to_string(),
                from_address: Some(format!("user{}@example.com", i)),
                from_name: Some(format!("User{}", i)),
                to_addresses: None,
                cc_addresses: None,
                subject: Some("Thread".to_string()),
                snippet: None,
                date: Some(1709500800 + i as i64 * 3600),
                body_text: Some(format!("Message body {}", i)),
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

        let context = build_thread_context(&messages);
        // Should contain the last 3 messages (User2, User3, User4)
        assert!(context.contains("User2"));
        assert!(context.contains("User3"));
        assert!(context.contains("User4"));
        // Should NOT contain the first 2 (User0, User1)
        assert!(!context.contains("User0"));
        assert!(!context.contains("User1"));
    }

    #[test]
    fn test_extract_json_object() {
        assert_eq!(
            extract_json_object(r#"text {"key": "val"} more"#),
            Some(r#"{"key": "val"}"#)
        );
        assert_eq!(extract_json_object("no json here"), None);
        assert_eq!(
            extract_json_object(r#"{"simple": true}"#),
            Some(r#"{"simple": true}"#)
        );
    }

    #[test]
    fn test_confidence_ordering() {
        let response = r#"{"suggestions": [{"text": "low", "confidence": 0.3}, {"text": "high", "confidence": 0.95}, {"text": "mid", "confidence": 0.6}]}"#;
        let suggestions = parse_suggestions(response, "");
        assert_eq!(suggestions[0].text, "high");
        assert_eq!(suggestions[1].text, "mid");
        assert_eq!(suggestions[2].text, "low");
    }
}
