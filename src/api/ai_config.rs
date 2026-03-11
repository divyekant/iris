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
    // Priority decay settings
    pub decay_enabled: bool,
    pub decay_threshold_days: i64,
    pub decay_factor: f64,
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
    pub memories_url: Option<String>,
    pub memories_api_key: Option<String>,
    pub decay_enabled: Option<bool>,
    pub decay_threshold_days: Option<i64>,
    pub decay_factor: Option<f64>,
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
    let (ollama_url, model, enabled, decay_enabled, decay_threshold_days, decay_factor) = {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let ollama_url = get_config_value(&conn, "ai_ollama_url", &state.config.ollama_url);
        let model = get_config_value(&conn, "ai_model", "");
        let enabled = get_config_value(&conn, "ai_enabled", "false") == "true";
        let decay_enabled = get_config_value(&conn, "decay_enabled", "true") == "true";
        let decay_threshold_days: i64 = get_config_value(&conn, "decay_threshold_days", "7").parse().unwrap_or(7);
        let decay_factor: f64 = get_config_value(&conn, "decay_factor", "0.85").parse().unwrap_or(0.85);
        (ollama_url, model, enabled, decay_enabled, decay_threshold_days, decay_factor)
    };

    let providers = state.providers.health_status().await;
    let connected = providers.iter().any(|p| p.healthy);
    let memories_connected = state.memories.health().await;

    Ok(Json(AiConfigResponse {
        enabled,
        providers,
        memories_url: state.memories.base_url(),
        memories_connected,
        ollama_url,
        model,
        connected,
        decay_enabled,
        decay_threshold_days,
        decay_factor,
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
        if let Some(ref url) = input.memories_url {
            set_config_value(&conn, "memories_url", url)?;
        }
        if let Some(ref key) = input.memories_api_key {
            set_config_value(&conn, "memories_api_key", key)?;
        }
        if let Some(decay_enabled) = input.decay_enabled {
            set_config_value(&conn, "decay_enabled", if decay_enabled { "true" } else { "false" })?;
        }
        if let Some(days) = input.decay_threshold_days {
            set_config_value(&conn, "decay_threshold_days", &days.to_string())?;
        }
        if let Some(factor) = input.decay_factor {
            set_config_value(&conn, "decay_factor", &factor.to_string())?;
        }
    }

    // Apply memories config change live (no restart needed)
    if input.memories_url.is_some() || input.memories_api_key.is_some() {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let url = get_config_value(&conn, "memories_url", &state.config.memories_url);
        let key = get_config_value(&conn, "memories_api_key", "");
        let key = if key.is_empty() { state.config.memories_api_key.clone() } else { Some(key) };
        state.memories.update_config(&url, key);
    }

    // Re-read to return current state
    let (ollama_url, model, enabled, decay_enabled, decay_threshold_days, decay_factor) = {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let ollama_url = get_config_value(&conn, "ai_ollama_url", &state.config.ollama_url);
        let model = get_config_value(&conn, "ai_model", "");
        let enabled = get_config_value(&conn, "ai_enabled", "false") == "true";
        let decay_enabled = get_config_value(&conn, "decay_enabled", "true") == "true";
        let decay_threshold_days: i64 = get_config_value(&conn, "decay_threshold_days", "7").parse().unwrap_or(7);
        let decay_factor: f64 = get_config_value(&conn, "decay_factor", "0.85").parse().unwrap_or(0.85);
        (ollama_url, model, enabled, decay_enabled, decay_threshold_days, decay_factor)
    };

    let providers = state.providers.health_status().await;
    let connected = providers.iter().any(|p| p.healthy);
    let memories_connected = state.memories.health().await;

    Ok(Json(AiConfigResponse {
        enabled,
        providers,
        memories_url: state.memories.base_url(),
        memories_connected,
        ollama_url,
        model,
        connected,
        decay_enabled,
        decay_threshold_days,
        decay_factor,
    }))
}

pub async fn test_ai_connection(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AiTestResponse>, StatusCode> {
    let providers = state.providers.health_status().await;
    Ok(Json(AiTestResponse { providers }))
}
