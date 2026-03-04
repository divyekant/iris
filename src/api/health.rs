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
    pub ollama: bool,
    pub memories: bool,
}

pub async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let db_ok = state.db.get().map(|conn| {
        conn.query_row("SELECT 1", [], |_| Ok(())).is_ok()
    }).unwrap_or(false);

    let ollama_ok = state.ollama.health().await;
    let memories_ok = state.memories.health().await;

    let status = if db_ok { "ok" } else { "degraded" }.to_string();

    Json(HealthResponse {
        status,
        version: env!("CARGO_PKG_VERSION").to_string(),
        db: db_ok,
        ollama: ollama_ok,
        memories: memories_ok,
    })
}
