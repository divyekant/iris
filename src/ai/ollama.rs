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

    /// Generate a completion with tool-use support (text-based fallback for Ollama).
    ///
    /// Injects tool descriptions into the system prompt and parses `TOOL_CALL:` markers
    /// from the model's text output. Does not use Ollama's native tool-calling API.
    pub async fn generate_with_tools(
        &self,
        messages: &[crate::ai::tools::LlmMessage],
        system: Option<&str>,
        tools: &[crate::ai::tools::Tool],
    ) -> Option<crate::ai::tools::LlmResponse> {
        use crate::ai::tools::{LlmMessage, LlmResponse, ToolCall};

        // Build tool description block for system prompt
        let mut tool_desc = String::from(
            "You have access to these tools. To use a tool, output TOOL_CALL: followed by a JSON object.\n\
             Only output ONE tool call per response. After receiving results, continue your analysis.\n\n\
             Available tools:\n",
        );
        for t in tools {
            let params: Vec<String> = t
                .input_schema
                .get("properties")
                .and_then(|p| p.as_object())
                .map(|props| props.keys().map(|k| format!("{}: string", k)).collect())
                .unwrap_or_default();
            if params.is_empty() {
                tool_desc.push_str(&format!("- {}(): {}\n", t.name, t.description));
            } else {
                tool_desc.push_str(&format!(
                    "- {}({}): {}\n",
                    t.name,
                    params.join(", "),
                    t.description
                ));
            }
        }
        tool_desc.push_str(
            "\nExample: TOOL_CALL:{\"name\":\"search_emails\",\"arguments\":{\"query\":\"security\"}}\n",
        );

        // Combine system + tool descriptions
        let full_system = match system {
            Some(s) => format!("{}\n\n{}", s, tool_desc),
            None => tool_desc,
        };

        // Build prompt from messages (Ollama uses single prompt string)
        let mut prompt_parts: Vec<String> = Vec::new();
        for m in messages {
            match m {
                LlmMessage::User(text) => prompt_parts.push(format!("User: {}", text)),
                LlmMessage::AssistantText(text) => {
                    prompt_parts.push(format!("Assistant: {}", text))
                }
                LlmMessage::AssistantToolCalls { text, tool_calls } => {
                    let mut part = String::from("Assistant: ");
                    if let Some(t) = text {
                        part.push_str(t);
                        part.push('\n');
                    }
                    for tc in tool_calls {
                        part.push_str(&format!(
                            "TOOL_CALL:{}",
                            serde_json::to_string(&serde_json::json!({
                                "name": tc.name,
                                "arguments": tc.arguments,
                            }))
                            .unwrap_or_default()
                        ));
                    }
                    prompt_parts.push(part);
                }
                LlmMessage::ToolResult { content, .. } => {
                    prompt_parts.push(format!("Tool result: {}", content));
                }
            }
        }
        prompt_parts.push("Assistant:".to_string());
        let prompt = prompt_parts.join("\n\n");

        // Call existing generate
        let response = self.generate(&self.model, &prompt, Some(&full_system)).await?;

        // Parse TOOL_CALL from response
        if let Some(tc_start) = response.find("TOOL_CALL:") {
            let json_str = &response[tc_start + "TOOL_CALL:".len()..];
            // Find the JSON object boundaries
            if let Some(obj_start) = json_str.find('{') {
                let remaining = &json_str[obj_start..];
                // Find matching closing brace
                let mut depth = 0;
                let mut end = 0;
                for (i, ch) in remaining.char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = i + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if end > 0 {
                    let json_obj = &remaining[..end];
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_obj) {
                        let name = parsed
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments = parsed
                            .get("arguments")
                            .cloned()
                            .unwrap_or(serde_json::json!({}));
                        if !name.is_empty() {
                            let text_before = response[..tc_start].trim().to_string();
                            return Some(LlmResponse::ToolCalls {
                                text: if text_before.is_empty() {
                                    None
                                } else {
                                    Some(text_before)
                                },
                                calls: vec![ToolCall {
                                    id: format!(
                                        "ollama_{}",
                                        uuid::Uuid::new_v4()
                                            .to_string()
                                            .split('-')
                                            .next()
                                            .unwrap_or("0")
                                    ),
                                    name,
                                    arguments,
                                }],
                            });
                        }
                    }
                }
            }
        }

        // No tool call found — return as text
        Some(LlmResponse::Text(response))
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
