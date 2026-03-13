use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_TEXT_BYTES: usize = 50_000;

// ---------------------------------------------------------------------------
// POST /api/ai/translate
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TranslateRequest {
    pub text: String,
    pub source_language: Option<String>,
    pub target_language: String,
    pub context: Option<String>,
    pub formality: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TranslateResponse {
    pub translated_text: String,
    pub detected_source: String,
    pub target_language: String,
}

/// Validate and normalise the `context` field.
/// Returns None if the value is invalid.
pub fn parse_context(context: Option<&str>) -> Option<&str> {
    match context {
        None => Some("email_compose"),
        Some("email_compose") | Some("casual") | Some("business") => context,
        _ => None,
    }
}

/// Validate and normalise the `formality` field.
/// Returns None if the value is invalid.
pub fn parse_formality(formality: Option<&str>) -> Option<&str> {
    match formality {
        None => Some("formal"),
        Some("formal") | Some("informal") | Some("neutral") => formality,
        _ => None,
    }
}

/// Build the system prompt for a translate request.
pub fn build_translate_system_prompt(context: &str, formality: &str) -> String {
    let context_desc = match context {
        "casual" => "casual conversation",
        "business" => "formal business communication",
        _ => "professional email composition",
    };

    let formality_desc = match formality {
        "informal" => "Use an informal, friendly tone.",
        "neutral" => "Use a neutral, balanced tone.",
        _ => "Use a formal, professional tone.",
    };

    format!(
        "You are a professional translator specialising in {}. \
{}  \
Preserve paragraph breaks, greetings, sign-offs, and email formatting. \
When translating, detect the source language if not provided. \
Respond with ONLY a JSON object (no markdown, no explanation) with these fields:\n\
- \"translated_text\": the translated text\n\
- \"detected_source\": the detected or confirmed source language name in English\n\
- \"target_language\": the target language name exactly as requested\n\
Respond with valid JSON only.",
        context_desc, formality_desc
    )
}

/// Build the user prompt for a translate request.
pub fn build_translate_user_prompt(
    text: &str,
    source_language: Option<&str>,
    target_language: &str,
) -> String {
    match source_language {
        Some(src) => format!(
            "Translate the following text from {} to {}:\n\n{}",
            src, target_language, text
        ),
        None => format!(
            "Detect the source language and translate the following text to {}:\n\n{}",
            target_language, text
        ),
    }
}

/// Parse a translate JSON response, handling markdown fence wrappers.
fn parse_translate_response(raw: &str) -> Option<(String, String, String)> {
    let json_str = extract_json(raw);
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let translated_text = v.get("translated_text")?.as_str()?.to_string();
    let detected_source = v
        .get("detected_source")
        .and_then(|s| s.as_str())
        .unwrap_or("Unknown")
        .to_string();
    let target_language = v
        .get("target_language")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();

    Some((translated_text, detected_source, target_language))
}

pub async fn translate(
    State(state): State<Arc<AppState>>,
    Json(input): Json<TranslateRequest>,
) -> Result<Json<TranslateResponse>, StatusCode> {
    // Validate inputs
    if input.text.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if input.text.len() > MAX_TEXT_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    if input.target_language.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let context = parse_context(input.context.as_deref()).ok_or(StatusCode::BAD_REQUEST)?;
    let formality = parse_formality(input.formality.as_deref()).ok_or(StatusCode::BAD_REQUEST)?;

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

    let system_prompt = build_translate_system_prompt(context, formality);
    let user_prompt = build_translate_user_prompt(
        &input.text,
        input.source_language.as_deref(),
        &input.target_language,
    );

    let raw = state
        .providers
        .generate(&user_prompt, Some(&system_prompt))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let (translated_text, detected_source, target_language) =
        parse_translate_response(&raw).ok_or(StatusCode::BAD_GATEWAY)?;

    Ok(Json(TranslateResponse {
        translated_text,
        detected_source,
        target_language,
    }))
}

// ---------------------------------------------------------------------------
// POST /api/ai/detect-language
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DetectLanguageRequest {
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct DetectLanguageResponse {
    pub language: String,
    pub confidence: f64,
}

const DETECT_SYSTEM_PROMPT: &str =
    "You are a language identification expert. Identify the language of the given text. \
Respond with ONLY a JSON object (no markdown, no explanation) with these fields:\n\
- \"language\": the detected language name in English (e.g. \"French\", \"Japanese\")\n\
- \"confidence\": a float from 0.0 to 1.0 representing your confidence\n\
Respond with valid JSON only.";

/// Build the user prompt for language detection.
pub fn build_detect_user_prompt(text: &str) -> String {
    format!("Identify the language of this text:\n\n{}", text)
}

/// Parse a detect-language JSON response.
fn parse_detect_response(raw: &str) -> Option<(String, f64)> {
    let json_str = extract_json(raw);
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let language = v.get("language")?.as_str()?.to_string();
    let confidence = v
        .get("confidence")
        .and_then(|c| c.as_f64())
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);

    Some((language, confidence))
}

pub async fn detect_language(
    State(state): State<Arc<AppState>>,
    Json(input): Json<DetectLanguageRequest>,
) -> Result<Json<DetectLanguageResponse>, StatusCode> {
    if input.text.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if input.text.len() > MAX_TEXT_BYTES {
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

    let user_prompt = build_detect_user_prompt(&input.text);

    let raw = state
        .providers
        .generate(&user_prompt, Some(DETECT_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let (language, confidence) = parse_detect_response(&raw).ok_or(StatusCode::BAD_GATEWAY)?;

    Ok(Json(DetectLanguageResponse {
        language,
        confidence,
    }))
}

// ---------------------------------------------------------------------------
// POST /api/ai/translate-email
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TranslateEmailRequest {
    pub subject: String,
    pub body: String,
    pub target_language: String,
    pub formality: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TranslateEmailResponse {
    pub subject: String,
    pub body: String,
    pub detected_source: String,
    pub target_language: String,
}

const TRANSLATE_EMAIL_SYSTEM_PROMPT_BASE: &str =
    "You are a professional email translator. Translate both the subject and body of the email, \
preserving email structure, paragraph breaks, greetings, sign-offs, and formatting. \
Detect the source language automatically. \
Respond with ONLY a JSON object (no markdown, no explanation) with these fields:\n\
- \"subject\": the translated subject line\n\
- \"body\": the translated email body\n\
- \"detected_source\": the detected source language name in English\n\
- \"target_language\": the target language name exactly as requested\n\
Respond with valid JSON only.";

/// Build the system prompt for email translation, incorporating formality.
pub fn build_translate_email_system_prompt(formality: &str) -> String {
    let formality_clause = match formality {
        "informal" => " Use an informal, friendly tone.",
        "neutral" => " Use a neutral, balanced tone.",
        _ => " Use a formal, professional tone.",
    };
    format!("{}{}", TRANSLATE_EMAIL_SYSTEM_PROMPT_BASE, formality_clause)
}

/// Build the user prompt for email translation.
pub fn build_translate_email_user_prompt(
    subject: &str,
    body: &str,
    target_language: &str,
) -> String {
    format!(
        "Translate the following email to {}.\n\nSubject: {}\n\nBody:\n{}",
        target_language, subject, body
    )
}

/// Parse a translate-email JSON response.
fn parse_translate_email_response(raw: &str) -> Option<(String, String, String, String)> {
    let json_str = extract_json(raw);
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let subject = v.get("subject")?.as_str()?.to_string();
    let body = v.get("body")?.as_str()?.to_string();
    let detected_source = v
        .get("detected_source")
        .and_then(|s| s.as_str())
        .unwrap_or("Unknown")
        .to_string();
    let target_language = v
        .get("target_language")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();

    Some((subject, body, detected_source, target_language))
}

pub async fn translate_email(
    State(state): State<Arc<AppState>>,
    Json(input): Json<TranslateEmailRequest>,
) -> Result<Json<TranslateEmailResponse>, StatusCode> {
    // Validate inputs
    if input.body.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let combined_len = input.subject.len() + input.body.len();
    if combined_len > MAX_TEXT_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    if input.target_language.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let formality = parse_formality(input.formality.as_deref()).ok_or(StatusCode::BAD_REQUEST)?;

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

    let system_prompt = build_translate_email_system_prompt(formality);
    let user_prompt =
        build_translate_email_user_prompt(&input.subject, &input.body, &input.target_language);

    let raw = state
        .providers
        .generate(&user_prompt, Some(&system_prompt))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let (subject, body, detected_source, target_language) =
        parse_translate_email_response(&raw).ok_or(StatusCode::BAD_GATEWAY)?;

    Ok(Json(TranslateEmailResponse {
        subject,
        body,
        detected_source,
        target_language,
    }))
}

// ---------------------------------------------------------------------------
// Shared JSON extraction helper (mirrors src/ai/pipeline.rs)
// ---------------------------------------------------------------------------

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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Input validation helpers ---

    #[test]
    fn test_parse_context_default() {
        assert_eq!(parse_context(None), Some("email_compose"));
    }

    #[test]
    fn test_parse_context_valid_values() {
        assert_eq!(parse_context(Some("email_compose")), Some("email_compose"));
        assert_eq!(parse_context(Some("casual")), Some("casual"));
        assert_eq!(parse_context(Some("business")), Some("business"));
    }

    #[test]
    fn test_parse_context_invalid() {
        assert_eq!(parse_context(Some("unknown")), None);
        assert_eq!(parse_context(Some("")), None);
        assert_eq!(parse_context(Some("FORMAL")), None);
    }

    #[test]
    fn test_parse_formality_default() {
        assert_eq!(parse_formality(None), Some("formal"));
    }

    #[test]
    fn test_parse_formality_valid_values() {
        assert_eq!(parse_formality(Some("formal")), Some("formal"));
        assert_eq!(parse_formality(Some("informal")), Some("informal"));
        assert_eq!(parse_formality(Some("neutral")), Some("neutral"));
    }

    #[test]
    fn test_parse_formality_invalid() {
        assert_eq!(parse_formality(Some("ultra-formal")), None);
        assert_eq!(parse_formality(Some("")), None);
    }

    // --- Prompt building ---

    #[test]
    fn test_build_translate_system_prompt_email_compose_formal() {
        let prompt = build_translate_system_prompt("email_compose", "formal");
        assert!(prompt.contains("professional email composition"));
        assert!(prompt.contains("formal, professional tone"));
    }

    #[test]
    fn test_build_translate_system_prompt_casual_informal() {
        let prompt = build_translate_system_prompt("casual", "informal");
        assert!(prompt.contains("casual conversation"));
        assert!(prompt.contains("informal, friendly tone"));
    }

    #[test]
    fn test_build_translate_system_prompt_business_neutral() {
        let prompt = build_translate_system_prompt("business", "neutral");
        assert!(prompt.contains("formal business communication"));
        assert!(prompt.contains("neutral, balanced tone"));
    }

    #[test]
    fn test_build_translate_user_prompt_with_source() {
        let prompt = build_translate_user_prompt("Hello", Some("English"), "Japanese");
        assert!(prompt.contains("from English to Japanese"));
        assert!(prompt.contains("Hello"));
    }

    #[test]
    fn test_build_translate_user_prompt_auto_detect() {
        let prompt = build_translate_user_prompt("Bonjour", None, "German");
        assert!(prompt.contains("Detect the source language"));
        assert!(prompt.contains("to German"));
        assert!(prompt.contains("Bonjour"));
    }

    // --- Language detection prompt ---

    #[test]
    fn test_build_detect_user_prompt() {
        let prompt = build_detect_user_prompt("こんにちは");
        assert!(prompt.contains("Identify the language"));
        assert!(prompt.contains("こんにちは"));
    }

    // --- Email translation prompt ---

    #[test]
    fn test_build_translate_email_system_prompt_formal() {
        let prompt = build_translate_email_system_prompt("formal");
        assert!(prompt.contains("formal, professional tone"));
    }

    #[test]
    fn test_build_translate_email_system_prompt_informal() {
        let prompt = build_translate_email_system_prompt("informal");
        assert!(prompt.contains("informal, friendly tone"));
    }

    #[test]
    fn test_build_translate_email_user_prompt() {
        let prompt = build_translate_email_user_prompt("Hello", "Hi team", "Spanish");
        assert!(prompt.contains("to Spanish"));
        assert!(prompt.contains("Subject: Hello"));
        assert!(prompt.contains("Hi team"));
    }

    // --- JSON extraction ---

    #[test]
    fn test_extract_json_plain() {
        let raw = r#"{"translated_text":"こんにちは","detected_source":"English","target_language":"Japanese"}"#;
        assert_eq!(extract_json(raw), raw);
    }

    #[test]
    fn test_extract_json_markdown_fence() {
        let raw = "```json\n{\"language\":\"French\",\"confidence\":0.95}\n```";
        let result = extract_json(raw);
        assert!(result.contains("French"));
        assert!(!result.contains("```"));
    }

    #[test]
    fn test_extract_json_bare_fence() {
        let raw = "```\n{\"a\":1}\n```";
        let result = extract_json(raw);
        assert_eq!(result, "{\"a\":1}");
    }

    #[test]
    fn test_extract_json_with_preamble() {
        let raw = "Here is the translation: {\"translated_text\":\"Hola\",\"detected_source\":\"English\",\"target_language\":\"Spanish\"}";
        let result = extract_json(raw);
        assert!(result.starts_with('{'));
        assert!(result.ends_with('}'));
    }

    // --- Response parsers ---

    #[test]
    fn test_parse_translate_response_valid() {
        let raw = r#"{"translated_text":"こんにちは","detected_source":"English","target_language":"Japanese"}"#;
        let (text, src, tgt) = parse_translate_response(raw).unwrap();
        assert_eq!(text, "こんにちは");
        assert_eq!(src, "English");
        assert_eq!(tgt, "Japanese");
    }

    #[test]
    fn test_parse_translate_response_missing_detected_source_defaults() {
        let raw = r#"{"translated_text":"Hola","target_language":"Spanish"}"#;
        let (text, src, tgt) = parse_translate_response(raw).unwrap();
        assert_eq!(text, "Hola");
        assert_eq!(src, "Unknown");
        assert_eq!(tgt, "Spanish");
    }

    #[test]
    fn test_parse_translate_response_missing_translated_text_fails() {
        let raw = r#"{"detected_source":"English","target_language":"Japanese"}"#;
        assert!(parse_translate_response(raw).is_none());
    }

    #[test]
    fn test_parse_detect_response_valid() {
        let raw = r#"{"language":"French","confidence":0.95}"#;
        let (lang, conf) = parse_detect_response(raw).unwrap();
        assert_eq!(lang, "French");
        assert!((conf - 0.95).abs() < 1e-9);
    }

    #[test]
    fn test_parse_detect_response_missing_confidence_defaults_to_1() {
        let raw = r#"{"language":"German"}"#;
        let (lang, conf) = parse_detect_response(raw).unwrap();
        assert_eq!(lang, "German");
        assert!((conf - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_detect_response_confidence_clamped() {
        let raw = r#"{"language":"Spanish","confidence":1.5}"#;
        let (_, conf) = parse_detect_response(raw).unwrap();
        assert!((conf - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_detect_response_missing_language_fails() {
        let raw = r#"{"confidence":0.9}"#;
        assert!(parse_detect_response(raw).is_none());
    }

    #[test]
    fn test_parse_translate_email_response_valid() {
        let raw = r#"{"subject":"Reunión","body":"Hola equipo","detected_source":"English","target_language":"Spanish"}"#;
        let (subj, body, src, tgt) = parse_translate_email_response(raw).unwrap();
        assert_eq!(subj, "Reunión");
        assert_eq!(body, "Hola equipo");
        assert_eq!(src, "English");
        assert_eq!(tgt, "Spanish");
    }

    #[test]
    fn test_parse_translate_email_response_missing_body_fails() {
        let raw = r#"{"subject":"Test","detected_source":"English","target_language":"French"}"#;
        assert!(parse_translate_email_response(raw).is_none());
    }

    #[test]
    fn test_parse_translate_email_response_in_fence() {
        let raw = "```json\n{\"subject\":\"Test\",\"body\":\"Corp\",\"detected_source\":\"English\",\"target_language\":\"French\"}\n```";
        let (subj, body, src, tgt) = parse_translate_email_response(raw).unwrap();
        assert_eq!(subj, "Test");
        assert_eq!(body, "Corp");
        assert_eq!(src, "English");
        assert_eq!(tgt, "French");
    }
}
