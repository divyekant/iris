use axum::extract::State;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

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
    let pool = state.db.clone();
    let db_ok = tokio::time::timeout(Duration::from_secs(5), tokio::task::spawn_blocking(move || {
        pool.get().map(|conn| {
            conn.query_row("SELECT 1", [], |_| Ok(())).is_ok()
        }).unwrap_or(false)
    }))
    .await
    .map(|r| r.unwrap_or(false))
    .unwrap_or(false);

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
