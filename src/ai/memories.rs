use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct MemoriesClient {
    client: Client,
    pub base_url: String,
    api_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpsertEntry {
    pub text: String,
    pub source: String,
    pub key: String,
}

#[derive(Debug, Serialize)]
struct UpsertRequest {
    text: String,
    source: String,
    key: String,
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
    pub score: f64,
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
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.request(method, &url);
        if let Some(ref key) = self.api_key {
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
    pub async fn upsert(&self, text: &str, source: &str, key: &str) -> bool {
        let body = UpsertRequest {
            text: text.to_string(),
            source: source.to_string(),
            key: key.to_string(),
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
    ) -> Vec<MemoryResult> {
        let body = SearchRequest {
            query: query.to_string(),
            k,
            hybrid: true,
            source: source_prefix.map(|s| s.to_string()),
        };

        match self.request(reqwest::Method::POST, "/search")
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<Vec<MemoryResult>>().await {
                    Ok(results) => results,
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
        assert_eq!(client.base_url, "http://localhost:8900");
        assert_eq!(client.api_key, Some("test-key".into()));
    }

    #[test]
    fn test_memories_client_no_trailing_slash() {
        let client = MemoriesClient::new("http://localhost:8900", None);
        assert_eq!(client.base_url, "http://localhost:8900");
        assert!(client.api_key.is_none());
    }

    #[test]
    fn test_upsert_entry_serialization() {
        let entry = UpsertEntry {
            text: "test content".into(),
            source: "iris/1/messages/abc".into(),
            key: "<abc@example.com>".into(),
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["text"], "test content");
        assert_eq!(json["source"], "iris/1/messages/abc");
        assert_eq!(json["key"], "<abc@example.com>");
    }

    #[test]
    fn test_search_request_serialization() {
        let req = SearchRequest {
            query: "budget concerns".into(),
            k: 10,
            hybrid: true,
            source: Some("iris/".into()),
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
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("source").is_none());
    }
}
