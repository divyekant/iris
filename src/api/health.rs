use axum::extract::State;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

use crate::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub db: bool,
    pub ai: bool,
    pub memories: bool,
}

pub async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let db_ok = state.db.get().map(|conn| {
        conn.query_row("SELECT 1", [], |_| Ok(())).is_ok()
    }).unwrap_or(false);

    let ai_ok = state.providers.any_healthy().await;
    let memories_ok = state.memories.health().await;

    let status = if db_ok { "ok" } else { "degraded" }.to_string();

    Json(HealthResponse {
        status,
        version: "0.3.0".to_string(),
        db: db_ok,
        ai: ai_ok,
        memories: memories_ok,
    })
}
