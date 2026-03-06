use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct OpenAIClient {
    client: Client,
    api_key: String,
    pub model: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

impl OpenAIClient {
    pub fn new(api_key: &str, model: Option<&str>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            api_key: api_key.to_string(),
            model: model.unwrap_or("gpt-4o-mini").to_string(),
        }
    }

    pub async fn health(&self) -> bool {
        !self.api_key.is_empty()
    }

    pub async fn generate(&self, prompt: &str, system: Option<&str>) -> Option<String> {
        let mut messages = Vec::new();
        if let Some(sys) = system {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: sys.to_string(),
            });
        }
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        });

        let body = ChatCompletionRequest {
            model: self.model.clone(),
            messages,
            max_tokens: 4096,
        };

        match self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ChatCompletionResponse>().await {
                    Ok(r) => r.choices.into_iter().next().and_then(|c| c.message.content),
                    Err(e) => {
                        tracing::warn!("Failed to parse OpenAI response: {e}");
                        None
                    }
                }
            }
            Ok(resp) => {
                tracing::warn!("OpenAI returned status {}", resp.status());
                None
            }
            Err(e) => {
                tracing::warn!("OpenAI request failed: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_client_default_model() {
        let client = OpenAIClient::new("sk-test", None);
        assert_eq!(client.model, "gpt-4o-mini");
    }

    #[test]
    fn test_openai_client_custom_model() {
        let client = OpenAIClient::new("sk-test", Some("gpt-4o"));
        assert_eq!(client.model, "gpt-4o");
    }
}
