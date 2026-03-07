use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

use crate::jobs::queue;
use crate::AppState;

/// GET /api/ai/queue-status — return job queue statistics
pub async fn queue_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<queue::QueueStatus>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(queue::get_queue_status(&conn)))
}

#[derive(Serialize)]
pub struct ReprocessResult {
    pub enqueued: i64,
}

/// POST /api/ai/reprocess — enqueue AI classification for all untagged non-draft messages
pub async fn reprocess_untagged(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ReprocessResult>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Find all non-draft messages that have no AI priority label
    let mut stmt = conn
        .prepare(
            "SELECT id, subject, from_address, COALESCE(body_text, snippet, '') \
             FROM messages WHERE ai_priority_label IS NULL AND is_draft = 0",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows: Vec<(String, String, String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let count = rows.len() as i64;
    for (id, subject, from, body) in &rows {
        queue::enqueue_ai_classify(&conn, id, subject, from, body);
    }

    tracing::info!("Reprocess: enqueued {} untagged messages for AI classification", count);
    Ok(Json(ReprocessResult { enqueued: count }))
}
