use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Valid intent categories for classification.
const VALID_INTENTS: &[&str] = &[
    "action_request",
    "question",
    "fyi",
    "scheduling",
    "sales",
    "social",
    "newsletter",
];

const INTENT_SYSTEM_PROMPT: &str = r#"You are an email intent classifier. Classify the email into exactly one of these categories:
- action_request: sender wants the recipient to do something specific
- question: sender is asking a question that needs an answer
- fyi: informational, no action needed
- scheduling: meeting requests, calendar invites, availability checks
- sales: sales pitches, product promotions, cold outreach
- social: personal messages, social invitations, networking
- newsletter: recurring newsletters, digests, subscriptions

Respond with JSON only: {"intent": "<category>", "confidence": <0.0-1.0>}"#;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DetectIntentRequest {
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentResponse {
    pub intent: String,
    pub confidence: f64,
}

// ---------------------------------------------------------------------------
// Prompt builder
// ---------------------------------------------------------------------------

/// Build the user prompt for intent classification from email fields.
pub fn build_intent_prompt(subject: &str, from: &str, body: &str) -> String {
    let body_truncated: String = body.chars().take(2000).collect();
    format!("From: {from}\nSubject: {subject}\n\n{body_truncated}")
}

/// Parse the AI response JSON into an IntentResponse.
/// Returns None if the response is unparseable or contains an invalid intent.
pub fn parse_intent_response(response: &str) -> Option<IntentResponse> {
    let json_str = extract_json(response);

    let parsed: IntentResponse = serde_json::from_str(json_str).ok()?;

    // Validate the intent is one of the known categories
    let normalized = parsed.intent.to_lowercase();
    if !VALID_INTENTS.contains(&normalized.as_str()) {
        tracing::warn!("Unknown intent category from AI: {}", parsed.intent);
        return None;
    }

    // Clamp confidence to 0.0-1.0
    let confidence = parsed.confidence.clamp(0.0, 1.0);

    Some(IntentResponse {
        intent: normalized,
        confidence,
    })
}

/// Extract JSON object from a response that may contain markdown code blocks.
fn extract_json(response: &str) -> &str {
    let trimmed = response.trim();

    // Check for ```json ... ``` blocks
    if let Some(start) = trimmed.find("```json") {
        let content = &trimmed[start + 7..];
        if let Some(end) = content.find("```") {
            return content[..end].trim();
        }
    }

    // Check for ``` ... ``` blocks
    if let Some(start) = trimmed.find("```") {
        let content = &trimmed[start + 3..];
        if let Some(end) = content.find("```") {
            return content[..end].trim();
        }
    }

    // Try to find raw JSON object
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return &trimmed[start..=end];
        }
    }

    trimmed
}

// ---------------------------------------------------------------------------
// GET /api/messages/{id}/intent
// ---------------------------------------------------------------------------

pub async fn get_intent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<IntentResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result: Result<(Option<String>, Option<f64>), _> = conn.query_row(
        "SELECT intent, intent_confidence FROM messages WHERE id = ?1 AND is_deleted = 0",
        rusqlite::params![id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    );

    match result {
        Ok((Some(intent), confidence)) => Ok(Json(IntentResponse {
            intent,
            confidence: confidence.unwrap_or(0.0),
        })),
        Ok((None, _)) => {
            // Message exists but no intent yet
            Err(StatusCode::NOT_FOUND)
        }
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

// ---------------------------------------------------------------------------
// POST /api/ai/detect-intent
// ---------------------------------------------------------------------------

pub async fn detect_intent(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DetectIntentRequest>,
) -> Result<Json<IntentResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Fetch the message
    let msg: Result<(String, String, String), _> = conn.query_row(
        "SELECT COALESCE(subject, ''), COALESCE(from_address, ''), COALESCE(body_text, '')
         FROM messages WHERE id = ?1 AND is_deleted = 0",
        rusqlite::params![req.message_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    );

    let (subject, from, body) = msg.map_err(|_| StatusCode::NOT_FOUND)?;

    // Check AI is available
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

    let prompt = build_intent_prompt(&subject, &from, &body);

    let response = state
        .providers
        .generate(&prompt, Some(INTENT_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let intent_result = parse_intent_response(&response).ok_or(StatusCode::BAD_GATEWAY)?;

    // Store the result
    conn.execute(
        "UPDATE messages SET intent = ?2, intent_confidence = ?3, updated_at = unixepoch() WHERE id = ?1",
        rusqlite::params![req.message_id, intent_result.intent, intent_result.confidence],
    )
    .ok();

    Ok(Json(intent_result))
}

// ---------------------------------------------------------------------------
// Pipeline helper — for use by the job worker
// ---------------------------------------------------------------------------

/// Detect intent for a message using the AI provider pool.
/// Returns the parsed intent response, or None if AI is unavailable or parsing fails.
pub async fn detect_intent_for_message(
    providers: &crate::ai::provider::ProviderPool,
    subject: &str,
    from: &str,
    body: &str,
) -> Option<IntentResponse> {
    let prompt = build_intent_prompt(subject, from, body);
    let response = providers.generate(&prompt, Some(INTENT_SYSTEM_PROMPT)).await?;
    parse_intent_response(&response)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_intent_response_valid() {
        let response = r#"{"intent": "action_request", "confidence": 0.92}"#;
        let result = parse_intent_response(response).unwrap();
        assert_eq!(result.intent, "action_request");
        assert!((result.confidence - 0.92).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_intent_response_all_categories() {
        for category in VALID_INTENTS {
            let response = format!(r#"{{"intent": "{}", "confidence": 0.85}}"#, category);
            let result = parse_intent_response(&response).unwrap();
            assert_eq!(result.intent, *category);
        }
    }

    #[test]
    fn test_parse_intent_response_markdown_wrapped() {
        let response = "```json\n{\"intent\": \"question\", \"confidence\": 0.88}\n```";
        let result = parse_intent_response(response).unwrap();
        assert_eq!(result.intent, "question");
        assert!((result.confidence - 0.88).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_intent_response_invalid_category() {
        let response = r#"{"intent": "unknown_category", "confidence": 0.5}"#;
        let result = parse_intent_response(response);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_intent_response_invalid_json() {
        let response = "I cannot classify this email";
        let result = parse_intent_response(response);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_intent_response_clamps_confidence() {
        let response = r#"{"intent": "fyi", "confidence": 1.5}"#;
        let result = parse_intent_response(response).unwrap();
        assert_eq!(result.confidence, 1.0);

        let response = r#"{"intent": "fyi", "confidence": -0.5}"#;
        let result = parse_intent_response(response).unwrap();
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn test_parse_intent_response_case_insensitive() {
        let response = r#"{"intent": "ACTION_REQUEST", "confidence": 0.9}"#;
        let result = parse_intent_response(response).unwrap();
        assert_eq!(result.intent, "action_request");
    }

    #[test]
    fn test_build_intent_prompt() {
        let prompt = build_intent_prompt("Meeting tomorrow", "alice@test.com", "Let's meet at 3pm.");
        assert!(prompt.contains("From: alice@test.com"));
        assert!(prompt.contains("Subject: Meeting tomorrow"));
        assert!(prompt.contains("Let's meet at 3pm."));
    }

    #[test]
    fn test_build_intent_prompt_truncates_body() {
        let long_body = "x".repeat(3000);
        let prompt = build_intent_prompt("Test", "test@test.com", &long_body);
        // Body should be truncated to 2000 chars
        let body_start = prompt.find("\n\n").unwrap() + 2;
        let body_part = &prompt[body_start..];
        assert_eq!(body_part.len(), 2000);
    }

    #[test]
    fn test_extract_json_raw() {
        assert_eq!(
            extract_json(r#"  {"intent": "fyi"}  "#),
            r#"{"intent": "fyi"}"#
        );
    }

    #[test]
    fn test_extract_json_code_block() {
        let input = "Here's the result:\n```json\n{\"intent\": \"sales\"}\n```\nDone.";
        assert_eq!(extract_json(input), r#"{"intent": "sales"}"#);
    }

    #[test]
    fn test_stored_intent_retrieval() {
        // Test that we can store and retrieve intent from the database
        use crate::db::create_test_pool;

        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Create a test account and message
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc1', 'gmail', 'test@test.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, folder) VALUES ('msg1', 'acc1', 'INBOX')",
            [],
        )
        .unwrap();

        // Initially no intent
        let intent: Option<String> = conn
            .query_row(
                "SELECT intent FROM messages WHERE id = 'msg1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(intent.is_none());

        // Store intent
        conn.execute(
            "UPDATE messages SET intent = ?1, intent_confidence = ?2 WHERE id = 'msg1'",
            rusqlite::params!["action_request", 0.95],
        )
        .unwrap();

        // Retrieve intent
        let (intent, confidence): (String, f64) = conn
            .query_row(
                "SELECT intent, intent_confidence FROM messages WHERE id = 'msg1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(intent, "action_request");
        assert!((confidence - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_unknown_message_returns_none() {
        use crate::db::create_test_pool;

        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Query for a non-existent message
        let result: Result<(Option<String>, Option<f64>), _> = conn.query_row(
            "SELECT intent, intent_confidence FROM messages WHERE id = 'nonexistent'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        assert!(result.is_err());
    }
}
