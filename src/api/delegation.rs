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
pub struct TriggerConditions {
    pub sender_domain: Option<String>,
    pub subject_contains: Option<String>,
    pub category: Option<String>,
    pub intent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playbook {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub trigger_conditions: TriggerConditions,
    pub action_type: String,
    pub action_template: Option<String>,
    pub confidence_threshold: f64,
    pub enabled: bool,
    pub match_count: i64,
    pub last_matched_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreatePlaybook {
    pub account_id: String,
    pub name: String,
    pub trigger_conditions: TriggerConditions,
    pub action_type: String,
    pub action_template: Option<String>,
    pub confidence_threshold: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePlaybook {
    pub name: Option<String>,
    pub trigger_conditions: Option<TriggerConditions>,
    pub action_type: Option<String>,
    pub action_template: Option<String>,
    pub confidence_threshold: Option<f64>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationAction {
    pub id: String,
    pub playbook_id: String,
    pub message_id: String,
    pub action_taken: String,
    pub confidence: f64,
    pub status: String,
    pub created_at: i64,
    pub playbook_name: Option<String>,
    pub subject: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProcessResult {
    pub matched: bool,
    pub actions: Vec<DelegationAction>,
}

#[derive(Debug, Serialize)]
pub struct DelegationSummary {
    pub actions_today: i64,
    pub pending_review: i64,
    pub active_playbooks: i64,
}

#[derive(Debug, Deserialize)]
pub struct ListActionsQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ListPlaybooksQuery {
    pub account_id: Option<String>,
}

// ---------------------------------------------------------------------------
// GET /api/delegation/playbooks
// ---------------------------------------------------------------------------

pub async fn list_playbooks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListPlaybooksQuery>,
) -> Result<Json<Vec<Playbook>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(ref account_id) = params.account_id {
        (
            "SELECT id, account_id, name, trigger_conditions, action_type, action_template, confidence_threshold, enabled, match_count, last_matched_at, created_at, updated_at FROM delegation_playbooks WHERE account_id = ?1 ORDER BY created_at DESC".to_string(),
            vec![Box::new(account_id.clone()) as Box<dyn rusqlite::types::ToSql>],
        )
    } else {
        (
            "SELECT id, account_id, name, trigger_conditions, action_type, action_template, confidence_threshold, enabled, match_count, last_matched_at, created_at, updated_at FROM delegation_playbooks ORDER BY created_at DESC".to_string(),
            vec![],
        )
    };

    let mut stmt = conn.prepare(&sql).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let playbooks = stmt
        .query_map(params_ref.as_slice(), |row| {
            let trigger_json: String = row.get(3)?;
            let conditions: TriggerConditions = serde_json::from_str(&trigger_json)
                .unwrap_or(TriggerConditions { sender_domain: None, subject_contains: None, category: None, intent: None });
            Ok(Playbook {
                id: row.get(0)?,
                account_id: row.get(1)?,
                name: row.get(2)?,
                trigger_conditions: conditions,
                action_type: row.get(4)?,
                action_template: row.get(5)?,
                confidence_threshold: row.get(6)?,
                enabled: row.get::<_, i64>(7)? != 0,
                match_count: row.get(8)?,
                last_matched_at: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();

    Ok(Json(playbooks))
}

// ---------------------------------------------------------------------------
// POST /api/delegation/playbooks
// ---------------------------------------------------------------------------

pub async fn create_playbook(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreatePlaybook>,
) -> Result<Json<Playbook>, StatusCode> {
    let valid_actions = ["auto_reply", "draft_reply", "forward", "archive", "label"];
    if !valid_actions.contains(&input.action_type.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if input.name.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let id = uuid::Uuid::new_v4().to_string();
    let trigger_json = serde_json::to_string(&input.trigger_conditions)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let threshold = input.confidence_threshold.unwrap_or(0.85);

    conn.execute(
        "INSERT INTO delegation_playbooks (id, account_id, name, trigger_conditions, action_type, action_template, confidence_threshold)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![id, input.account_id, input.name.trim(), trigger_json, input.action_type, input.action_template, threshold],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let now = chrono::Utc::now().timestamp();
    Ok(Json(Playbook {
        id,
        account_id: input.account_id,
        name: input.name.trim().to_string(),
        trigger_conditions: input.trigger_conditions,
        action_type: input.action_type,
        action_template: input.action_template,
        confidence_threshold: threshold,
        enabled: true,
        match_count: 0,
        last_matched_at: None,
        created_at: now,
        updated_at: now,
    }))
}

// ---------------------------------------------------------------------------
// PUT /api/delegation/playbooks/{id}
// ---------------------------------------------------------------------------

pub async fn update_playbook(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<UpdatePlaybook>,
) -> Result<Json<Playbook>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Check exists
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM delegation_playbooks WHERE id = ?1)",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }

    if let Some(ref name) = input.name {
        conn.execute(
            "UPDATE delegation_playbooks SET name = ?1, updated_at = unixepoch() WHERE id = ?2",
            rusqlite::params![name.trim(), id],
        ).ok();
    }
    if let Some(ref conditions) = input.trigger_conditions {
        let json = serde_json::to_string(conditions).map_err(|_| StatusCode::BAD_REQUEST)?;
        conn.execute(
            "UPDATE delegation_playbooks SET trigger_conditions = ?1, updated_at = unixepoch() WHERE id = ?2",
            rusqlite::params![json, id],
        ).ok();
    }
    if let Some(ref action_type) = input.action_type {
        let valid_actions = ["auto_reply", "draft_reply", "forward", "archive", "label"];
        if !valid_actions.contains(&action_type.as_str()) {
            return Err(StatusCode::BAD_REQUEST);
        }
        conn.execute(
            "UPDATE delegation_playbooks SET action_type = ?1, updated_at = unixepoch() WHERE id = ?2",
            rusqlite::params![action_type, id],
        ).ok();
    }
    if let Some(ref template) = input.action_template {
        conn.execute(
            "UPDATE delegation_playbooks SET action_template = ?1, updated_at = unixepoch() WHERE id = ?2",
            rusqlite::params![template, id],
        ).ok();
    }
    if let Some(threshold) = input.confidence_threshold {
        conn.execute(
            "UPDATE delegation_playbooks SET confidence_threshold = ?1, updated_at = unixepoch() WHERE id = ?2",
            rusqlite::params![threshold, id],
        ).ok();
    }
    if let Some(enabled) = input.enabled {
        conn.execute(
            "UPDATE delegation_playbooks SET enabled = ?1, updated_at = unixepoch() WHERE id = ?2",
            rusqlite::params![enabled as i64, id],
        ).ok();
    }

    // Fetch updated
    let playbook = conn
        .query_row(
            "SELECT id, account_id, name, trigger_conditions, action_type, action_template, confidence_threshold, enabled, match_count, last_matched_at, created_at, updated_at FROM delegation_playbooks WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                let trigger_json: String = row.get(3)?;
                let conditions: TriggerConditions = serde_json::from_str(&trigger_json)
                    .unwrap_or(TriggerConditions { sender_domain: None, subject_contains: None, category: None, intent: None });
                Ok(Playbook {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    name: row.get(2)?,
                    trigger_conditions: conditions,
                    action_type: row.get(4)?,
                    action_template: row.get(5)?,
                    confidence_threshold: row.get(6)?,
                    enabled: row.get::<_, i64>(7)? != 0,
                    match_count: row.get(8)?,
                    last_matched_at: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(playbook))
}

// ---------------------------------------------------------------------------
// DELETE /api/delegation/playbooks/{id}
// ---------------------------------------------------------------------------

pub async fn delete_playbook(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Delete associated actions first
    conn.execute("DELETE FROM delegation_actions WHERE playbook_id = ?1", rusqlite::params![id]).ok();
    let deleted = conn
        .execute("DELETE FROM delegation_playbooks WHERE id = ?1", rusqlite::params![id])
        .unwrap_or(0);

    if deleted == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

// ---------------------------------------------------------------------------
// POST /api/delegation/process/{message_id}
// ---------------------------------------------------------------------------

pub async fn process_message(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> Result<Json<ProcessResult>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Fetch message details
    let (account_id, from_address, subject, category, intent): (String, String, String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT account_id, COALESCE(from_address, ''), COALESCE(subject, ''), ai_category, intent FROM messages WHERE id = ?1",
            rusqlite::params![message_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let sender_domain = from_address
        .split('@')
        .nth(1)
        .unwrap_or("")
        .to_lowercase();
    let subject_lower = subject.to_lowercase();

    // Get all enabled playbooks for this account
    let mut stmt = conn
        .prepare(
            "SELECT id, name, trigger_conditions, action_type, action_template, confidence_threshold
             FROM delegation_playbooks
             WHERE account_id = ?1 AND enabled = 1
             ORDER BY confidence_threshold DESC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let playbooks: Vec<(String, String, String, String, Option<String>, f64)> = stmt
        .query_map(rusqlite::params![account_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let mut actions = Vec::new();

    for (pb_id, pb_name, trigger_json, action_type, action_template, threshold) in playbooks {
        let conditions: TriggerConditions = match serde_json::from_str(&trigger_json) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Calculate confidence by checking how many conditions match
        let mut matched = 0u32;
        let mut total = 0u32;

        if let Some(ref domain) = conditions.sender_domain {
            total += 1;
            if sender_domain == domain.to_lowercase() {
                matched += 1;
            }
        }
        if let Some(ref substr) = conditions.subject_contains {
            total += 1;
            if subject_lower.contains(&substr.to_lowercase()) {
                matched += 1;
            }
        }
        if let Some(ref cat) = conditions.category {
            total += 1;
            if category.as_deref().unwrap_or("").eq_ignore_ascii_case(cat) {
                matched += 1;
            }
        }
        if let Some(ref int) = conditions.intent {
            total += 1;
            if intent.as_deref().unwrap_or("").eq_ignore_ascii_case(int) {
                matched += 1;
            }
        }

        if total == 0 {
            continue;
        }

        let confidence = matched as f64 / total as f64;

        if confidence >= threshold {
            // Execute action
            let action_id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO delegation_actions (id, playbook_id, message_id, action_taken, confidence, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'completed')",
                rusqlite::params![action_id, pb_id, message_id, action_type, confidence],
            ).ok();

            // Update playbook stats
            conn.execute(
                "UPDATE delegation_playbooks SET match_count = match_count + 1, last_matched_at = unixepoch(), updated_at = unixepoch() WHERE id = ?1",
                rusqlite::params![pb_id],
            ).ok();

            // Execute the actual action
            execute_delegation_action(&conn, &message_id, &action_type, action_template.as_deref());

            actions.push(DelegationAction {
                id: action_id,
                playbook_id: pb_id,
                message_id: message_id.clone(),
                action_taken: action_type,
                confidence,
                status: "completed".to_string(),
                created_at: chrono::Utc::now().timestamp(),
                playbook_name: Some(pb_name),
                subject: Some(subject.clone()),
            });
        } else if confidence > 0.5 {
            // Pending review
            let action_id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO delegation_actions (id, playbook_id, message_id, action_taken, confidence, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'pending_review')",
                rusqlite::params![action_id, pb_id, message_id, action_type, confidence],
            ).ok();

            actions.push(DelegationAction {
                id: action_id,
                playbook_id: pb_id,
                message_id: message_id.clone(),
                action_taken: action_type,
                confidence,
                status: "pending_review".to_string(),
                created_at: chrono::Utc::now().timestamp(),
                playbook_name: Some(pb_name),
                subject: Some(subject.clone()),
            });
        }
    }

    Ok(Json(ProcessResult {
        matched: !actions.is_empty(),
        actions,
    }))
}

/// Execute the delegation action (archive, label, etc.)
fn execute_delegation_action(conn: &rusqlite::Connection, message_id: &str, action_type: &str, _template: Option<&str>) {
    match action_type {
        "archive" => {
            conn.execute(
                "UPDATE messages SET folder = 'Archive' WHERE id = ?1",
                rusqlite::params![message_id],
            ).ok();
        }
        "label" => {
            // Apply a label if template contains label name
            if let Some(label_name) = _template {
                conn.execute(
                    "UPDATE messages SET labels = CASE WHEN labels IS NULL THEN ?1 ELSE labels || ',' || ?1 END WHERE id = ?2",
                    rusqlite::params![label_name.trim(), message_id],
                ).ok();
            }
        }
        "draft_reply" => {
            // Create an auto_draft with the template
            if let Some(body) = _template {
                let draft_id = uuid::Uuid::new_v4().to_string();
                let account_id: Option<String> = conn
                    .query_row("SELECT account_id FROM messages WHERE id = ?1", rusqlite::params![message_id], |row| row.get(0))
                    .ok();
                if let Some(aid) = account_id {
                    conn.execute(
                        "INSERT INTO auto_drafts (id, message_id, account_id, draft_body, status)
                         VALUES (?1, ?2, ?3, ?4, 'pending')",
                        rusqlite::params![draft_id, message_id, aid, body],
                    ).ok();
                }
            }
        }
        // auto_reply and forward would need SMTP; log for now
        _ => {
            tracing::info!(action_type = action_type, message_id = message_id, "Delegation action logged (SMTP actions require runtime context)");
        }
    }
}

// ---------------------------------------------------------------------------
// GET /api/delegation/actions
// ---------------------------------------------------------------------------

pub async fn list_actions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListActionsQuery>,
) -> Result<Json<Vec<DelegationAction>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let limit = params.limit.unwrap_or(20).min(100);

    let mut stmt = conn
        .prepare(
            "SELECT da.id, da.playbook_id, da.message_id, da.action_taken, da.confidence, da.status, da.created_at,
                    dp.name, m.subject
             FROM delegation_actions da
             LEFT JOIN delegation_playbooks dp ON dp.id = da.playbook_id
             LEFT JOIN messages m ON m.id = da.message_id
             ORDER BY da.created_at DESC
             LIMIT ?1",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let actions = stmt
        .query_map(rusqlite::params![limit], |row| {
            Ok(DelegationAction {
                id: row.get(0)?,
                playbook_id: row.get(1)?,
                message_id: row.get(2)?,
                action_taken: row.get(3)?,
                confidence: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
                playbook_name: row.get(7)?,
                subject: row.get(8)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();

    Ok(Json(actions))
}

// ---------------------------------------------------------------------------
// POST /api/delegation/actions/{id}/undo
// ---------------------------------------------------------------------------

pub async fn undo_action(
    State(state): State<Arc<AppState>>,
    Path(action_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (message_id, action_taken): (String, String) = conn
        .query_row(
            "SELECT message_id, action_taken FROM delegation_actions WHERE id = ?1 AND status != 'undone'",
            rusqlite::params![action_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    // Reverse the action
    match action_taken.as_str() {
        "archive" => {
            conn.execute(
                "UPDATE messages SET folder = 'INBOX' WHERE id = ?1",
                rusqlite::params![message_id],
            ).ok();
        }
        "draft_reply" => {
            // Delete the auto-draft created by delegation
            conn.execute(
                "DELETE FROM auto_drafts WHERE message_id = ?1 AND status = 'pending'",
                rusqlite::params![message_id],
            ).ok();
        }
        _ => {}
    }

    conn.execute(
        "UPDATE delegation_actions SET status = 'undone' WHERE id = ?1",
        rusqlite::params![action_id],
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "undone": true })))
}

// ---------------------------------------------------------------------------
// GET /api/delegation/summary
// ---------------------------------------------------------------------------

pub async fn get_summary(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DelegationSummary>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let today_start = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight (0,0,0) is always valid")
        .and_utc()
        .timestamp();

    let actions_today: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM delegation_actions WHERE created_at >= ?1 AND status = 'completed'",
            rusqlite::params![today_start],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let pending_review: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM delegation_actions WHERE status = 'pending_review'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let active_playbooks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM delegation_playbooks WHERE enabled = 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    Ok(Json(DelegationSummary {
        actions_today,
        pending_review,
        active_playbooks,
    }))
}

// ---------------------------------------------------------------------------
// Job integration: enqueue delegation processing
// ---------------------------------------------------------------------------

/// Enqueue a delegation processing job for a message (called from sync).
pub fn enqueue_delegation_process(conn: &rusqlite::Connection, message_id: &str) {
    // Check if delegation has any enabled playbooks
    let has_playbooks: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM delegation_playbooks WHERE enabled = 1)",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !has_playbooks {
        return;
    }

    // Dedup check
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM processing_jobs WHERE job_type = 'delegation_process' AND message_id = ?1 AND status IN ('pending','processing'))",
            rusqlite::params![message_id],
            |row| row.get(0),
        )
        .unwrap_or(true);

    if exists {
        return;
    }

    if let Err(e) = conn.execute(
        "INSERT INTO processing_jobs (job_type, message_id) VALUES ('delegation_process', ?1)",
        rusqlite::params![message_id],
    ) {
        tracing::warn!("Failed to enqueue delegation_process: {e}");
    }
}

/// Public wrapper for execute_delegation_action (callable from worker).
pub fn execute_delegation_action_pub(conn: &rusqlite::Connection, message_id: &str, action_type: &str, template: Option<&str>) {
    execute_delegation_action(conn, message_id, action_type, template);
}

/// Check if any delegation playbook matched a message (used by worker for auto_draft precedence).
pub fn has_delegation_match(conn: &rusqlite::Connection, message_id: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM delegation_actions WHERE message_id = ?1 AND status IN ('completed', 'pending_review'))",
        rusqlite::params![message_id],
        |row| row.get(0),
    )
    .unwrap_or(false)
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
                 labels TEXT, ai_status TEXT, memories_status TEXT
             );
             CREATE TABLE IF NOT EXISTS auto_drafts (
                 id TEXT PRIMARY KEY, message_id TEXT, account_id TEXT, pattern_id TEXT,
                 draft_body TEXT, status TEXT DEFAULT 'pending', created_at INTEGER DEFAULT (unixepoch())
             );
             CREATE TABLE IF NOT EXISTS delegation_playbooks (
                 id TEXT PRIMARY KEY, account_id TEXT NOT NULL, name TEXT NOT NULL,
                 trigger_conditions TEXT NOT NULL, action_type TEXT NOT NULL,
                 action_template TEXT, confidence_threshold REAL DEFAULT 0.85,
                 enabled INTEGER DEFAULT 1, match_count INTEGER DEFAULT 0,
                 last_matched_at INTEGER, created_at INTEGER DEFAULT (unixepoch()),
                 updated_at INTEGER DEFAULT (unixepoch()),
                 FOREIGN KEY(account_id) REFERENCES accounts(id)
             );
             CREATE TABLE IF NOT EXISTS delegation_actions (
                 id TEXT PRIMARY KEY, playbook_id TEXT NOT NULL, message_id TEXT NOT NULL,
                 action_taken TEXT NOT NULL, confidence REAL NOT NULL,
                 status TEXT DEFAULT 'completed', created_at INTEGER DEFAULT (unixepoch()),
                 FOREIGN KEY(playbook_id) REFERENCES delegation_playbooks(id),
                 FOREIGN KEY(message_id) REFERENCES messages(id)
             );
             CREATE TABLE IF NOT EXISTS processing_jobs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 job_type TEXT NOT NULL,
                 message_id TEXT,
                 status TEXT NOT NULL DEFAULT 'pending',
                 attempts INTEGER NOT NULL DEFAULT 0,
                 max_attempts INTEGER NOT NULL DEFAULT 4,
                 payload TEXT, error TEXT,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 next_retry_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             INSERT INTO accounts (id, email) VALUES ('acc1', 'test@example.com');
             INSERT INTO messages (id, account_id, from_address, subject, ai_category, intent)
                 VALUES ('msg1', 'acc1', 'sender@corp.com', 'Weekly Meeting Update', 'updates', 'informational');",
        ).unwrap();
        conn
    }

    #[test]
    fn test_trigger_conditions_match() {
        let conn = setup_db();
        let conditions = TriggerConditions {
            sender_domain: Some("corp.com".to_string()),
            subject_contains: Some("meeting".to_string()),
            category: None,
            intent: None,
        };
        let trigger_json = serde_json::to_string(&conditions).unwrap();

        conn.execute(
            "INSERT INTO delegation_playbooks (id, account_id, name, trigger_conditions, action_type, confidence_threshold)
             VALUES ('pb1', 'acc1', 'Archive corp meetings', ?1, 'archive', 0.85)",
            rusqlite::params![trigger_json],
        ).unwrap();

        // Simulate matching
        let sender_domain = "corp.com";
        let subject_lower = "weekly meeting update";
        let mut matched = 0u32;
        let mut total = 0u32;

        if let Some(ref domain) = conditions.sender_domain {
            total += 1;
            if sender_domain == domain.to_lowercase() { matched += 1; }
        }
        if let Some(ref substr) = conditions.subject_contains {
            total += 1;
            if subject_lower.contains(&substr.to_lowercase()) { matched += 1; }
        }

        let confidence = matched as f64 / total as f64;
        assert_eq!(confidence, 1.0);
        assert!(confidence >= 0.85);
    }

    #[test]
    fn test_execute_archive_action() {
        let conn = setup_db();
        execute_delegation_action(&conn, "msg1", "archive", None);

        let folder: String = conn
            .query_row("SELECT folder FROM messages WHERE id = 'msg1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(folder, "Archive");
    }

    #[test]
    fn test_execute_label_action() {
        let conn = setup_db();
        execute_delegation_action(&conn, "msg1", "label", Some("Important"));

        let labels: String = conn
            .query_row("SELECT labels FROM messages WHERE id = 'msg1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(labels, "Important");
    }

    #[test]
    fn test_has_delegation_match_false() {
        let conn = setup_db();
        assert!(!has_delegation_match(&conn, "msg1"));
    }

    #[test]
    fn test_has_delegation_match_true() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO delegation_playbooks (id, account_id, name, trigger_conditions, action_type)
             VALUES ('pb1', 'acc1', 'Test', '{}', 'archive')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO delegation_actions (id, playbook_id, message_id, action_taken, confidence, status)
             VALUES ('da1', 'pb1', 'msg1', 'archive', 1.0, 'completed')",
            [],
        ).unwrap();
        assert!(has_delegation_match(&conn, "msg1"));
    }

    #[test]
    fn test_enqueue_delegation_no_playbooks() {
        let conn = setup_db();
        enqueue_delegation_process(&conn, "msg1");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM processing_jobs WHERE job_type = 'delegation_process'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0); // No enabled playbooks, should not enqueue
    }

    #[test]
    fn test_enqueue_delegation_with_playbooks() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO delegation_playbooks (id, account_id, name, trigger_conditions, action_type)
             VALUES ('pb1', 'acc1', 'Test', '{}', 'archive')",
            [],
        ).unwrap();
        enqueue_delegation_process(&conn, "msg1");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM processing_jobs WHERE job_type = 'delegation_process'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_enqueue_delegation_dedup() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO delegation_playbooks (id, account_id, name, trigger_conditions, action_type)
             VALUES ('pb1', 'acc1', 'Test', '{}', 'archive')",
            [],
        ).unwrap();
        enqueue_delegation_process(&conn, "msg1");
        enqueue_delegation_process(&conn, "msg1"); // duplicate
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM processing_jobs WHERE job_type = 'delegation_process'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
