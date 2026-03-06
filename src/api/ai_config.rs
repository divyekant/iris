use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::ai::provider::ProviderStatus;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct AiConfigResponse {
    pub enabled: bool,
    pub providers: Vec<ProviderStatus>,
    pub memories_url: String,
    pub memories_connected: bool,
    // Legacy fields for backward compat
    pub ollama_url: String,
    pub model: String,
    pub connected: bool,
}

#[derive(Debug, Deserialize)]
pub struct SetAiConfigRequest {
    pub enabled: Option<bool>,
    pub ollama_url: Option<String>,
    pub model: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub anthropic_model: Option<String>,
    pub openai_api_key: Option<String>,
    pub openai_model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AiTestResponse {
    pub providers: Vec<ProviderStatus>,
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
    let (ollama_url, model, enabled) = {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let ollama_url = get_config_value(&conn, "ai_ollama_url", &state.config.ollama_url);
        let model = get_config_value(&conn, "ai_model", "");
        let enabled = get_config_value(&conn, "ai_enabled", "false") == "true";
        (ollama_url, model, enabled)
    };

    let providers = state.providers.health_status().await;
    let connected = providers.iter().any(|p| p.healthy);
    let memories_connected = state.memories.health().await;

    Ok(Json(AiConfigResponse {
        enabled,
        providers,
        memories_url: state.memories.base_url.clone(),
        memories_connected,
        ollama_url,
        model,
        connected,
    }))
}

pub async fn set_ai_config(
    State(state): State<Arc<AppState>>,
    Json(input): Json<SetAiConfigRequest>,
) -> Result<Json<AiConfigResponse>, StatusCode> {
    {
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
        if let Some(ref key) = input.anthropic_api_key {
            set_config_value(&conn, "anthropic_api_key", key)?;
        }
        if let Some(ref model) = input.anthropic_model {
            set_config_value(&conn, "ai_model_anthropic", model)?;
        }
        if let Some(ref key) = input.openai_api_key {
            set_config_value(&conn, "openai_api_key", key)?;
        }
        if let Some(ref model) = input.openai_model {
            set_config_value(&conn, "ai_model_openai", model)?;
        }
    }

    // Re-read to return current state
    let (ollama_url, model, enabled) = {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let ollama_url = get_config_value(&conn, "ai_ollama_url", &state.config.ollama_url);
        let model = get_config_value(&conn, "ai_model", "");
        let enabled = get_config_value(&conn, "ai_enabled", "false") == "true";
        (ollama_url, model, enabled)
    };

    let providers = state.providers.health_status().await;
    let connected = providers.iter().any(|p| p.healthy);
    let memories_connected = state.memories.health().await;

    Ok(Json(AiConfigResponse {
        enabled,
        providers,
        memories_url: state.memories.base_url.clone(),
        memories_connected,
        ollama_url,
        model,
        connected,
    }))
}

pub async fn test_ai_connection(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AiTestResponse>, StatusCode> {
    let providers = state.providers.health_status().await;
    Ok(Json(AiTestResponse { providers }))
}
