use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct AiConfigResponse {
    pub ollama_url: String,
    pub model: String,
    pub enabled: bool,
    pub connected: bool,
}

#[derive(Debug, Deserialize)]
pub struct SetAiConfigRequest {
    pub ollama_url: Option<String>,
    pub model: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct AiTestResponse {
    pub connected: bool,
    pub models: Vec<String>,
}

fn get_config_value(conn: &rusqlite::Connection, key: &str, default: &str) -> String {
    conn.query_row(
        "SELECT value FROM config WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get(0),
    )
    .unwrap_or_else(|_| default.to_string())
}

fn set_config_value(conn: &rusqlite::Connection, key: &str, value: &str) -> Result<(), StatusCode> {
    conn.execute(
        "INSERT INTO config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = unixepoch()",
        rusqlite::params![key, value],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

pub async fn get_ai_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AiConfigResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let ollama_url = get_config_value(&conn, "ai_ollama_url", &state.config.ollama_url);
    let model = get_config_value(&conn, "ai_model", "");
    let enabled = get_config_value(&conn, "ai_enabled", "false") == "true";

    let connected = state.ollama.health().await;

    Ok(Json(AiConfigResponse {
        ollama_url,
        model,
        enabled,
        connected,
    }))
}

pub async fn set_ai_config(
    State(state): State<Arc<AppState>>,
    Json(input): Json<SetAiConfigRequest>,
) -> Result<Json<AiConfigResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(ref url) = input.ollama_url {
        set_config_value(&conn, "ai_ollama_url", url)?;
    }
    if let Some(ref model) = input.model {
        set_config_value(&conn, "ai_model", model)?;
    }
    if let Some(enabled) = input.enabled {
        set_config_value(&conn, "ai_enabled", if enabled { "true" } else { "false" })?;
    }

    let ollama_url = get_config_value(&conn, "ai_ollama_url", &state.config.ollama_url);
    let model = get_config_value(&conn, "ai_model", "");
    let enabled = get_config_value(&conn, "ai_enabled", "false") == "true";
    let connected = state.ollama.health().await;

    Ok(Json(AiConfigResponse {
        ollama_url,
        model,
        enabled,
        connected,
    }))
}

pub async fn test_ai_connection(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AiTestResponse>, StatusCode> {
    let connected = state.ollama.health().await;
    let models = if connected {
        state.ollama.list_models().await
    } else {
        Vec::new()
    };

    Ok(Json(AiTestResponse { connected, models }))
}
