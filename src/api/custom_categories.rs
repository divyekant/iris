use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomCategory {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub description: Option<String>,
    pub is_ai_generated: bool,
    pub email_count: i64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateCategory {
    pub account_id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCategory {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeResult {
    pub suggested: Vec<CustomCategory>,
    pub analyzed_messages: i64,
}

#[derive(Debug, Serialize)]
pub struct ExplainResult {
    pub message_id: String,
    pub category: String,
    pub reasoning: String,
}

#[derive(Debug, Deserialize)]
pub struct ListCategoriesQuery {
    pub account_id: Option<String>,
}

// ---------------------------------------------------------------------------
// GET /api/categories/custom
// ---------------------------------------------------------------------------

pub async fn list_custom_categories(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListCategoriesQuery>,
) -> Result<Json<Vec<CustomCategory>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(ref account_id) = params.account_id {
        (
            "SELECT id, account_id, name, description, is_ai_generated, email_count, status, created_at, updated_at FROM custom_categories WHERE account_id = ?1 AND status != 'dismissed' ORDER BY status ASC, email_count DESC".to_string(),
            vec![Box::new(account_id.clone()) as Box<dyn rusqlite::types::ToSql>],
        )
    } else {
        (
            "SELECT id, account_id, name, description, is_ai_generated, email_count, status, created_at, updated_at FROM custom_categories WHERE status != 'dismissed' ORDER BY status ASC, email_count DESC".to_string(),
            vec![],
        )
    };

    let mut stmt = conn.prepare(&sql).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let categories = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(CustomCategory {
                id: row.get(0)?,
                account_id: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                is_ai_generated: row.get::<_, i64>(4)? != 0,
                email_count: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();

    Ok(Json(categories))
}

// ---------------------------------------------------------------------------
// POST /api/categories/custom
// ---------------------------------------------------------------------------

pub async fn create_custom_category(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateCategory>,
) -> Result<Json<CustomCategory>, StatusCode> {
    if input.name.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "INSERT INTO custom_categories (id, account_id, name, description, is_ai_generated, status)
         VALUES (?1, ?2, ?3, ?4, 0, 'active')",
        rusqlite::params![id, input.account_id, input.name.trim(), input.description],
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(CustomCategory {
        id,
        account_id: input.account_id,
        name: input.name.trim().to_string(),
        description: input.description,
        is_ai_generated: false,
        email_count: 0,
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
    }))
}

// ---------------------------------------------------------------------------
// PUT /api/categories/custom/{id}
// ---------------------------------------------------------------------------

pub async fn update_custom_category(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<UpdateCategory>,
) -> Result<Json<CustomCategory>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(ref name) = input.name {
        if name.trim().is_empty() { return Err(StatusCode::BAD_REQUEST); }
        conn.execute(
            "UPDATE custom_categories SET name = ?1, updated_at = unixepoch() WHERE id = ?2",
            rusqlite::params![name.trim(), id],
        ).ok();
    }
    if let Some(ref desc) = input.description {
        conn.execute(
            "UPDATE custom_categories SET description = ?1, updated_at = unixepoch() WHERE id = ?2",
            rusqlite::params![desc, id],
        ).ok();
    }

    let category = conn
        .query_row(
            "SELECT id, account_id, name, description, is_ai_generated, email_count, status, created_at, updated_at FROM custom_categories WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(CustomCategory {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    is_ai_generated: row.get::<_, i64>(4)? != 0,
                    email_count: row.get(5)?,
                    status: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(category))
}

// ---------------------------------------------------------------------------
// DELETE /api/categories/custom/{id}
// ---------------------------------------------------------------------------

pub async fn delete_custom_category(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let deleted = conn
        .execute("DELETE FROM custom_categories WHERE id = ?1", rusqlite::params![id])
        .unwrap_or(0);

    if deleted == 0 { Err(StatusCode::NOT_FOUND) } else { Ok(StatusCode::NO_CONTENT) }
}

// ---------------------------------------------------------------------------
// POST /api/categories/analyze/{account_id}
// ---------------------------------------------------------------------------

pub async fn analyze_categories(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<AnalyzeResult>, StatusCode> {
    // Phase 1: Gather data from DB (drop conn before .await)
    let (samples, existing, analyzed) = {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // Check AI enabled
        let ai_enabled = conn
            .query_row("SELECT value FROM config WHERE key = 'ai_enabled'", [], |row| row.get::<_, String>(0))
            .unwrap_or_else(|_| "false".to_string());

        if ai_enabled != "true" || !state.providers.has_providers() {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }

        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(subject, ''), COALESCE(from_address, ''), COALESCE(ai_category, '')
                 FROM messages
                 WHERE account_id = ?1 AND folder = 'INBOX'
                 ORDER BY date DESC
                 LIMIT 200",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let samps: Vec<(String, String, String)> = stmt
            .query_map(rusqlite::params![account_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let count = samps.len() as i64;

        if samps.is_empty() {
            return Ok(Json(AnalyzeResult { suggested: vec![], analyzed_messages: 0 }));
        }

        let mut stmt2 = conn
            .prepare("SELECT name FROM custom_categories WHERE account_id = ?1 AND status != 'dismissed'")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let ex: Vec<String> = stmt2
            .query_map(rusqlite::params![account_id], |row| row.get::<_, String>(0))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        (samps, ex, count)
    }; // conn dropped here

    // Phase 2: Build prompt and call AI
    let mut sample_text = String::new();
    for (i, (subject, from, cat)) in samples.iter().enumerate().take(50) {
        let sender_domain = from.split('@').nth(1).unwrap_or("unknown");
        sample_text.push_str(&format!("{}. [{}] {} (from {})\n", i + 1, cat, subject, sender_domain));
    }

    let existing_list = if existing.is_empty() { "None".to_string() } else { existing.join(", ") };

    let prompt = format!(
        r#"Analyze these email subjects and suggest 1-3 new custom categories that would help organize them better. The standard categories are: primary, updates, social, promotions. Existing custom categories: {existing_list}.

Suggest ONLY categories that represent clear, recurring patterns NOT covered by existing categories. Return a JSON array:
[{{"name": "Category Name", "description": "What emails belong here"}}]

If no new categories make sense, return an empty array [].

Emails:
{sample_text}"#
    );

    let response = state
        .providers
        .generate(&prompt, Some("You suggest email categories based on usage patterns. Return ONLY valid JSON."))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

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

    #[derive(Deserialize)]
    struct SuggestedCat {
        name: String,
        description: Option<String>,
    }

    let suggestions: Vec<SuggestedCat> = serde_json::from_str(json_str).unwrap_or_default();

    // Phase 3: Store suggestions in DB
    let now = chrono::Utc::now().timestamp();
    let mut created = Vec::new();
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    for sug in suggestions.into_iter().take(3) {
        if sug.name.trim().is_empty() { continue; }
        let dupe: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM custom_categories WHERE account_id = ?1 AND name = ?2 COLLATE NOCASE AND status != 'dismissed')",
                rusqlite::params![account_id, sug.name.trim()],
                |row| row.get(0),
            )
            .unwrap_or(true);
        if dupe { continue; }

        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO custom_categories (id, account_id, name, description, is_ai_generated, status)
             VALUES (?1, ?2, ?3, ?4, 1, 'suggested')",
            rusqlite::params![id, account_id, sug.name.trim(), sug.description],
        ).ok();

        created.push(CustomCategory {
            id,
            account_id: account_id.clone(),
            name: sug.name.trim().to_string(),
            description: sug.description,
            is_ai_generated: true,
            email_count: 0,
            status: "suggested".to_string(),
            created_at: now,
            updated_at: now,
        });
    }

    Ok(Json(AnalyzeResult {
        suggested: created,
        analyzed_messages: analyzed,
    }))
}

// ---------------------------------------------------------------------------
// POST /api/categories/custom/{id}/accept
// ---------------------------------------------------------------------------

pub async fn accept_category(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<CustomCategory>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let updated = conn
        .execute(
            "UPDATE custom_categories SET status = 'active', updated_at = unixepoch() WHERE id = ?1 AND status = 'suggested'",
            rusqlite::params![id],
        )
        .unwrap_or(0);

    if updated == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    let cat = conn
        .query_row(
            "SELECT id, account_id, name, description, is_ai_generated, email_count, status, created_at, updated_at FROM custom_categories WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(CustomCategory {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    is_ai_generated: row.get::<_, i64>(4)? != 0,
                    email_count: row.get(5)?,
                    status: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(cat))
}

// ---------------------------------------------------------------------------
// POST /api/categories/custom/{id}/dismiss
// ---------------------------------------------------------------------------

pub async fn dismiss_category(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let updated = conn
        .execute(
            "UPDATE custom_categories SET status = 'dismissed', updated_at = unixepoch() WHERE id = ?1",
            rusqlite::params![id],
        )
        .unwrap_or(0);

    if updated == 0 { Err(StatusCode::NOT_FOUND) }
    else { Ok(Json(serde_json::json!({ "dismissed": true }))) }
}

// ---------------------------------------------------------------------------
// GET /api/categories/explain/{message_id}
// ---------------------------------------------------------------------------

pub async fn explain_category(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> Result<Json<ExplainResult>, StatusCode> {
    if !state.providers.has_providers() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    // Phase 1: fetch from DB (drop conn before .await)
    let (subject, from_address, body_truncated, category) = {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let (subj, from, body, cat): (String, String, String, String) = conn
            .query_row(
                "SELECT COALESCE(subject, ''), COALESCE(from_address, ''), COALESCE(body_text, ''), COALESCE(ai_category, 'primary') FROM messages WHERE id = ?1",
                rusqlite::params![message_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|_| StatusCode::NOT_FOUND)?;
        let truncated: String = body.chars().take(1000).collect();
        (subj, from, truncated, cat)
    };

    // Phase 2: call AI
    let prompt = format!(
        r#"This email was categorized as "{category}". Explain in 1-2 sentences why this categorization makes sense.

From: {from_address}
Subject: {subject}

{body_truncated}"#
    );

    let reasoning = state
        .providers
        .generate(&prompt, Some("You explain email categorization decisions concisely."))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    Ok(Json(ExplainResult {
        message_id,
        category,
        reasoning: reasoning.trim().to_string(),
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS accounts (id TEXT PRIMARY KEY, email TEXT, is_active INTEGER DEFAULT 1);
             CREATE TABLE IF NOT EXISTS messages (
                 id TEXT PRIMARY KEY, account_id TEXT, from_address TEXT, subject TEXT,
                 body_text TEXT, ai_category TEXT, intent TEXT, folder TEXT DEFAULT 'INBOX',
                 date INTEGER, ai_status TEXT, memories_status TEXT
             );
             CREATE TABLE IF NOT EXISTS config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS custom_categories (
                 id TEXT PRIMARY KEY, account_id TEXT NOT NULL, name TEXT NOT NULL,
                 description TEXT, is_ai_generated INTEGER DEFAULT 0,
                 email_count INTEGER DEFAULT 0,
                 status TEXT DEFAULT 'active' CHECK(status IN ('active', 'suggested', 'dismissed')),
                 created_at INTEGER DEFAULT (unixepoch()), updated_at INTEGER DEFAULT (unixepoch()),
                 FOREIGN KEY(account_id) REFERENCES accounts(id)
             );
             INSERT INTO accounts (id, email) VALUES ('acc1', 'test@example.com');",
        ).unwrap();
        conn
    }

    #[test]
    fn test_create_and_list_categories() {
        let conn = setup_db();
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO custom_categories (id, account_id, name, description, status)
             VALUES (?1, 'acc1', 'Receipts', 'Purchase receipts', 'active')",
            rusqlite::params![id],
        ).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM custom_categories WHERE account_id = 'acc1' AND status = 'active'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_accept_suggested_category() {
        let conn = setup_db();
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO custom_categories (id, account_id, name, description, is_ai_generated, status)
             VALUES (?1, 'acc1', 'Travel', 'Travel bookings', 1, 'suggested')",
            rusqlite::params![id],
        ).unwrap();

        conn.execute(
            "UPDATE custom_categories SET status = 'active' WHERE id = ?1 AND status = 'suggested'",
            rusqlite::params![id],
        ).unwrap();

        let status: String = conn
            .query_row("SELECT status FROM custom_categories WHERE id = ?1", rusqlite::params![id], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "active");
    }

    #[test]
    fn test_dismiss_category() {
        let conn = setup_db();
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO custom_categories (id, account_id, name, status) VALUES (?1, 'acc1', 'Test', 'suggested')",
            rusqlite::params![id],
        ).unwrap();

        conn.execute(
            "UPDATE custom_categories SET status = 'dismissed' WHERE id = ?1",
            rusqlite::params![id],
        ).unwrap();

        let status: String = conn
            .query_row("SELECT status FROM custom_categories WHERE id = ?1", rusqlite::params![id], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "dismissed");
    }

    #[test]
    fn test_dismissed_not_in_active_list() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO custom_categories (id, account_id, name, status) VALUES ('c1', 'acc1', 'Active', 'active')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO custom_categories (id, account_id, name, status) VALUES ('c2', 'acc1', 'Gone', 'dismissed')",
            [],
        ).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM custom_categories WHERE account_id = 'acc1' AND status != 'dismissed'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_delete_category() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO custom_categories (id, account_id, name, status) VALUES ('c1', 'acc1', 'Test', 'active')",
            [],
        ).unwrap();

        let deleted = conn.execute("DELETE FROM custom_categories WHERE id = 'c1'", []).unwrap();
        assert_eq!(deleted, 1);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM custom_categories WHERE id = 'c1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
