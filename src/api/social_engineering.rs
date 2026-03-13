use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::message::MessageDetail;
use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SocialEngineeringTactic {
    #[serde(rename = "type")]
    pub tactic_type: String,
    pub evidence: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SocialEngineeringResult {
    pub risk_level: String,
    pub tactics: Vec<SocialEngineeringTactic>,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
pub struct DetectRequest {
    pub message_id: String,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const VALID_RISK_LEVELS: &[&str] = &["none", "low", "medium", "high", "critical"];

const VALID_TACTIC_TYPES: &[&str] = &[
    "urgency_pressure",
    "authority_exploitation",
    "fear_threat",
    "reward_lure",
    "trust_exploitation",
    "information_harvesting",
];

const SE_SYSTEM_PROMPT: &str = r#"You are a cybersecurity analyst specializing in social engineering detection. Analyze the following email for manipulation tactics.

Evaluate for these categories:
1. urgency_pressure — artificial time pressure or deadline threats
2. authority_exploitation — impersonating authority figures or claiming organizational power
3. fear_threat — threats of negative consequences
4. reward_lure — promises of rewards, prizes, or financial gain
5. trust_exploitation — referencing fake prior agreements or relationships
6. information_harvesting — requesting sensitive personal or financial information

For each detected tactic, provide:
- type: category name
- evidence: exact quoted text from the email
- confidence: 0.0 to 1.0

Assign overall risk_level:
- none: no tactics detected
- low: minor suspicious elements, likely benign
- medium: clear manipulation attempt, user should be cautious
- high: multiple tactics, likely malicious
- critical: sophisticated attack targeting sensitive information

Respond in JSON format only: { "risk_level": "...", "tactics": [...], "summary": "..." }"#;

// ---------------------------------------------------------------------------
// AI response parsing
// ---------------------------------------------------------------------------

/// Parse the AI-generated JSON response into a SocialEngineeringResult.
/// Falls back gracefully on malformed responses.
pub fn parse_ai_response(raw: &str) -> SocialEngineeringResult {
    // Try to extract JSON from the response (AI may wrap in markdown code blocks)
    let json_str = extract_json_block(raw);

    match serde_json::from_str::<serde_json::Value>(json_str) {
        Ok(val) => parse_json_value(&val),
        Err(_) => SocialEngineeringResult {
            risk_level: "none".to_string(),
            tactics: vec![],
            summary: "Analysis could not be completed — unable to parse AI response.".to_string(),
        },
    }
}

/// Extract a JSON block from potentially markdown-wrapped AI output.
fn extract_json_block(raw: &str) -> &str {
    let trimmed = raw.trim();
    // Strip ```json ... ``` wrapping
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return &trimmed[start..=end];
        }
    }
    trimmed
}

/// Parse a serde_json::Value into our result struct with validation.
fn parse_json_value(val: &serde_json::Value) -> SocialEngineeringResult {
    let risk_level = val
        .get("risk_level")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase())
        .filter(|s| VALID_RISK_LEVELS.contains(&s.as_str()))
        .unwrap_or_else(|| "none".to_string());

    let tactics = val
        .get("tactics")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| parse_tactic(t))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let summary = val
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("No summary available.")
        .to_string();

    // Validate: if no tactics but risk_level > low, downgrade
    let risk_level = if tactics.is_empty() && risk_level != "none" && risk_level != "low" {
        "none".to_string()
    } else {
        risk_level
    };

    SocialEngineeringResult {
        risk_level,
        tactics,
        summary,
    }
}

/// Parse a single tactic from a JSON value.
fn parse_tactic(val: &serde_json::Value) -> Option<SocialEngineeringTactic> {
    let tactic_type = val
        .get("type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase())?;

    // Must be a recognized tactic type
    if !VALID_TACTIC_TYPES.contains(&tactic_type.as_str()) {
        return None;
    }

    let evidence = val
        .get("evidence")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let confidence = val
        .get("confidence")
        .and_then(|v| v.as_f64())
        .map(|c| c.clamp(0.0, 1.0))
        .unwrap_or(0.5);

    Some(SocialEngineeringTactic {
        tactic_type,
        evidence,
        confidence,
    })
}

/// Build the user prompt from an email message for analysis.
pub fn build_analysis_prompt(
    from: &str,
    subject: &str,
    body: &str,
) -> String {
    let body_truncated = if body.chars().count() > 5000 {
        let end = body.char_indices().nth(5000).map(|(i, _)| i).unwrap_or(body.len());
        format!("{}...(truncated)", &body[..end])
    } else {
        body.to_string()
    };

    format!(
        "Analyze this email for social engineering tactics:\n\nFrom: {}\nSubject: {}\n\nBody:\n{}",
        from, subject, body_truncated
    )
}

// ---------------------------------------------------------------------------
// POST /api/ai/detect-social-engineering
// ---------------------------------------------------------------------------

pub async fn detect_social_engineering(
    State(state): State<Arc<AppState>>,
    Json(input): Json<DetectRequest>,
) -> Result<Json<SocialEngineeringResult>, StatusCode> {
    if input.message_id.trim().is_empty() || input.message_id.len() > 500 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Check cache first
    if let Some(cached) = get_cached_result(&conn, &input.message_id) {
        return Ok(Json(cached));
    }

    // Fetch the message
    let message = MessageDetail::get_by_id(&conn, &input.message_id)
        .ok_or(StatusCode::NOT_FOUND)?;

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

    let from = message.from_address.as_deref().unwrap_or("unknown");
    let subject = message.subject.as_deref().unwrap_or("(no subject)");
    let body = message
        .body_text
        .as_deref()
        .or(message.body_html.as_deref())
        .unwrap_or("");

    // Short emails (< 20 chars body) are unlikely to be social engineering
    if body.len() < 20 {
        let result = SocialEngineeringResult {
            risk_level: "none".to_string(),
            tactics: vec![],
            summary: "Email body too short for meaningful analysis.".to_string(),
        };
        store_result(&conn, &input.message_id, &result);
        return Ok(Json(result));
    }

    let prompt = build_analysis_prompt(from, subject, body);

    let ai_response = state
        .providers
        .generate(&prompt, Some(SE_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let result = parse_ai_response(&ai_response);

    // Cache the result
    store_result(&conn, &input.message_id, &result);

    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// GET /api/messages/{id}/social-engineering
// ---------------------------------------------------------------------------

pub async fn get_social_engineering(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Option<SocialEngineeringResult>>, StatusCode> {
    if id.trim().is_empty() || id.len() > 500 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Verify message exists
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE id = ?1 AND is_deleted = 0",
            rusqlite::params![&id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }

    match get_cached_result(&conn, &id) {
        Some(result) => Ok(Json(Some(result))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

// ---------------------------------------------------------------------------
// DB helpers
// ---------------------------------------------------------------------------

fn get_cached_result(
    conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    message_id: &str,
) -> Option<SocialEngineeringResult> {
    conn.query_row(
        "SELECT risk_level, tactics_json, summary FROM social_engineering_analysis WHERE message_id = ?1",
        rusqlite::params![message_id],
        |row| {
            let risk_level: String = row.get(0)?;
            let tactics_json: Option<String> = row.get(1)?;
            let summary: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();

            let tactics: Vec<SocialEngineeringTactic> = tactics_json
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default();

            Ok(SocialEngineeringResult {
                risk_level,
                tactics,
                summary,
            })
        },
    )
    .ok()
}

fn store_result(
    conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    message_id: &str,
    result: &SocialEngineeringResult,
) {
    let tactics_json = serde_json::to_string(&result.tactics).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT OR REPLACE INTO social_engineering_analysis (message_id, risk_level, tactics_json, summary)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![message_id, result.risk_level, tactics_json, result.summary],
    )
    .ok();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_response_all_fields() {
        let json = r#"{
            "risk_level": "high",
            "tactics": [
                { "type": "urgency_pressure", "evidence": "Act now or lose access!", "confidence": 0.9 },
                { "type": "fear_threat", "evidence": "Your account has been compromised", "confidence": 0.85 }
            ],
            "summary": "This email uses urgency and fear tactics to pressure the recipient."
        }"#;

        let result = parse_ai_response(json);
        assert_eq!(result.risk_level, "high");
        assert_eq!(result.tactics.len(), 2);
        assert_eq!(result.tactics[0].tactic_type, "urgency_pressure");
        assert_eq!(result.tactics[0].evidence, "Act now or lose access!");
        assert!((result.tactics[0].confidence - 0.9).abs() < f64::EPSILON);
        assert_eq!(result.tactics[1].tactic_type, "fear_threat");
        assert!(result.summary.contains("urgency"));
    }

    #[test]
    fn test_parse_malformed_response_graceful_fallback() {
        let garbage = "This is not JSON at all, just random text from the AI.";
        let result = parse_ai_response(garbage);
        assert_eq!(result.risk_level, "none");
        assert!(result.tactics.is_empty());
        assert!(result.summary.contains("unable to parse"));
    }

    #[test]
    fn test_parse_response_with_markdown_wrapping() {
        let wrapped = r#"```json
        { "risk_level": "medium", "tactics": [{ "type": "reward_lure", "evidence": "You've won $1M!", "confidence": 0.95 }], "summary": "Reward lure detected." }
        ```"#;

        let result = parse_ai_response(wrapped);
        assert_eq!(result.risk_level, "medium");
        assert_eq!(result.tactics.len(), 1);
        assert_eq!(result.tactics[0].tactic_type, "reward_lure");
    }

    #[test]
    fn test_parse_urgency_pressure_tactic() {
        let json = r#"{
            "risk_level": "medium",
            "tactics": [{ "type": "urgency_pressure", "evidence": "Your account will be suspended in 24 hours", "confidence": 0.88 }],
            "summary": "Urgency pressure detected."
        }"#;
        let result = parse_ai_response(json);
        assert_eq!(result.tactics[0].tactic_type, "urgency_pressure");
        assert!((result.tactics[0].confidence - 0.88).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_authority_exploitation_tactic() {
        let json = r#"{
            "risk_level": "high",
            "tactics": [{ "type": "authority_exploitation", "evidence": "As per CEO's directive", "confidence": 0.82 }],
            "summary": "Authority exploitation detected."
        }"#;
        let result = parse_ai_response(json);
        assert_eq!(result.tactics[0].tactic_type, "authority_exploitation");
    }

    #[test]
    fn test_parse_fear_threat_tactic() {
        let json = r#"{
            "risk_level": "high",
            "tactics": [{ "type": "fear_threat", "evidence": "Legal action will be taken", "confidence": 0.91 }],
            "summary": "Fear/threat tactic."
        }"#;
        let result = parse_ai_response(json);
        assert_eq!(result.tactics[0].tactic_type, "fear_threat");
    }

    #[test]
    fn test_parse_reward_lure_tactic() {
        let json = r#"{
            "risk_level": "critical",
            "tactics": [{ "type": "reward_lure", "evidence": "Claim your $500 gift card", "confidence": 0.96 }],
            "summary": "Reward lure."
        }"#;
        let result = parse_ai_response(json);
        assert_eq!(result.tactics[0].tactic_type, "reward_lure");
    }

    #[test]
    fn test_parse_trust_exploitation_tactic() {
        let json = r#"{
            "risk_level": "medium",
            "tactics": [{ "type": "trust_exploitation", "evidence": "As we discussed last week", "confidence": 0.75 }],
            "summary": "Trust exploitation."
        }"#;
        let result = parse_ai_response(json);
        assert_eq!(result.tactics[0].tactic_type, "trust_exploitation");
    }

    #[test]
    fn test_parse_information_harvesting_tactic() {
        let json = r#"{
            "risk_level": "critical",
            "tactics": [{ "type": "information_harvesting", "evidence": "Please send your SSN and bank details", "confidence": 0.98 }],
            "summary": "Information harvesting."
        }"#;
        let result = parse_ai_response(json);
        assert_eq!(result.tactics[0].tactic_type, "information_harvesting");
        assert!((result.tactics[0].confidence - 0.98).abs() < f64::EPSILON);
    }

    #[test]
    fn test_risk_level_none_when_no_tactics() {
        let json = r#"{
            "risk_level": "none",
            "tactics": [],
            "summary": "No social engineering tactics detected."
        }"#;
        let result = parse_ai_response(json);
        assert_eq!(result.risk_level, "none");
        assert!(result.tactics.is_empty());
    }

    #[test]
    fn test_risk_level_downgrade_when_tactics_empty_but_level_high() {
        // AI says high risk but provides no tactics — downgrade to none
        let json = r#"{
            "risk_level": "high",
            "tactics": [],
            "summary": "Something suspicious but no specifics."
        }"#;
        let result = parse_ai_response(json);
        assert_eq!(result.risk_level, "none");
    }

    #[test]
    fn test_invalid_risk_level_defaults_to_none() {
        let json = r#"{
            "risk_level": "extreme_danger",
            "tactics": [],
            "summary": "Invalid level."
        }"#;
        let result = parse_ai_response(json);
        assert_eq!(result.risk_level, "none");
    }

    #[test]
    fn test_invalid_tactic_type_filtered_out() {
        let json = r#"{
            "risk_level": "low",
            "tactics": [
                { "type": "unknown_tactic", "evidence": "something", "confidence": 0.5 },
                { "type": "urgency_pressure", "evidence": "Act now!", "confidence": 0.7 }
            ],
            "summary": "Mixed."
        }"#;
        let result = parse_ai_response(json);
        // Only the valid tactic should remain
        assert_eq!(result.tactics.len(), 1);
        assert_eq!(result.tactics[0].tactic_type, "urgency_pressure");
    }

    #[test]
    fn test_confidence_clamped_to_range() {
        let json = r#"{
            "risk_level": "low",
            "tactics": [{ "type": "urgency_pressure", "evidence": "hurry", "confidence": 1.5 }],
            "summary": "Clamped."
        }"#;
        let result = parse_ai_response(json);
        assert!((result.tactics[0].confidence - 1.0).abs() < f64::EPSILON);

        let json2 = r#"{
            "risk_level": "low",
            "tactics": [{ "type": "urgency_pressure", "evidence": "hurry", "confidence": -0.5 }],
            "summary": "Clamped."
        }"#;
        let result2 = parse_ai_response(json2);
        assert!((result2.tactics[0].confidence - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_build_analysis_prompt_structure() {
        let prompt = build_analysis_prompt(
            "attacker@evil.com",
            "URGENT: Verify your account",
            "Click here to verify your credentials immediately.",
        );
        assert!(prompt.contains("From: attacker@evil.com"));
        assert!(prompt.contains("Subject: URGENT: Verify your account"));
        assert!(prompt.contains("Click here to verify"));
    }

    #[test]
    fn test_build_analysis_prompt_truncates_long_body() {
        let long_body = "x".repeat(10_000);
        let prompt = build_analysis_prompt("test@test.com", "Test", &long_body);
        assert!(prompt.len() < 6000);
        assert!(prompt.contains("truncated"));
    }

    #[test]
    fn test_cached_result_roundtrip() {
        // Test that serialization/deserialization of tactics works
        let tactics = vec![
            SocialEngineeringTactic {
                tactic_type: "urgency_pressure".to_string(),
                evidence: "Act now!".to_string(),
                confidence: 0.9,
            },
        ];
        let json = serde_json::to_string(&tactics).unwrap();
        let parsed: Vec<SocialEngineeringTactic> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].tactic_type, "urgency_pressure");
    }

    #[test]
    fn test_parse_missing_summary_field() {
        let json = r#"{
            "risk_level": "low",
            "tactics": [{ "type": "urgency_pressure", "evidence": "hurry", "confidence": 0.6 }]
        }"#;
        let result = parse_ai_response(json);
        assert_eq!(result.summary, "No summary available.");
    }

    #[test]
    fn test_parse_missing_tactics_field() {
        let json = r#"{
            "risk_level": "none",
            "summary": "Clean email."
        }"#;
        let result = parse_ai_response(json);
        assert!(result.tactics.is_empty());
        assert_eq!(result.risk_level, "none");
    }

    #[test]
    fn test_parse_missing_evidence_in_tactic() {
        let json = r#"{
            "risk_level": "low",
            "tactics": [{ "type": "urgency_pressure", "confidence": 0.7 }],
            "summary": "Test."
        }"#;
        let result = parse_ai_response(json);
        assert_eq!(result.tactics.len(), 1);
        assert_eq!(result.tactics[0].evidence, "");
    }

    #[test]
    fn test_parse_missing_confidence_defaults() {
        let json = r#"{
            "risk_level": "low",
            "tactics": [{ "type": "urgency_pressure", "evidence": "hurry" }],
            "summary": "Test."
        }"#;
        let result = parse_ai_response(json);
        assert_eq!(result.tactics.len(), 1);
        assert!((result.tactics[0].confidence - 0.5).abs() < f64::EPSILON);
    }
}
