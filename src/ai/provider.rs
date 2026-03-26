use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;

use super::anthropic::AnthropicClient;
use super::ollama::OllamaClient;
use super::openai::OpenAIClient;

/// A single AI provider that can generate text completions.
#[derive(Clone)]
pub enum LlmProvider {
    Ollama(OllamaClient),
    Anthropic(AnthropicClient),
    OpenAI(OpenAIClient),
}

impl LlmProvider {
    pub fn name(&self) -> &str {
        match self {
            Self::Ollama(_) => "ollama",
            Self::Anthropic(_) => "anthropic",
            Self::OpenAI(_) => "openai",
        }
    }

    pub async fn health(&self) -> bool {
        match self {
            Self::Ollama(c) => c.health().await,
            Self::Anthropic(c) => c.health().await,
            Self::OpenAI(c) => c.health().await,
        }
    }

    pub async fn generate(&self, prompt: &str, system: Option<&str>) -> Option<String> {
        match self {
            Self::Ollama(c) => {
                // Ollama needs its model passed — stored on the client
                c.generate(&c.model, prompt, system).await
            }
            Self::Anthropic(c) => c.generate(prompt, system).await,
            Self::OpenAI(c) => c.generate(prompt, system).await,
        }
    }

    pub async fn generate_with_tools(
        &self,
        messages: &[crate::ai::tools::LlmMessage],
        system: Option<&str>,
        tools: &[crate::ai::tools::Tool],
    ) -> Option<crate::ai::tools::LlmResponse> {
        match self {
            Self::Ollama(c) => c.generate_with_tools(messages, system, tools).await,
            Self::Anthropic(c) => c.generate_with_tools(messages, system, tools).await,
            Self::OpenAI(c) => c.generate_with_tools(messages, system, tools).await,
        }
    }

    pub fn model(&self) -> &str {
        match self {
            Self::Ollama(c) => &c.model,
            Self::Anthropic(c) => &c.model,
            Self::OpenAI(c) => &c.model,
        }
    }
}

/// Provider status for API responses.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderStatus {
    pub name: String,
    pub model: String,
    pub healthy: bool,
}

/// Pool of LLM providers with round-robin load balancing.
/// Providers can be hot-reloaded via `reload()` without restarting the server.
#[derive(Clone)]
pub struct ProviderPool {
    providers: std::sync::Arc<RwLock<Vec<LlmProvider>>>,
    counter: std::sync::Arc<AtomicUsize>,
}

impl ProviderPool {
    pub fn new(providers: Vec<LlmProvider>) -> Self {
        Self {
            providers: std::sync::Arc::new(RwLock::new(providers)),
            counter: std::sync::Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Replace the provider list at runtime (e.g. after saving new API keys).
    pub fn reload(&self, providers: Vec<LlmProvider>) {
        let mut lock = self.providers.write().expect("provider pool lock poisoned");
        *lock = providers;
    }

    /// Snapshot current providers for iteration (releases the lock immediately).
    fn snapshot(&self) -> Vec<LlmProvider> {
        self.providers.read().expect("provider pool lock poisoned").clone()
    }

    /// Generate a completion using round-robin with fallback.
    /// Tries each provider starting from the next in rotation.
    pub async fn generate(&self, prompt: &str, system: Option<&str>) -> Option<String> {
        let providers = self.snapshot();
        let n = providers.len();
        if n == 0 {
            return None;
        }

        let start = self.counter.fetch_add(1, Ordering::Relaxed) % n;
        for i in 0..n {
            let idx = (start + i) % n;
            let provider = &providers[idx];
            if let Some(result) = provider.generate(prompt, system).await {
                return Some(result);
            }
            tracing::debug!("Provider {} failed, trying next", provider.name());
        }
        None
    }

    /// Generate a completion with tool-use support, using round-robin with fallback.
    pub async fn generate_with_tools(
        &self,
        messages: &[crate::ai::tools::LlmMessage],
        system: Option<&str>,
        tools: &[crate::ai::tools::Tool],
    ) -> Option<crate::ai::tools::LlmResponse> {
        let providers = self.snapshot();
        let n = providers.len();
        if n == 0 {
            return None;
        }

        let start = self.counter.fetch_add(1, Ordering::Relaxed) % n;
        for i in 0..n {
            let idx = (start + i) % n;
            let provider = &providers[idx];
            if let Some(result) = provider.generate_with_tools(messages, system, tools).await {
                return Some(result);
            }
            tracing::debug!("Provider {} (tools) failed, trying next", provider.name());
        }
        None
    }

    /// Check health of all providers concurrently.
    pub async fn health_status(&self) -> Vec<ProviderStatus> {
        let providers = self.snapshot();
        let futures: Vec<_> = providers.iter().map(|p| async {
            ProviderStatus {
                name: p.name().to_string(),
                model: p.model().to_string(),
                healthy: p.health().await,
            }
        }).collect();
        futures::future::join_all(futures).await
    }

    /// Whether any provider is configured.
    pub fn has_providers(&self) -> bool {
        !self.providers.read().expect("provider pool lock poisoned").is_empty()
    }

    /// Whether any provider reports healthy.
    pub async fn any_healthy(&self) -> bool {
        for p in &self.snapshot() {
            if p.health().await {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_pool() {
        let pool = ProviderPool::new(vec![]);
        assert!(!pool.has_providers());
    }

    #[test]
    fn test_pool_with_providers() {
        let ollama = OllamaClient::new("http://localhost:11434");
        let pool = ProviderPool::new(vec![LlmProvider::Ollama(ollama)]);
        assert!(pool.has_providers());
    }

    #[test]
    fn test_provider_names() {
        let ollama = LlmProvider::Ollama(OllamaClient::new("http://localhost:11434"));
        assert_eq!(ollama.name(), "ollama");

        let anthropic = LlmProvider::Anthropic(AnthropicClient::new("key", None));
        assert_eq!(anthropic.name(), "anthropic");

        let openai = LlmProvider::OpenAI(OpenAIClient::new("key", None));
        assert_eq!(openai.name(), "openai");
    }
}
