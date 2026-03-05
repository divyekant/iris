use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

const VALID_FIELDS: &[&str] = &["category", "priority_label", "intent"];
const VALID_CATEGORIES: &[&str] = &[
    "Primary", "Updates", "Social", "Promotions", "Finance", "Travel", "Newsletters",
];
const VALID_PRIORITIES: &[&str] = &["urgent", "high", "normal", "low"];
const VALID_INTENTS: &[&str] = &[
    "ACTION_REQUEST", "INFORMATIONAL", "TRANSACTIONAL", "SOCIAL", "MARKETING", "NOTIFICATION",
];

#[derive(Debug, Deserialize)]
pub struct FeedbackRequest {
    pub field: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct FeedbackResponse {
    pub updated: bool,
}

/// PUT /api/messages/{id}/ai-feedback — correct an AI classification
pub async fn submit_feedback(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<FeedbackRequest>,
) -> Result<Json<FeedbackResponse>, StatusCode> {
    if !VALID_FIELDS.contains(&req.field.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Validate value for the given field
    let valid = match req.field.as_str() {
        "category" => VALID_CATEGORIES.contains(&req.value.as_str()),
        "priority_label" => VALID_PRIORITIES.contains(&req.value.as_str()),
        "intent" => VALID_INTENTS.contains(&req.value.as_str()),
        _ => false,
    };
    if !valid {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get the current value before updating
    let column = match req.field.as_str() {
        "category" => "ai_category",
        "priority_label" => "ai_priority_label",
        "intent" => "ai_intent",
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let original: Option<String> = conn
        .query_row(
            &format!("SELECT {} FROM messages WHERE id = ?1", column),
            rusqlite::params![id],
            |row| row.get(0),
        )
        .ok();

    // Update the message's AI field
    let updated = conn
        .execute(
            &format!(
                "UPDATE messages SET {} = ?1, updated_at = unixepoch() WHERE id = ?2",
                column
            ),
            rusqlite::params![req.value, id],
        )
        .unwrap_or(0)
        > 0;

    if !updated {
        return Err(StatusCode::NOT_FOUND);
    }

    // Record the correction for future prompt tuning
    conn.execute(
        "INSERT INTO ai_feedback (message_id, field, original_value, corrected_value)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, req.field, original, req.value],
    )
    .ok();

    // Trigger preference extraction every 10 corrections
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM ai_feedback", [], |row| row.get(0))
        .unwrap_or(0);
    if total > 0 && total % 10 == 0 {
        crate::jobs::queue::enqueue_pref_extract(&conn);
    }

    Ok(Json(FeedbackResponse { updated }))
}

/// GET /api/ai/feedback-stats — summary of user corrections
#[derive(Debug, Serialize)]
pub struct FeedbackStats {
    pub total_corrections: i64,
    pub by_field: Vec<FieldStat>,
    pub common_corrections: Vec<CorrectionPattern>,
}

#[derive(Debug, Serialize)]
pub struct FieldStat {
    pub field: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct CorrectionPattern {
    pub field: String,
    pub original: Option<String>,
    pub corrected: String,
    pub count: i64,
}

pub async fn feedback_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<FeedbackStats>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM ai_feedback", [], |row| row.get(0))
        .unwrap_or(0);

    let mut by_field = Vec::new();
    {
        let mut stmt = conn
            .prepare("SELECT field, COUNT(*) as cnt FROM ai_feedback GROUP BY field ORDER BY cnt DESC")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(FieldStat {
                    field: row.get(0)?,
                    count: row.get(1)?,
                })
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for r in rows {
            if let Ok(s) = r {
                by_field.push(s);
            }
        }
    }

    let mut common = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT field, original_value, corrected_value, COUNT(*) as cnt
                 FROM ai_feedback
                 GROUP BY field, original_value, corrected_value
                 ORDER BY cnt DESC
                 LIMIT 20",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CorrectionPattern {
                    field: row.get(0)?,
                    original: row.get(1)?,
                    corrected: row.get(2)?,
                    count: row.get(3)?,
                })
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for r in rows {
            if let Ok(c) = r {
                common.push(c);
            }
        }
    }

    Ok(Json(FeedbackStats {
        total_corrections: total,
        by_field,
        common_corrections: common,
    }))
}

/// Build a feedback-aware system prompt suffix from stored corrections.
/// Used by the AI pipeline to adjust classifications based on past user corrections.
pub fn build_feedback_context(conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>) -> Option<String> {
    let mut stmt = conn
        .prepare(
            "SELECT field, original_value, corrected_value, COUNT(*) as cnt
             FROM ai_feedback
             GROUP BY field, original_value, corrected_value
             HAVING cnt >= 2
             ORDER BY cnt DESC
             LIMIT 10",
        )
        .ok()?;

    let patterns: Vec<String> = stmt
        .query_map([], |row| {
            let field: String = row.get(0)?;
            let original: Option<String> = row.get(1)?;
            let corrected: String = row.get(2)?;
            let count: i64 = row.get(3)?;
            let orig = original.unwrap_or_else(|| "unset".to_string());
            Ok(format!("- The user corrected {field} from \"{orig}\" to \"{corrected}\" ({count} times)"))
        })
        .ok()?
        .filter_map(|r| r.ok())
        .collect();

    if patterns.is_empty() {
        return None;
    }

    Some(format!(
        "\n\nUser correction patterns (adjust your classifications accordingly):\n{}",
        patterns.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_fields() {
        assert!(VALID_FIELDS.contains(&"category"));
        assert!(VALID_FIELDS.contains(&"priority_label"));
        assert!(VALID_FIELDS.contains(&"intent"));
        assert!(!VALID_FIELDS.contains(&"summary"));
    }

    #[test]
    fn test_valid_categories() {
        assert!(VALID_CATEGORIES.contains(&"Primary"));
        assert!(VALID_CATEGORIES.contains(&"Promotions"));
        assert!(!VALID_CATEGORIES.contains(&"Spam"));
    }

    #[test]
    fn test_valid_priorities() {
        assert!(VALID_PRIORITIES.contains(&"urgent"));
        assert!(!VALID_PRIORITIES.contains(&"critical"));
    }
}
