use super::ollama::OllamaClient;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct AiMetadata {
    pub intent: String,
    pub priority_score: f64,
    pub priority_label: String,
    pub category: String,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
struct AiResponse {
    intent: Option<String>,
    priority_score: Option<f64>,
    priority_label: Option<String>,
    category: Option<String>,
    summary: Option<String>,
}

const SYSTEM_PROMPT: &str = r#"You are an email classification assistant. Analyze the email and respond with ONLY a JSON object (no markdown, no explanation) with these fields:

- "intent": one of ACTION_REQUEST, INFORMATIONAL, TRANSACTIONAL, SOCIAL, MARKETING, NOTIFICATION
- "priority_score": float 0.0-1.0 (1.0 = most urgent)
- "priority_label": one of "urgent", "high", "normal", "low"
- "category": one of "Primary", "Updates", "Social", "Promotions", "Finance", "Travel", "Newsletters"
- "summary": 1-2 sentence summary of the email

Respond with valid JSON only."#;

/// Process a single email through the AI pipeline.
/// Returns None if Ollama is unavailable or response can't be parsed.
pub async fn process_email(
    client: &OllamaClient,
    model: &str,
    subject: &str,
    from: &str,
    body: &str,
) -> Option<AiMetadata> {
    // Truncate body to avoid overwhelming the model
    let body_truncated = if body.len() > 2000 {
        &body[..2000]
    } else {
        body
    };

    let prompt = format!(
        "From: {from}\nSubject: {subject}\n\n{body_truncated}"
    );

    let response = client.generate(model, &prompt, Some(SYSTEM_PROMPT)).await?;

    parse_ai_response(&response)
}

/// Parse the JSON response from Ollama into AiMetadata.
fn parse_ai_response(response: &str) -> Option<AiMetadata> {
    // Try to extract JSON from the response (model may wrap it in markdown code blocks)
    let json_str = extract_json(response);

    match serde_json::from_str::<AiResponse>(json_str) {
        Ok(parsed) => Some(AiMetadata {
            intent: parsed.intent.unwrap_or_else(|| "INFORMATIONAL".to_string()),
            priority_score: parsed.priority_score.unwrap_or(0.5),
            priority_label: parsed.priority_label.unwrap_or_else(|| "normal".to_string()),
            category: parsed.category.unwrap_or_else(|| "Primary".to_string()),
            summary: parsed.summary.unwrap_or_default(),
        }),
        Err(e) => {
            tracing::warn!("Failed to parse AI response: {e}. Raw: {response}");
            None
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_clean_json() {
        let response = r#"{"intent":"ACTION_REQUEST","priority_score":0.8,"priority_label":"high","category":"Primary","summary":"Request to review PR"}"#;
        let meta = parse_ai_response(response).unwrap();
        assert_eq!(meta.intent, "ACTION_REQUEST");
        assert_eq!(meta.priority_score, 0.8);
        assert_eq!(meta.priority_label, "high");
        assert_eq!(meta.category, "Primary");
        assert_eq!(meta.summary, "Request to review PR");
    }

    #[test]
    fn test_parse_markdown_wrapped_json() {
        let response = "```json\n{\"intent\":\"MARKETING\",\"priority_score\":0.2,\"priority_label\":\"low\",\"category\":\"Promotions\",\"summary\":\"Sale announcement\"}\n```";
        let meta = parse_ai_response(response).unwrap();
        assert_eq!(meta.intent, "MARKETING");
        assert_eq!(meta.category, "Promotions");
    }

    #[test]
    fn test_parse_partial_response() {
        let response = r#"{"intent":"SOCIAL","summary":"Birthday party invite"}"#;
        let meta = parse_ai_response(response).unwrap();
        assert_eq!(meta.intent, "SOCIAL");
        assert_eq!(meta.priority_score, 0.5); // default
        assert_eq!(meta.priority_label, "normal"); // default
        assert_eq!(meta.category, "Primary"); // default
    }

    #[test]
    fn test_parse_invalid_json() {
        let response = "I can't process this email";
        let meta = parse_ai_response(response);
        assert!(meta.is_none());
    }

    #[test]
    fn test_extract_json_raw() {
        assert_eq!(extract_json(r#"  {"key": "val"}  "#), r#"{"key": "val"}"#);
    }

    #[test]
    fn test_extract_json_code_block() {
        let input = "Here's the result:\n```json\n{\"key\": \"val\"}\n```\nDone.";
        assert_eq!(extract_json(input), r#"{"key": "val"}"#);
    }
}
