use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::AppState;

// --- Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub canonical_name: String,
    pub entity_type: String,
    pub confidence: f64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityAlias {
    pub id: String,
    pub entity_id: String,
    pub alias_name: String,
    pub source_message_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRelation {
    pub id: String,
    pub entity_a: String,
    pub entity_b: String,
    pub relation_type: String,
    pub weight: f64,
    pub source_message_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct EntityWithDetails {
    pub entity: Entity,
    pub aliases: Vec<String>,
    pub relations: Vec<RelatedEntity>,
    pub connected_threads: Vec<ConnectedThread>,
}

#[derive(Debug, Serialize)]
pub struct RelatedEntity {
    pub entity_id: String,
    pub canonical_name: String,
    pub entity_type: String,
    pub relation_type: String,
    pub weight: f64,
}

#[derive(Debug, Serialize)]
pub struct ConnectedThread {
    pub thread_id: String,
    pub subject: String,
    pub date: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ExtractResponse {
    pub message_id: String,
    pub entities_created: usize,
    pub relations_created: usize,
    pub events_created: usize,
}

#[derive(Debug, Serialize)]
pub struct GraphQueryResponse {
    pub results: Vec<EntityWithDetails>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct GraphQueryParams {
    pub query: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EntityListParams {
    #[serde(rename = "type")]
    pub entity_type: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct EntityListResponse {
    pub entities: Vec<Entity>,
    pub total: i64,
}

// --- AI extraction types ---

#[derive(Debug, Deserialize)]
struct AiEntity {
    name: String,
    #[serde(rename = "type")]
    entity_type: String,
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AiRelation {
    from: String,
    to: String,
    relation: String,
}

#[derive(Debug, Deserialize)]
struct AiEvent {
    name: String,
    date: String,
    #[serde(default = "default_precision")]
    precision: String,
}

fn default_precision() -> String {
    "day".to_string()
}

#[derive(Debug, Deserialize)]
struct AiExtractionResult {
    #[serde(default)]
    entities: Vec<AiEntity>,
    #[serde(default)]
    relations: Vec<AiRelation>,
    #[serde(default)]
    events: Vec<AiEvent>,
}

const VALID_ENTITY_TYPES: &[&str] = &["person", "org", "project", "date", "amount"];
const VALID_PRECISIONS: &[&str] = &["day", "week", "month", "quarter", "year"];

fn normalize_entity_type(t: &str) -> Option<&'static str> {
    let lower = t.to_lowercase();
    VALID_ENTITY_TYPES.iter().find(|&&v| v == lower).copied()
}

fn normalize_precision(p: &str) -> &'static str {
    let lower = p.to_lowercase();
    VALID_PRECISIONS
        .iter()
        .find(|&&v| v == lower)
        .copied()
        .unwrap_or("day")
}

// --- AI prompt ---

fn build_extraction_prompt(subject: &str, from: &str, body: &str) -> String {
    format!(
        r#"Extract entities, relationships, and date events from this email. Return a JSON object with these fields:

1. "entities": array of {{ "name": string, "type": "person"|"org"|"project"|"date"|"amount", "aliases": [string] }}
   - Use email addresses to identify people (e.g., "john@acme.com" -> person "John" with alias "john@acme.com")
   - "org" = company/organization, "project" = product/project/initiative

2. "relations": array of {{ "from": string, "to": string, "relation": string }}
   - from/to are entity names from the entities array
   - relation examples: "works_at", "manages", "collaborates_with", "part_of", "reports_to"

3. "events": array of {{ "name": string, "date": "YYYY-MM-DD", "precision": "day"|"week"|"month"|"quarter"|"year" }}
   - Extract date-anchored events like launches, deadlines, meetings
   - Use approximate dates when exact dates aren't given

Return an empty arrays if nothing is found. Respond ONLY with the JSON object.

From: {from}
Subject: {subject}

Body:
{body}"#
    )
}

const EXTRACTION_SYSTEM_PROMPT: &str =
    "You are an entity and relationship extraction engine for emails. Extract people, organizations, projects, monetary amounts, and their relationships. Also extract date-anchored events. Return ONLY valid JSON.";

async fn extract_with_ai(
    providers: &crate::ai::provider::ProviderPool,
    subject: &str,
    from: &str,
    body: &str,
) -> Option<AiExtractionResult> {
    let prompt = build_extraction_prompt(subject, from, body);
    let response = providers
        .generate(&prompt, Some(EXTRACTION_SYSTEM_PROMPT))
        .await?;

    let trimmed = response.trim();
    let json_str = if trimmed.starts_with("```") {
        trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        trimmed
    };

    serde_json::from_str::<AiExtractionResult>(json_str).ok()
}

/// Find or create an entity by canonical name, returning its ID.
pub fn find_or_create_entity(
    conn: &rusqlite::Connection,
    name: &str,
    entity_type: &str,
    message_id: &str,
) -> Option<String> {
    // Check for existing entity by canonical name (case-insensitive)
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM entities WHERE canonical_name = ?1 COLLATE NOCASE AND entity_type = ?2",
            rusqlite::params![name, entity_type],
            |row| row.get(0),
        )
        .ok();

    if let Some(id) = existing {
        // Update timestamp
        conn.execute(
            "UPDATE entities SET updated_at = unixepoch() WHERE id = ?1",
            rusqlite::params![id],
        )
        .ok();
        return Some(id);
    }

    // Also check aliases
    let from_alias: Option<String> = conn
        .query_row(
            "SELECT e.id FROM entities e JOIN entity_aliases ea ON e.id = ea.entity_id WHERE ea.alias_name = ?1 COLLATE NOCASE",
            rusqlite::params![name],
            |row| row.get(0),
        )
        .ok();

    if let Some(id) = from_alias {
        conn.execute(
            "UPDATE entities SET updated_at = unixepoch() WHERE id = ?1",
            rusqlite::params![id],
        )
        .ok();
        return Some(id);
    }

    // Create new entity
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO entities (id, canonical_name, entity_type) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, name, entity_type],
    )
    .ok()?;

    // Add an alias for the canonical name itself, anchored to the source message
    let alias_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO entity_aliases (id, entity_id, alias_name, source_message_id) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![alias_id, id, name, message_id],
    )
    .ok();

    Some(id)
}

// --- Handlers ---

/// POST /api/graph/extract/{message_id} -- extract entities from a message
pub async fn extract_entities(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> Result<Json<ExtractResponse>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Fetch the message
    let (subject, from_address, body_text): (String, String, String) = conn
        .query_row(
            "SELECT COALESCE(subject, ''), COALESCE(from_address, ''), COALESCE(body_text, '') FROM messages WHERE id = ?1",
            rusqlite::params![message_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let body_truncated: String = body_text.chars().take(4000).collect();

    // Run AI extraction
    let result = extract_with_ai(&state.providers, &subject, &from_address, &body_truncated).await;

    let extraction = match result {
        Some(r) => r,
        None => {
            return Ok(Json(ExtractResponse {
                message_id,
                entities_created: 0,
                relations_created: 0,
                events_created: 0,
            }));
        }
    };

    let mut entities_created = 0usize;
    let mut relations_created = 0usize;
    let mut events_created = 0usize;

    // Map entity names to IDs for relation wiring
    let mut name_to_id: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    // Process entities
    for ai_entity in &extraction.entities {
        let entity_type = match normalize_entity_type(&ai_entity.entity_type) {
            Some(t) => t,
            None => continue,
        };

        if let Some(entity_id) = find_or_create_entity(&conn, &ai_entity.name, entity_type, &message_id) {
            name_to_id.insert(ai_entity.name.to_lowercase(), entity_id.clone());
            entities_created += 1;

            // Add aliases
            for alias in &ai_entity.aliases {
                if alias.is_empty() {
                    continue;
                }
                // Check if alias already exists for this entity
                let exists: bool = conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM entity_aliases WHERE entity_id = ?1 AND alias_name = ?2 COLLATE NOCASE)",
                        rusqlite::params![entity_id, alias],
                        |row| row.get(0),
                    )
                    .unwrap_or(true);

                if !exists {
                    let alias_id = Uuid::new_v4().to_string();
                    conn.execute(
                        "INSERT INTO entity_aliases (id, entity_id, alias_name, source_message_id) VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![alias_id, entity_id, alias, message_id],
                    )
                    .ok();
                }
            }
        }
    }

    // Process relations
    for ai_rel in &extraction.relations {
        let entity_a = name_to_id.get(&ai_rel.from.to_lowercase());
        let entity_b = name_to_id.get(&ai_rel.to.to_lowercase());

        if let (Some(a_id), Some(b_id)) = (entity_a, entity_b) {
            // Check for existing relation
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM entity_relations WHERE entity_a = ?1 AND entity_b = ?2 AND relation_type = ?3)",
                    rusqlite::params![a_id, b_id, ai_rel.relation],
                    |row| row.get(0),
                )
                .unwrap_or(true);

            if exists {
                // Increase weight on existing relation
                conn.execute(
                    "UPDATE entity_relations SET weight = weight + 0.5 WHERE entity_a = ?1 AND entity_b = ?2 AND relation_type = ?3",
                    rusqlite::params![a_id, b_id, ai_rel.relation],
                )
                .ok();
            } else {
                let rel_id = Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO entity_relations (id, entity_a, entity_b, relation_type, source_message_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![rel_id, a_id, b_id, ai_rel.relation, message_id],
                )
                .ok();
                relations_created += 1;
            }
        }
    }

    // Process events -> timeline_events
    for ai_event in &extraction.events {
        if ai_event.name.is_empty() || ai_event.date.is_empty() {
            continue;
        }

        let precision = normalize_precision(&ai_event.precision);

        // Check for duplicate events
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM timeline_events WHERE event_name = ?1 COLLATE NOCASE AND approximate_date = ?2)",
                rusqlite::params![ai_event.name, ai_event.date],
                |row| row.get(0),
            )
            .unwrap_or(true);

        if !exists {
            let event_id = Uuid::new_v4().to_string();
            // Fetch account_id for the message
            let account_id: Option<String> = conn
                .query_row(
                    "SELECT account_id FROM messages WHERE id = ?1",
                    rusqlite::params![message_id],
                    |row| row.get(0),
                )
                .ok();

            conn.execute(
                "INSERT INTO timeline_events (id, event_name, approximate_date, date_precision, source_message_id, account_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![event_id, ai_event.name, ai_event.date, precision, message_id, account_id],
            )
            .ok();
            events_created += 1;
        }
    }

    Ok(Json(ExtractResponse {
        message_id,
        entities_created,
        relations_created,
        events_created,
    }))
}

/// GET /api/graph?query=... -- search entities and return with relations
pub async fn query_graph(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GraphQueryParams>,
) -> Result<Json<GraphQueryResponse>, StatusCode> {
    let query = match params.query {
        Some(q) if !q.trim().is_empty() => q.trim().to_string(),
        _ => return Ok(Json(GraphQueryResponse { results: Vec::new(), total: 0 })),
    };

    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let search_pattern = format!("%{}%", query);

    // Find entities matching by canonical name or alias
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT e.id, e.canonical_name, e.entity_type, e.confidence, e.created_at, e.updated_at
             FROM entities e
             LEFT JOIN entity_aliases ea ON e.id = ea.entity_id
             WHERE e.canonical_name LIKE ?1 COLLATE NOCASE
                OR ea.alias_name LIKE ?1 COLLATE NOCASE
             ORDER BY e.updated_at DESC
             LIMIT 20",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let entities: Vec<Entity> = stmt
        .query_map(rusqlite::params![search_pattern], |row| {
            Ok(Entity {
                id: row.get(0)?,
                canonical_name: row.get(1)?,
                entity_type: row.get(2)?,
                confidence: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let mut results = Vec::new();

    for entity in &entities {
        let details = build_entity_details(&conn, entity)?;
        results.push(details);
    }

    let total = results.len();
    Ok(Json(GraphQueryResponse { results, total }))
}

/// GET /api/graph/entities?type=person&limit=20 -- list entities
pub async fn list_entities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<EntityListParams>,
) -> Result<Json<EntityListResponse>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let limit = params.limit.unwrap_or(20).min(100);

    let (sql, count_sql) = if let Some(ref entity_type) = params.entity_type {
        if !VALID_ENTITY_TYPES.contains(&entity_type.as_str()) {
            return Err(StatusCode::BAD_REQUEST);
        }
        (
            "SELECT id, canonical_name, entity_type, confidence, created_at, updated_at FROM entities WHERE entity_type = ?1 ORDER BY updated_at DESC LIMIT ?2".to_string(),
            "SELECT COUNT(*) FROM entities WHERE entity_type = ?1".to_string(),
        )
    } else {
        (
            "SELECT id, canonical_name, entity_type, confidence, created_at, updated_at FROM entities ORDER BY updated_at DESC LIMIT ?1".to_string(),
            "SELECT COUNT(*) FROM entities".to_string(),
        )
    };

    let total: i64 = if let Some(ref et) = params.entity_type {
        conn.query_row(&count_sql, rusqlite::params![et], |row| row.get(0))
            .unwrap_or(0)
    } else {
        conn.query_row(&count_sql, [], |row| row.get(0))
            .unwrap_or(0)
    };

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let entities: Vec<Entity> = if let Some(ref et) = params.entity_type {
        stmt.query_map(rusqlite::params![et, limit], |row| {
            Ok(Entity {
                id: row.get(0)?,
                canonical_name: row.get(1)?,
                entity_type: row.get(2)?,
                confidence: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map(rusqlite::params![limit], |row| {
            Ok(Entity {
                id: row.get(0)?,
                canonical_name: row.get(1)?,
                entity_type: row.get(2)?,
                confidence: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect()
    };

    Ok(Json(EntityListResponse { entities, total }))
}

// --- Helpers ---

fn build_entity_details(
    conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    entity: &Entity,
) -> Result<EntityWithDetails, StatusCode> {
    // Fetch aliases
    let mut alias_stmt = conn
        .prepare("SELECT alias_name FROM entity_aliases WHERE entity_id = ?1")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let aliases: Vec<String> = alias_stmt
        .query_map(rusqlite::params![entity.id], |row| row.get(0))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    // Fetch relations (where entity is either side)
    let mut rel_stmt = conn
        .prepare(
            "SELECT r.entity_a, r.entity_b, r.relation_type, r.weight,
                    CASE WHEN r.entity_a = ?1 THEN eb.id ELSE ea.id END as other_id,
                    CASE WHEN r.entity_a = ?1 THEN eb.canonical_name ELSE ea.canonical_name END as other_name,
                    CASE WHEN r.entity_a = ?1 THEN eb.entity_type ELSE ea.entity_type END as other_type
             FROM entity_relations r
             JOIN entities ea ON r.entity_a = ea.id
             JOIN entities eb ON r.entity_b = eb.id
             WHERE r.entity_a = ?1 OR r.entity_b = ?1
             ORDER BY r.weight DESC
             LIMIT 20",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let relations: Vec<RelatedEntity> = rel_stmt
        .query_map(rusqlite::params![entity.id], |row| {
            Ok(RelatedEntity {
                entity_id: row.get(4)?,
                canonical_name: row.get(5)?,
                entity_type: row.get(6)?,
                relation_type: row.get(2)?,
                weight: row.get(3)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    // Fetch connected threads via source_message_id on aliases and relations
    let mut thread_stmt = conn
        .prepare(
            "SELECT DISTINCT m.thread_id, COALESCE(m.subject, '(no subject)'), m.date
             FROM messages m
             WHERE m.id IN (
                SELECT source_message_id FROM entity_aliases WHERE entity_id = ?1 AND source_message_id IS NOT NULL
                UNION
                SELECT source_message_id FROM entity_relations WHERE (entity_a = ?1 OR entity_b = ?1) AND source_message_id IS NOT NULL
             )
             AND m.thread_id IS NOT NULL
             ORDER BY m.date DESC
             LIMIT 10",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let connected_threads: Vec<ConnectedThread> = thread_stmt
        .query_map(rusqlite::params![entity.id], |row| {
            Ok(ConnectedThread {
                thread_id: row.get(0)?,
                subject: row.get(1)?,
                date: row.get(2)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(EntityWithDetails {
        entity: entity.clone(),
        aliases,
        relations,
        connected_threads,
    })
}

// --- Job integration ---

/// Enqueue entity extraction for a message.
pub fn enqueue_entity_extract(conn: &rusqlite::Connection, message_id: &str) {
    // Deduplicate: skip if already pending/processing
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM processing_jobs WHERE job_type = 'entity_extract' AND message_id = ?1 AND status IN ('pending','processing'))",
            rusqlite::params![message_id],
            |row| row.get(0),
        )
        .unwrap_or(true);

    if exists {
        return;
    }

    if let Err(e) = conn.execute(
        "INSERT INTO processing_jobs (job_type, message_id) VALUES ('entity_extract', ?1)",
        rusqlite::params![message_id],
    ) {
        tracing::warn!("Failed to enqueue entity_extract: {e}");
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};
    use crate::models::message::InsertMessage;

    fn setup_test_message(body: &str, subject: &str) -> (crate::db::DbPool, String, String) {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account = Account::create(
            &conn,
            &CreateAccount {
                provider: "gmail".to_string(),
                email: "test@example.com".to_string(),
                display_name: Some("Test User".to_string()),
                imap_host: Some("imap.gmail.com".to_string()),
                imap_port: Some(993),
                smtp_host: Some("smtp.gmail.com".to_string()),
                smtp_port: Some(587),
                username: Some("test@example.com".to_string()),
                password: Some("secret".to_string()),
            },
        );

        let msg = InsertMessage {
            account_id: account.id.clone(),
            message_id: Some("<test-msg@example.com>".to_string()),
            thread_id: Some("thread-1".to_string()),
            folder: "INBOX".to_string(),
            from_address: Some("sender@example.com".to_string()),
            from_name: Some("Sender".to_string()),
            to_addresses: Some("[\"test@example.com\"]".to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(1700000000),
            snippet: Some(body.chars().take(200).collect()),
            body_text: Some(body.to_string()),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(1),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: None,
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        };

        let msg_id = InsertMessage::insert(&conn, &msg).expect("insert should succeed");
        (pool, account.id, msg_id)
    }

    #[test]
    fn test_entities_table_exists() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='entities'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_entity_aliases_table_exists() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='entity_aliases'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_entity_relations_table_exists() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='entity_relations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_timeline_events_table_exists() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='timeline_events'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_find_or_create_entity_new() {
        let (pool, _account_id, msg_id) = setup_test_message("Hello world", "Test");
        let conn = pool.get().unwrap();

        let entity_id = find_or_create_entity(&conn, "Acme Corp", "org", &msg_id);
        assert!(entity_id.is_some());

        let name: String = conn
            .query_row(
                "SELECT canonical_name FROM entities WHERE id = ?1",
                rusqlite::params![entity_id.unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "Acme Corp");
    }

    #[test]
    fn test_find_or_create_entity_dedup() {
        let (pool, _account_id, msg_id) = setup_test_message("Hello world", "Test");
        let conn = pool.get().unwrap();

        let id1 = find_or_create_entity(&conn, "Acme Corp", "org", &msg_id).unwrap();
        let id2 = find_or_create_entity(&conn, "Acme Corp", "org", &msg_id).unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_find_entity_by_alias() {
        let (pool, _account_id, msg_id) = setup_test_message("Hello world", "Test");
        let conn = pool.get().unwrap();

        let entity_id = find_or_create_entity(&conn, "John Smith", "person", &msg_id).unwrap();

        // Add an alias
        let alias_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO entity_aliases (id, entity_id, alias_name, source_message_id) VALUES (?1, ?2, 'john@acme.com', ?3)",
            rusqlite::params![alias_id, entity_id, msg_id],
        )
        .unwrap();

        // Look up by alias
        let found = find_or_create_entity(&conn, "john@acme.com", "person", &msg_id).unwrap();
        assert_eq!(found, entity_id);
    }

    #[test]
    fn test_entity_relation_insert() {
        let (pool, _account_id, msg_id) = setup_test_message("Hello world", "Test");
        let conn = pool.get().unwrap();

        let id_a = find_or_create_entity(&conn, "Alice", "person", &msg_id).unwrap();
        let id_b = find_or_create_entity(&conn, "Acme Corp", "org", &msg_id).unwrap();

        let rel_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO entity_relations (id, entity_a, entity_b, relation_type, source_message_id) VALUES (?1, ?2, ?3, 'works_at', ?4)",
            rusqlite::params![rel_id, id_a, id_b, msg_id],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entity_relations WHERE entity_a = ?1",
                rusqlite::params![id_a],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_enqueue_entity_extract() {
        let (pool, _account_id, msg_id) = setup_test_message("Hello world", "Test");
        let conn = pool.get().unwrap();

        enqueue_entity_extract(&conn, &msg_id);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM processing_jobs WHERE job_type = 'entity_extract'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_enqueue_entity_extract_dedup() {
        let (pool, _account_id, msg_id) = setup_test_message("Hello world", "Test");
        let conn = pool.get().unwrap();

        enqueue_entity_extract(&conn, &msg_id);
        enqueue_entity_extract(&conn, &msg_id); // duplicate

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM processing_jobs WHERE job_type = 'entity_extract'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_entity_type_validation() {
        assert_eq!(normalize_entity_type("person"), Some("person"));
        assert_eq!(normalize_entity_type("Person"), Some("person"));
        assert_eq!(normalize_entity_type("ORG"), Some("org"));
        assert_eq!(normalize_entity_type("project"), Some("project"));
        assert_eq!(normalize_entity_type("invalid"), None);
        assert_eq!(normalize_entity_type(""), None);
    }

    #[test]
    fn test_precision_validation() {
        assert_eq!(normalize_precision("day"), "day");
        assert_eq!(normalize_precision("Week"), "week");
        assert_eq!(normalize_precision("MONTH"), "month");
        assert_eq!(normalize_precision("invalid"), "day");
    }

    #[test]
    fn test_ai_extraction_json_parsing() {
        let json = r#"{"entities": [{"name": "John", "type": "person", "aliases": ["john@acme.com"]}], "relations": [{"from": "John", "to": "Acme", "relation": "works_at"}], "events": [{"name": "Product Launch", "date": "2026-04-01", "precision": "day"}]}"#;
        let result: AiExtractionResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0].name, "John");
        assert_eq!(result.relations.len(), 1);
        assert_eq!(result.events.len(), 1);
    }

    #[test]
    fn test_ai_extraction_empty_arrays() {
        let json = r#"{"entities": [], "relations": [], "events": []}"#;
        let result: AiExtractionResult = serde_json::from_str(json).unwrap();
        assert!(result.entities.is_empty());
        assert!(result.relations.is_empty());
        assert!(result.events.is_empty());
    }

    #[test]
    fn test_extraction_prompt_contains_content() {
        let prompt = build_extraction_prompt("Meeting notes", "alice@acme.com", "Discussed the Q2 roadmap with Bob");
        assert!(prompt.contains("Meeting notes"));
        assert!(prompt.contains("alice@acme.com"));
        assert!(prompt.contains("Q2 roadmap"));
    }

    #[test]
    fn test_timeline_event_insert() {
        let (pool, account_id, msg_id) = setup_test_message("Launch on April 1st", "Product Launch");
        let conn = pool.get().unwrap();

        let event_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO timeline_events (id, event_name, approximate_date, date_precision, source_message_id, account_id) VALUES (?1, 'Product Launch', '2026-04-01', 'day', ?2, ?3)",
            rusqlite::params![event_id, msg_id, account_id],
        )
        .unwrap();

        let name: String = conn
            .query_row(
                "SELECT event_name FROM timeline_events WHERE id = ?1",
                rusqlite::params![event_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "Product Launch");
    }

    #[test]
    fn test_entity_cascade_delete() {
        let (pool, _account_id, msg_id) = setup_test_message("Hello", "Test");
        let conn = pool.get().unwrap();

        let entity_id = find_or_create_entity(&conn, "Test Entity", "org", &msg_id).unwrap();

        // Should have an alias created automatically
        let alias_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entity_aliases WHERE entity_id = ?1",
                rusqlite::params![entity_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(alias_count > 0);

        // Delete entity -- aliases should cascade
        conn.execute(
            "DELETE FROM entities WHERE id = ?1",
            rusqlite::params![entity_id],
        )
        .unwrap();

        let after_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entity_aliases WHERE entity_id = ?1",
                rusqlite::params![entity_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after_count, 0);
    }
}
