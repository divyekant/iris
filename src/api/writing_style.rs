use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingStyleTrait {
    pub id: String,
    pub account_id: String,
    pub trait_type: String,
    pub trait_value: String,
    pub confidence: f64,
    pub examples: Option<Vec<String>>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct StyleResponse {
    pub traits: Vec<WritingStyleTrait>,
    pub account_id: String,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeResponse {
    pub traits: Vec<WritingStyleTrait>,
    pub emails_analyzed: usize,
}

// ---------------------------------------------------------------------------
// GET /api/style/{account_id}
// ---------------------------------------------------------------------------

pub async fn get_style(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<StyleResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let traits = load_style_traits(&conn, &account_id);

    Ok(Json(StyleResponse {
        traits,
        account_id,
    }))
}

// ---------------------------------------------------------------------------
// POST /api/style/{account_id}/analyze
// ---------------------------------------------------------------------------

pub async fn analyze_style(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<AnalyzeResponse>, StatusCode> {
    // Verify AI is enabled
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

    // Fetch last 200 sent emails for this account
    let sent_emails: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(subject, ''), COALESCE(body_text, '')
                 FROM messages
                 WHERE account_id = ?1 AND folder = 'Sent'
                 ORDER BY date DESC
                 LIMIT 200",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        stmt.query_map(rusqlite::params![account_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .filter(|(_, body)| !body.trim().is_empty())
        .collect()
    };

    if sent_emails.is_empty() {
        return Ok(Json(AnalyzeResponse {
            traits: vec![],
            emails_analyzed: 0,
        }));
    }

    let emails_analyzed = sent_emails.len();

    // Build a sample of email bodies for analysis (cap at ~6000 chars)
    let mut sample = String::new();
    let max_sample = 6000;
    for (i, (subject, body)) in sent_emails.iter().enumerate().take(30) {
        let body_truncated: String = body.chars().take(300).collect();
        let entry = format!(
            "--- Email {} ---\nSubject: {}\n{}\n\n",
            i + 1,
            subject,
            body_truncated
        );
        if sample.len() + entry.len() > max_sample {
            break;
        }
        sample.push_str(&entry);
    }

    let prompt = format!(
        r#"Analyze the writing style of these sent emails and extract the following traits. Return ONLY a JSON array of objects, no markdown fences:

[
  {{"trait_type": "greeting", "trait_value": "most common greeting phrase", "confidence": 0.0-1.0, "examples": ["example1", "example2"]}},
  {{"trait_type": "signoff", "trait_value": "most common sign-off phrase", "confidence": 0.0-1.0, "examples": ["example1"]}},
  {{"trait_type": "tone", "trait_value": "one of: formal, semi-formal, casual, direct, warm, analytical", "confidence": 0.0-1.0, "examples": ["short excerpt showing tone"]}},
  {{"trait_type": "avg_length", "trait_value": "short (1-3 sentences) | medium (4-8 sentences) | long (9+ sentences)", "confidence": 0.0-1.0, "examples": []}},
  {{"trait_type": "formality", "trait_value": "score from 1 (very casual) to 10 (very formal)", "confidence": 0.0-1.0, "examples": ["excerpt showing formality level"]}},
  {{"trait_type": "vocabulary", "trait_value": "description of vocabulary patterns, e.g. uses technical jargon, simple words, etc.", "confidence": 0.0-1.0, "examples": ["characteristic phrase1", "characteristic phrase2"]}}
]

Sent emails to analyze:

{sample}"#
    );

    let system = "You are a writing style analyst. Analyze email writing patterns and extract style traits. Return ONLY valid JSON, no explanations.";

    let raw = state
        .providers
        .generate(&prompt, Some(system))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    // Parse the AI response
    let traits = parse_style_traits(&raw, &account_id);

    // Store traits in DB (replace existing)
    let conn2 = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    store_style_traits(&conn2, &account_id, &traits);

    // Record last analysis time
    conn2
        .execute(
            "INSERT INTO config (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            rusqlite::params![
                format!("style_last_analyzed_{}", account_id),
                chrono::Utc::now().timestamp().to_string()
            ],
        )
        .ok();

    Ok(Json(AnalyzeResponse {
        traits,
        emails_analyzed,
    }))
}

// ---------------------------------------------------------------------------
// Style prompt builder — injected into draft generation prompts
// ---------------------------------------------------------------------------

/// Build a style instruction snippet for AI draft prompts.
/// Returns None if no style traits exist for the account.
pub fn build_style_prompt(conn: &Connection, account_id: &str) -> Option<String> {
    let traits = load_style_traits(conn, account_id);
    if traits.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    for t in &traits {
        match t.trait_type.as_str() {
            "greeting" => parts.push(format!("They typically greet with '{}'", t.trait_value)),
            "signoff" => parts.push(format!("sign off with '{}'", t.trait_value)),
            "tone" => parts.push(format!("use a {} tone", t.trait_value)),
            "avg_length" => parts.push(format!("write {} emails", t.trait_value)),
            "formality" => parts.push(format!("formality level {}/10", t.trait_value)),
            "vocabulary" => parts.push(format!("vocabulary: {}", t.trait_value)),
            _ => {}
        }
    }

    if parts.is_empty() {
        return None;
    }

    Some(format!(
        "Write in the user's style: {}.",
        parts.join(", ")
    ))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_style_traits(conn: &Connection, account_id: &str) -> Vec<WritingStyleTrait> {
    let mut stmt = match conn.prepare(
        "SELECT id, account_id, trait_type, trait_value, confidence, examples, created_at, updated_at
         FROM writing_style
         WHERE account_id = ?1
         ORDER BY trait_type ASC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map(rusqlite::params![account_id], |row| {
        let examples_json: Option<String> = row.get(5)?;
        let examples: Option<Vec<String>> = examples_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok());

        Ok(WritingStyleTrait {
            id: row.get(0)?,
            account_id: row.get(1)?,
            trait_type: row.get(2)?,
            trait_value: row.get(3)?,
            confidence: row.get(4)?,
            examples,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })
    .ok()
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

fn store_style_traits(conn: &Connection, account_id: &str, traits: &[WritingStyleTrait]) {
    // Delete existing traits for this account
    conn.execute(
        "DELETE FROM writing_style WHERE account_id = ?1",
        rusqlite::params![account_id],
    )
    .ok();

    for t in traits {
        let examples_json = t
            .examples
            .as_ref()
            .and_then(|e| serde_json::to_string(e).ok());

        conn.execute(
            "INSERT INTO writing_style (id, account_id, trait_type, trait_value, confidence, examples)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![t.id, account_id, t.trait_type, t.trait_value, t.confidence, examples_json],
        )
        .ok();
    }
}

fn parse_style_traits(raw: &str, account_id: &str) -> Vec<WritingStyleTrait> {
    let trimmed = raw.trim();

    // Strip markdown fences if present
    let json_str = if trimmed.starts_with("```") {
        trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        trimmed
    };

    // Try to find JSON array
    let json_str = if let Some(start) = json_str.find('[') {
        if let Some(end) = json_str.rfind(']') {
            &json_str[start..=end]
        } else {
            json_str
        }
    } else {
        json_str
    };

    #[derive(Deserialize)]
    struct RawTrait {
        trait_type: String,
        trait_value: String,
        #[serde(default = "default_confidence")]
        confidence: f64,
        #[serde(default)]
        examples: Vec<String>,
    }

    fn default_confidence() -> f64 {
        0.5
    }

    let valid_types = ["greeting", "signoff", "tone", "avg_length", "formality", "vocabulary"];

    match serde_json::from_str::<Vec<RawTrait>>(json_str) {
        Ok(raw_traits) => raw_traits
            .into_iter()
            .filter(|t| valid_types.contains(&t.trait_type.as_str()))
            .map(|t| {
                let confidence = t.confidence.clamp(0.0, 1.0);
                WritingStyleTrait {
                    id: uuid::Uuid::new_v4().to_string(),
                    account_id: account_id.to_string(),
                    trait_type: t.trait_type,
                    trait_value: t.trait_value,
                    confidence,
                    examples: if t.examples.is_empty() {
                        None
                    } else {
                        Some(t.examples)
                    },
                    created_at: chrono::Utc::now().timestamp(),
                    updated_at: chrono::Utc::now().timestamp(),
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Public worker-facing helpers
// ---------------------------------------------------------------------------

/// Parse style traits from AI output — callable from worker.
pub fn parse_style_traits_for_worker(raw: &str, account_id: &str) -> Vec<WritingStyleTrait> {
    parse_style_traits(raw, account_id)
}

/// Store style traits — callable from worker.
pub fn store_style_traits_for_worker(conn: &Connection, account_id: &str, traits: &[WritingStyleTrait]) {
    store_style_traits(conn, account_id, traits)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS accounts (id TEXT PRIMARY KEY, email TEXT, is_active INTEGER DEFAULT 1);
             CREATE TABLE IF NOT EXISTS config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS writing_style (
                 id TEXT PRIMARY KEY,
                 account_id TEXT NOT NULL,
                 trait_type TEXT NOT NULL CHECK(trait_type IN ('greeting', 'signoff', 'tone', 'avg_length', 'formality', 'vocabulary')),
                 trait_value TEXT NOT NULL,
                 confidence REAL DEFAULT 0.5,
                 examples TEXT,
                 created_at INTEGER DEFAULT (unixepoch()),
                 updated_at INTEGER DEFAULT (unixepoch()),
                 FOREIGN KEY(account_id) REFERENCES accounts(id)
             );
             CREATE INDEX IF NOT EXISTS idx_writing_style_account ON writing_style(account_id);
             INSERT INTO accounts (id, email) VALUES ('acc1', 'test@example.com');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_store_and_load_traits() {
        let conn = setup_db();
        let traits = vec![
            WritingStyleTrait {
                id: "t1".to_string(),
                account_id: "acc1".to_string(),
                trait_type: "greeting".to_string(),
                trait_value: "Hey team,".to_string(),
                confidence: 0.85,
                examples: Some(vec!["Hey team,".to_string(), "Hi all,".to_string()]),
                created_at: 1000,
                updated_at: 1000,
            },
            WritingStyleTrait {
                id: "t2".to_string(),
                account_id: "acc1".to_string(),
                trait_type: "signoff".to_string(),
                trait_value: "Best, D".to_string(),
                confidence: 0.9,
                examples: None,
                created_at: 1000,
                updated_at: 1000,
            },
        ];

        store_style_traits(&conn, "acc1", &traits);
        let loaded = load_style_traits(&conn, "acc1");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].trait_type, "greeting");
        assert_eq!(loaded[1].trait_type, "signoff");
    }

    #[test]
    fn test_build_style_prompt_with_traits() {
        let conn = setup_db();
        let traits = vec![
            WritingStyleTrait {
                id: "t1".to_string(),
                account_id: "acc1".to_string(),
                trait_type: "greeting".to_string(),
                trait_value: "Hey team,".to_string(),
                confidence: 0.85,
                examples: None,
                created_at: 1000,
                updated_at: 1000,
            },
            WritingStyleTrait {
                id: "t2".to_string(),
                account_id: "acc1".to_string(),
                trait_type: "tone".to_string(),
                trait_value: "direct".to_string(),
                confidence: 0.8,
                examples: None,
                created_at: 1000,
                updated_at: 1000,
            },
        ];

        store_style_traits(&conn, "acc1", &traits);
        let prompt = build_style_prompt(&conn, "acc1");
        assert!(prompt.is_some());
        let prompt = prompt.unwrap();
        assert!(prompt.contains("Hey team,"));
        assert!(prompt.contains("direct tone"));
    }

    #[test]
    fn test_build_style_prompt_empty() {
        let conn = setup_db();
        let prompt = build_style_prompt(&conn, "acc1");
        assert!(prompt.is_none());
    }

    #[test]
    fn test_parse_style_traits_valid_json() {
        let raw = r#"[
            {"trait_type": "greeting", "trait_value": "Hi there,", "confidence": 0.9, "examples": ["Hi there,"]},
            {"trait_type": "tone", "trait_value": "casual", "confidence": 0.75, "examples": []}
        ]"#;

        let traits = parse_style_traits(raw, "acc1");
        assert_eq!(traits.len(), 2);
        assert_eq!(traits[0].trait_type, "greeting");
        assert_eq!(traits[0].trait_value, "Hi there,");
        assert_eq!(traits[1].trait_type, "tone");
    }

    #[test]
    fn test_parse_style_traits_with_fences() {
        let raw = "```json\n[{\"trait_type\": \"signoff\", \"trait_value\": \"Cheers\", \"confidence\": 0.8}]\n```";
        let traits = parse_style_traits(raw, "acc1");
        assert_eq!(traits.len(), 1);
        assert_eq!(traits[0].trait_type, "signoff");
    }

    #[test]
    fn test_parse_style_traits_invalid_type_filtered() {
        let raw = r#"[{"trait_type": "invalid_type", "trait_value": "test", "confidence": 0.5}]"#;
        let traits = parse_style_traits(raw, "acc1");
        assert_eq!(traits.len(), 0);
    }

    #[test]
    fn test_parse_style_traits_clamps_confidence() {
        let raw = r#"[{"trait_type": "greeting", "trait_value": "Hello", "confidence": 1.5}]"#;
        let traits = parse_style_traits(raw, "acc1");
        assert_eq!(traits.len(), 1);
        assert!(traits[0].confidence <= 1.0);
    }

    #[test]
    fn test_store_replaces_existing() {
        let conn = setup_db();
        let traits1 = vec![WritingStyleTrait {
            id: "t1".to_string(),
            account_id: "acc1".to_string(),
            trait_type: "greeting".to_string(),
            trait_value: "Hi,".to_string(),
            confidence: 0.5,
            examples: None,
            created_at: 1000,
            updated_at: 1000,
        }];
        store_style_traits(&conn, "acc1", &traits1);
        assert_eq!(load_style_traits(&conn, "acc1").len(), 1);

        let traits2 = vec![
            WritingStyleTrait {
                id: "t2".to_string(),
                account_id: "acc1".to_string(),
                trait_type: "greeting".to_string(),
                trait_value: "Hey,".to_string(),
                confidence: 0.7,
                examples: None,
                created_at: 2000,
                updated_at: 2000,
            },
            WritingStyleTrait {
                id: "t3".to_string(),
                account_id: "acc1".to_string(),
                trait_type: "tone".to_string(),
                trait_value: "warm".to_string(),
                confidence: 0.6,
                examples: None,
                created_at: 2000,
                updated_at: 2000,
            },
        ];
        store_style_traits(&conn, "acc1", &traits2);
        let loaded = load_style_traits(&conn, "acc1");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].trait_value, "Hey,");
    }
}
