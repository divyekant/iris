use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::Duration;

struct MemoriesConfig {
    base_url: String,
    api_key: Option<String>,
}

#[derive(Clone)]
pub struct MemoriesClient {
    client: Client,
    config: Arc<RwLock<MemoriesConfig>>,
}

#[derive(Debug, Serialize)]
pub struct UpsertEntry {
    pub text: String,
    pub source: String,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct UpsertBatchRequest {
    entries: Vec<UpsertEntry>,
}

#[derive(Debug, Serialize)]
struct SearchRequest {
    query: String,
    k: usize,
    hybrid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    until: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    graph_weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recency_weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence_weight: Option<f64>,
}

#[derive(Default)]
pub struct SearchOptions {
    pub since: Option<String>,
    pub until: Option<String>,
    pub graph_weight: Option<f64>,
    pub recency_weight: Option<f64>,
    pub confidence_weight: Option<f64>,
}

#[derive(Debug, Serialize)]
struct DeleteBySourceRequest {
    source: String,
}

#[derive(Debug, Deserialize)]
pub struct MemoryResult {
    pub id: usize,
    pub text: String,
    pub source: String,
    #[serde(default)]
    pub score: f64,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    results: Vec<MemoryResult>,
}

#[derive(Debug, Deserialize)]
struct CountResponse {
    count: usize,
}

#[derive(Debug, Deserialize)]
struct DeleteResponse {
    deleted: usize,
}

#[derive(Debug, Deserialize)]
struct UpsertBatchResponse {
    stored: Option<usize>,
}

impl MemoriesClient {
    pub fn new(base_url: &str, api_key: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            config: Arc::new(RwLock::new(MemoriesConfig {
                base_url: base_url.trim_end_matches('/').to_string(),
                api_key,
            })),
        }
    }

    pub fn base_url(&self) -> String {
        self.config.read().unwrap().base_url.clone()
    }

    pub fn update_config(&self, base_url: &str, api_key: Option<String>) {
        let mut cfg = self.config.write().unwrap();
        cfg.base_url = base_url.trim_end_matches('/').to_string();
        cfg.api_key = api_key;
    }

    /// Resolve the effective base URL for internal requests.
    /// When running inside Docker, localhost URLs can't reach the host,
    /// so we substitute host.docker.internal automatically.
    fn effective_url(base: &str) -> String {
        if std::env::var("BIND_ALL").is_ok() {
            base.replace("://localhost", "://host.docker.internal")
                .replace("://127.0.0.1", "://host.docker.internal")
        } else {
            base.to_string()
        }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let cfg = self.config.read().unwrap();
        let base = Self::effective_url(&cfg.base_url);
        let url = format!("{}{}", base, path);
        let mut req = self.client.request(method, &url);
        if let Some(ref key) = cfg.api_key {
            req = req.header("X-API-Key", key);
        }
        req
    }

    /// Check if Memories server is reachable
    pub async fn health(&self) -> bool {
        match self.request(reqwest::Method::GET, "/health")
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Store or update a single memory entry
    pub async fn upsert(&self, text: &str, source: &str, key: &str, document_at: Option<&str>) -> bool {
        let body = UpsertEntry {
            text: text.to_string(),
            source: source.to_string(),
            key: key.to_string(),
            document_at: document_at.map(|s| s.to_string()),
        };

        match self.request(reqwest::Method::POST, "/memory/upsert")
            .json(&body)
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                tracing::warn!("Memories upsert failed: {e}");
                false
            }
        }
    }

    /// Batch upsert multiple memory entries, returns count stored
    pub async fn upsert_batch(&self, entries: Vec<UpsertEntry>) -> usize {
        if entries.is_empty() {
            return 0;
        }

        let body = UpsertBatchRequest { entries };

        match self.request(reqwest::Method::POST, "/memory/upsert-batch")
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<UpsertBatchResponse>().await {
                    Ok(r) => r.stored.unwrap_or(0),
                    Err(_) => 0,
                }
            }
            Ok(resp) => {
                tracing::warn!("Memories upsert_batch returned status {}", resp.status());
                0
            }
            Err(e) => {
                tracing::warn!("Memories upsert_batch failed: {e}");
                0
            }
        }
    }

    /// Hybrid BM25+vector search for relevant memories
    pub async fn search(
        &self,
        query: &str,
        k: usize,
        source_prefix: Option<&str>,
        options: SearchOptions,
    ) -> Vec<MemoryResult> {
        let body = SearchRequest {
            query: query.to_string(),
            k,
            hybrid: true,
            source: source_prefix.map(|s| s.to_string()),
            since: options.since,
            until: options.until,
            graph_weight: options.graph_weight,
            recency_weight: options.recency_weight,
            confidence_weight: options.confidence_weight,
        };

        match self.request(reqwest::Method::POST, "/search")
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<SearchResponse>().await {
                    Ok(r) => r.results,
                    Err(e) => {
                        tracing::warn!("Failed to parse Memories search response: {e}");
                        Vec::new()
                    }
                }
            }
            Ok(resp) => {
                tracing::warn!("Memories search returned status {}", resp.status());
                Vec::new()
            }
            Err(e) => {
                tracing::warn!("Memories search failed: {e}");
                Vec::new()
            }
        }
    }

    /// Delete all memories with a given source prefix
    pub async fn delete_by_source(&self, source: &str) -> usize {
        let body = DeleteBySourceRequest {
            source: source.to_string(),
        };

        match self.request(reqwest::Method::POST, "/memory/delete-by-source")
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<DeleteResponse>().await {
                    Ok(r) => r.deleted,
                    Err(_) => 0,
                }
            }
            _ => 0,
        }
    }

    /// Count memories, optionally filtered by source prefix
    pub async fn count(&self, source: Option<&str>) -> usize {
        let mut req = self.request(reqwest::Method::GET, "/memories/count")
            .timeout(Duration::from_secs(5));
        if let Some(s) = source {
            req = req.query(&[("source", s)]);
        }

        match req.send().await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<CountResponse>().await {
                    Ok(r) => r.count,
                    Err(_) => 0,
                }
            }
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memories_client_new() {
        let client = MemoriesClient::new("http://localhost:8900/", Some("test-key".into()));
        assert_eq!(client.base_url(), "http://localhost:8900");
        let cfg = client.config.read().unwrap();
        assert_eq!(cfg.api_key, Some("test-key".into()));
    }

    #[test]
    fn test_memories_client_no_trailing_slash() {
        let client = MemoriesClient::new("http://localhost:8900", None);
        assert_eq!(client.base_url(), "http://localhost:8900");
        let cfg = client.config.read().unwrap();
        assert!(cfg.api_key.is_none());
    }

    #[test]
    fn test_upsert_entry_serialization() {
        let entry = UpsertEntry {
            text: "test content".into(),
            source: "iris/1/messages/abc".into(),
            key: "<abc@example.com>".into(),
            document_at: None,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["text"], "test content");
        assert_eq!(json["source"], "iris/1/messages/abc");
        assert_eq!(json["key"], "<abc@example.com>");
        assert!(json.get("document_at").is_none());
    }

    #[test]
    fn test_upsert_request_with_document_at() {
        let req = UpsertEntry {
            text: "email body".into(),
            source: "iris/1/messages/abc".into(),
            key: "<abc@example.com>".into(),
            document_at: Some("2024-03-15T10:00:00Z".into()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["document_at"], "2024-03-15T10:00:00Z");
    }

    #[test]
    fn test_upsert_request_without_document_at() {
        let req = UpsertEntry {
            text: "email body".into(),
            source: "iris/1/messages/abc".into(),
            key: "<abc@example.com>".into(),
            document_at: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("document_at").is_none());
    }

    #[test]
    fn test_search_request_serialization() {
        let req = SearchRequest {
            query: "budget concerns".into(),
            k: 10,
            hybrid: true,
            source: Some("iris/".into()),
            since: None,
            until: None,
            graph_weight: None,
            recency_weight: None,
            confidence_weight: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["query"], "budget concerns");
        assert_eq!(json["k"], 10);
        assert_eq!(json["hybrid"], true);
        assert_eq!(json["source"], "iris/");
    }

    #[test]
    fn test_search_request_no_source_prefix() {
        let req = SearchRequest {
            query: "test".into(),
            k: 5,
            hybrid: true,
            source: None,
            since: None,
            until: None,
            graph_weight: None,
            recency_weight: None,
            confidence_weight: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("source").is_none());
    }

    #[test]
    fn test_search_request_with_v5_params() {
        let req = SearchRequest {
            query: "project deadline".into(),
            k: 5,
            hybrid: true,
            source: Some("iris/1/".into()),
            since: Some("2026-01-01".into()),
            until: Some("2026-03-31".into()),
            graph_weight: Some(0.1),
            recency_weight: Some(0.0),
            confidence_weight: Some(0.0),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["since"], "2026-01-01");
        assert_eq!(json["until"], "2026-03-31");
        assert_eq!(json["graph_weight"], 0.1);
        assert_eq!(json["recency_weight"], 0.0);
        assert_eq!(json["confidence_weight"], 0.0);
    }

    #[test]
    fn test_search_options_default() {
        let opts = SearchOptions::default();
        assert!(opts.since.is_none());
        assert!(opts.until.is_none());
        assert!(opts.graph_weight.is_none());
        assert!(opts.recency_weight.is_none());
        assert!(opts.confidence_weight.is_none());
    }
}
