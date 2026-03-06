use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct AnthropicClient {
    client: Client,
    api_key: String,
    pub model: String,
    is_oauth: bool,
}

#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<Message>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    text: Option<String>,
}

/// OAuth tokens start with this prefix and need Bearer auth + beta headers.
fn is_oauth_token(key: &str) -> bool {
    key.starts_with("sk-ant-oat01-")
}

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const OAUTH_BETA_HEADER: &str = "oauth-2025-04-20,interleaved-thinking-2025-05-14";

impl AnthropicClient {
    pub fn new(api_key: &str, model: Option<&str>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to create HTTP client");

        let is_oauth = is_oauth_token(api_key);
        if is_oauth {
            tracing::info!("Anthropic: using OAuth subscription token (Bearer auth)");
        }

        Self {
            client,
            api_key: api_key.to_string(),
            model: model.unwrap_or("claude-haiku-4-5-20251001").to_string(),
            is_oauth,
        }
    }

    pub async fn health(&self) -> bool {
        !self.api_key.is_empty()
    }

    pub async fn generate(&self, prompt: &str, system: Option<&str>) -> Option<String> {
        let body = MessagesRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            system: system.map(|s| s.to_string()),
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        // Build URL: OAuth tokens need ?beta=true
        let url = if self.is_oauth {
            format!("{}?beta=true", ANTHROPIC_API_URL)
        } else {
            ANTHROPIC_API_URL.to_string()
        };

        // Build request with appropriate auth
        let mut req = self.client.post(&url).json(&body);

        req = req.header("anthropic-version", "2023-06-01");

        if self.is_oauth {
            // OAuth: Bearer auth + beta headers, NO x-api-key
            req = req
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("anthropic-beta", OAUTH_BETA_HEADER);
        } else {
            // Standard API key auth
            req = req.header("x-api-key", &self.api_key);
        }

        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<MessagesResponse>().await {
                    Ok(r) => r.content.into_iter().find_map(|b| b.text),
                    Err(e) => {
                        tracing::warn!("Failed to parse Anthropic response: {e}");
                        None
                    }
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!("Anthropic returned status {}: {}", status, body);
                None
            }
            Err(e) => {
                tracing::warn!("Anthropic request failed: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_client_default_model() {
        let client = AnthropicClient::new("sk-ant-api03-test", None);
        assert_eq!(client.model, "claude-haiku-4-5-20251001");
        assert!(!client.is_oauth);
    }

    #[test]
    fn test_anthropic_client_custom_model() {
        let client = AnthropicClient::new("sk-ant-api03-test", Some("claude-sonnet-4-6"));
        assert_eq!(client.model, "claude-sonnet-4-6");
    }

    #[test]
    fn test_anthropic_oauth_detection() {
        let client = AnthropicClient::new("sk-ant-oat01-abc123", None);
        assert!(client.is_oauth);
    }

    #[test]
    fn test_anthropic_api_key_not_oauth() {
        let client = AnthropicClient::new("sk-ant-api03-xyz", None);
        assert!(!client.is_oauth);
    }

    #[test]
    fn test_is_oauth_token() {
        assert!(is_oauth_token("sk-ant-oat01-abc"));
        assert!(!is_oauth_token("sk-ant-api03-abc"));
        assert!(!is_oauth_token("random-key"));
    }
}
