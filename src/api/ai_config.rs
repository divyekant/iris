use axum::extract::State;
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::ai::provider::{LlmProvider, ProviderStatus};
use crate::api::unified_auth::{AuthContext, Permission};
use crate::secrets;
use crate::AppState;

// ---------------------------------------------------------------------------
// Config key constants
// ---------------------------------------------------------------------------

const CONFIG_AI_OLLAMA_URL: &str = "ai_ollama_url";
const CONFIG_AI_MODEL: &str = "ai_model";
const CONFIG_AI_MODEL_ANTHROPIC: &str = "ai_model_anthropic";
const CONFIG_AI_MODEL_OPENAI: &str = "ai_model_openai";
const CONFIG_AI_ENABLED: &str = "ai_enabled";
const CONFIG_DECAY_ENABLED: &str = "decay_enabled";
const CONFIG_DECAY_THRESHOLD_DAYS: &str = "decay_threshold_days";
const CONFIG_DECAY_FACTOR: &str = "decay_factor";

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
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<AiConfigResponse>, StatusCode> {
    auth.require(Permission::Autonomous)?;
    let (ollama_url, model, enabled, decay_enabled, decay_threshold_days, decay_factor) = {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let ollama_url = get_config_value(&conn, CONFIG_AI_OLLAMA_URL, &state.config.ollama_url);
        let model = get_config_value(&conn, CONFIG_AI_MODEL, "");
        let enabled = get_config_value(&conn, CONFIG_AI_ENABLED, "false") == "true";
        let decay_enabled = get_config_value(&conn, CONFIG_DECAY_ENABLED, "true") == "true";
        let decay_threshold_days: i64 = get_config_value(&conn, CONFIG_DECAY_THRESHOLD_DAYS, "7").parse().unwrap_or(7);
        let decay_factor: f64 = get_config_value(&conn, CONFIG_DECAY_FACTOR, "0.85").parse().unwrap_or(0.85);
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
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
    Json(input): Json<SetAiConfigRequest>,
) -> Result<Json<AiConfigResponse>, StatusCode> {
    auth.require(Permission::Autonomous)?;
    {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(ref url) = input.ollama_url {
            set_config_value(&conn, CONFIG_AI_OLLAMA_URL, url)?;
        }
        if let Some(ref model) = input.model {
            set_config_value(&conn, CONFIG_AI_MODEL, model)?;
        }
        if let Some(enabled) = input.enabled {
            set_config_value(&conn, CONFIG_AI_ENABLED, if enabled { "true" } else { "false" })?;
        }
        if let Some(ref key) = input.anthropic_api_key {
            secrets::set_secret_config_value(&conn, "anthropic_api_key", key)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        if let Some(ref model) = input.anthropic_model {
            set_config_value(&conn, CONFIG_AI_MODEL_ANTHROPIC, model)?;
        }
        if let Some(ref key) = input.openai_api_key {
            secrets::set_secret_config_value(&conn, "openai_api_key", key)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        if let Some(ref model) = input.openai_model {
            set_config_value(&conn, CONFIG_AI_MODEL_OPENAI, model)?;
        }
        if let Some(ref url) = input.memories_url {
            set_config_value(&conn, "memories_url", url)?;
        }
        if let Some(ref key) = input.memories_api_key {
            secrets::set_secret_config_value(&conn, "memories_api_key", key)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        if let Some(decay_enabled) = input.decay_enabled {
            set_config_value(&conn, CONFIG_DECAY_ENABLED, if decay_enabled { "true" } else { "false" })?;
        }
        if let Some(days) = input.decay_threshold_days {
            if days < 1 {
                return Err(StatusCode::BAD_REQUEST);
            }
            set_config_value(&conn, CONFIG_DECAY_THRESHOLD_DAYS, &days.to_string())?;
        }
        if let Some(factor) = input.decay_factor {
            if !(0.0..=1.0).contains(&factor) {
                return Err(StatusCode::BAD_REQUEST);
            }
            set_config_value(&conn, CONFIG_DECAY_FACTOR, &factor.to_string())?;
        }
    }

    // Apply memories config change live (no restart needed)
    if input.memories_url.is_some() || input.memories_api_key.is_some() {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let url = get_config_value(&conn, "memories_url", &state.config.memories_url);
        let key = secrets::get_secret_config_value(&conn, "memories_api_key")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .unwrap_or_default();
        let key = if key.is_empty() { state.config.memories_api_key.clone() } else { Some(key) };
        state.memories.update_config(&url, key);
    }

    // Hot-reload providers when API keys or URLs change (no restart needed)
    if input.anthropic_api_key.is_some()
        || input.openai_api_key.is_some()
        || input.ollama_url.is_some()
        || input.model.is_some()
        || input.anthropic_model.is_some()
        || input.openai_model.is_some()
    {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let new_providers = rebuild_providers(&conn, &state.config);
        let count = new_providers.len();
        state.providers.reload(new_providers);
        tracing::info!("Provider pool reloaded: {} providers", count);
    }

    // Re-read to return current state
    let (ollama_url, model, enabled, decay_enabled, decay_threshold_days, decay_factor) = {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let ollama_url = get_config_value(&conn, CONFIG_AI_OLLAMA_URL, &state.config.ollama_url);
        let model = get_config_value(&conn, CONFIG_AI_MODEL, "");
        let enabled = get_config_value(&conn, CONFIG_AI_ENABLED, "false") == "true";
        let decay_enabled = get_config_value(&conn, CONFIG_DECAY_ENABLED, "true") == "true";
        let decay_threshold_days: i64 = get_config_value(&conn, CONFIG_DECAY_THRESHOLD_DAYS, "7").parse().unwrap_or(7);
        let decay_factor: f64 = get_config_value(&conn, CONFIG_DECAY_FACTOR, "0.85").parse().unwrap_or(0.85);
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
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<AiTestResponse>, StatusCode> {
    auth.require(Permission::Autonomous)?;
    let providers = state.providers.health_status().await;
    Ok(Json(AiTestResponse { providers }))
}

/// Rebuild the provider list from DB config + env fallbacks.
/// Mirrors the startup logic in main.rs so the same providers are created.
fn rebuild_providers(conn: &rusqlite::Connection, config: &crate::config::Config) -> Vec<LlmProvider> {
    let mut providers = Vec::new();

    // Ollama
    let ollama_url = get_config_value(conn, CONFIG_AI_OLLAMA_URL, &config.ollama_url);
    let ollama_model = get_config_value(conn, CONFIG_AI_MODEL, "");
    let ollama = crate::ai::ollama::OllamaClient::with_model(&ollama_url, &ollama_model);
    providers.push(LlmProvider::Ollama(ollama));

    // Anthropic
    let anthropic_key = secrets::get_secret_config_value(conn, "anthropic_api_key")
        .ok()
        .flatten()
        .or_else(|| config.anthropic_api_key.clone());
    if let Some(ref key) = anthropic_key {
        if !key.is_empty() {
            let model = get_config_value(conn, CONFIG_AI_MODEL_ANTHROPIC, "");
            let model = if model.is_empty() { None } else { Some(model) };
            let client = crate::ai::anthropic::AnthropicClient::new(key, model.as_deref());
            providers.push(LlmProvider::Anthropic(client));
        }
    }

    // OpenAI
    let openai_key = secrets::get_secret_config_value(conn, "openai_api_key")
        .ok()
        .flatten()
        .or_else(|| config.openai_api_key.clone());
    if let Some(ref key) = openai_key {
        if !key.is_empty() {
            let model = get_config_value(conn, CONFIG_AI_MODEL_OPENAI, "");
            let model = if model.is_empty() { None } else { Some(model) };
            let client = crate::ai::openai::OpenAIClient::new(key, model.as_deref());
            providers.push(LlmProvider::OpenAI(client));
        }
    }

    providers
}
