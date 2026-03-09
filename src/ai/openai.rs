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

// ── Tool-use request/response types ────────────────────────────────

#[derive(Debug, Serialize)]
struct ToolChatRequest {
    model: String,
    messages: Vec<ToolChatMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OpenAITool>,
}

#[derive(Debug, Serialize, Clone)]
struct ToolChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCallOut>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAIFunction,
}

#[derive(Debug, Serialize)]
struct OpenAIFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize, Clone)]
struct OpenAIToolCallOut {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAIFunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String, // JSON string, NOT object
}

#[derive(Debug, Deserialize)]
struct ToolChatResponse {
    choices: Vec<ToolChoice>,
}

#[derive(Debug, Deserialize)]
struct ToolChoice {
    message: ToolChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ToolChoiceMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAIToolCallIn>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIToolCallIn {
    id: String,
    function: OpenAIFunctionCall,
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

    /// Generate a completion with tool-use support (native OpenAI function calling).
    pub async fn generate_with_tools(
        &self,
        messages: &[crate::ai::tools::LlmMessage],
        system: Option<&str>,
        tools: &[crate::ai::tools::Tool],
    ) -> Option<crate::ai::tools::LlmResponse> {
        use crate::ai::tools::{LlmMessage, LlmResponse, ToolCall};

        let mut api_messages: Vec<ToolChatMessage> = Vec::new();

        if let Some(sys) = system {
            api_messages.push(ToolChatMessage {
                role: "system".to_string(),
                content: Some(sys.to_string()),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        for m in messages {
            match m {
                LlmMessage::User(text) => {
                    api_messages.push(ToolChatMessage {
                        role: "user".to_string(),
                        content: Some(text.clone()),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                }
                LlmMessage::AssistantText(text) => {
                    api_messages.push(ToolChatMessage {
                        role: "assistant".to_string(),
                        content: Some(text.clone()),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                }
                LlmMessage::AssistantToolCalls { text, tool_calls } => {
                    let calls: Vec<OpenAIToolCallOut> = tool_calls
                        .iter()
                        .map(|tc| OpenAIToolCallOut {
                            id: tc.id.clone(),
                            call_type: "function".to_string(),
                            function: OpenAIFunctionCall {
                                name: tc.name.clone(),
                                arguments: serde_json::to_string(&tc.arguments)
                                    .unwrap_or_default(),
                            },
                        })
                        .collect();
                    api_messages.push(ToolChatMessage {
                        role: "assistant".to_string(),
                        content: text.clone(),
                        tool_calls: Some(calls),
                        tool_call_id: None,
                    });
                }
                LlmMessage::ToolResult {
                    tool_call_id,
                    content,
                } => {
                    api_messages.push(ToolChatMessage {
                        role: "tool".to_string(),
                        content: Some(content.clone()),
                        tool_calls: None,
                        tool_call_id: Some(tool_call_id.clone()),
                    });
                }
            }
        }

        let api_tools: Vec<OpenAITool> = tools
            .iter()
            .map(|t| OpenAITool {
                tool_type: "function".to_string(),
                function: OpenAIFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                },
            })
            .collect();

        let body = ToolChatRequest {
            model: self.model.clone(),
            messages: api_messages,
            max_tokens: 4096,
            tools: api_tools,
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
                match resp.json::<ToolChatResponse>().await {
                    Ok(r) => {
                        let choice = r.choices.into_iter().next()?;
                        let msg = choice.message;
                        match msg.tool_calls {
                            Some(calls) if !calls.is_empty() => {
                                let tool_calls: Vec<ToolCall> = calls
                                    .into_iter()
                                    .filter_map(|tc| {
                                        let args: serde_json::Value =
                                            serde_json::from_str(&tc.function.arguments).ok()?;
                                        Some(ToolCall {
                                            id: tc.id,
                                            name: tc.function.name,
                                            arguments: args,
                                        })
                                    })
                                    .collect();
                                if tool_calls.is_empty() {
                                    Some(LlmResponse::Text(msg.content.unwrap_or_default()))
                                } else {
                                    Some(LlmResponse::ToolCalls {
                                        text: msg.content,
                                        calls: tool_calls,
                                    })
                                }
                            }
                            _ => Some(LlmResponse::Text(msg.content.unwrap_or_default())),
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse OpenAI tool use response: {e}");
                        None
                    }
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body_text = resp.text().await.unwrap_or_default();
                tracing::warn!("OpenAI tool use returned {}: {}", status, body_text);
                None
            }
            Err(e) => {
                tracing::warn!("OpenAI tool use request failed: {e}");
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
