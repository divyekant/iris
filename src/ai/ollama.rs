use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct OllamaClient {
    client: Client,
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Serialize)]
struct GenerateRequest {
    model: String,
    prompt: String,
    system: Option<String>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct GenerateResponse {
    response: String,
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Option<Vec<ModelInfo>>,
}

#[derive(Debug, Deserialize)]
pub struct ModelInfo {
    pub name: String,
}

impl OllamaClient {
    pub fn new(base_url: &str) -> Self {
        Self::with_model(base_url, "")
    }

    pub fn with_model(base_url: &str, model: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
        }
    }

    /// Check if Ollama is reachable
    pub async fn health(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        match self.client.get(&url).timeout(Duration::from_secs(5)).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// List available models
    pub async fn list_models(&self) -> Vec<String> {
        let url = format!("{}/api/tags", self.base_url);
        match self.client.get(&url).timeout(Duration::from_secs(5)).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<TagsResponse>().await {
                    Ok(tags) => tags.models.unwrap_or_default().into_iter().map(|m| m.name).collect(),
                    Err(_) => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    /// Generate a completion from Ollama
    pub async fn generate(&self, model: &str, prompt: &str, system: Option<&str>) -> Option<String> {
        let url = format!("{}/api/generate", self.base_url);

        let request = GenerateRequest {
            model: model.to_string(),
            prompt: prompt.to_string(),
            system: system.map(|s| s.to_string()),
            stream: false,
        };

        match self.client.post(&url).json(&request).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<GenerateResponse>().await {
                    Ok(resp) => Some(resp.response),
                    Err(e) => {
                        tracing::warn!("Failed to parse Ollama response: {e}");
                        None
                    }
                }
            }
            Ok(resp) => {
                tracing::warn!("Ollama returned status {}", resp.status());
                None
            }
            Err(e) => {
                tracing::warn!("Ollama request failed: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ollama_client_new() {
        let client = OllamaClient::new("http://localhost:11434/");
        assert_eq!(client.base_url, "http://localhost:11434");
        assert_eq!(client.model, "");
    }

    #[test]
    fn test_ollama_client_new_no_trailing_slash() {
        let client = OllamaClient::new("http://localhost:11434");
        assert_eq!(client.base_url, "http://localhost:11434");
    }

    #[test]
    fn test_ollama_client_with_model() {
        let client = OllamaClient::with_model("http://localhost:11434", "llama3.2");
        assert_eq!(client.model, "llama3.2");
    }
}
