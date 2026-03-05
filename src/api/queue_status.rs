use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
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
