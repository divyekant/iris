use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub theme: String,
}

#[derive(Debug, Deserialize)]
pub struct SetThemeRequest {
    pub theme: String,
}

pub async fn get_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ConfigResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let theme: String = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'theme'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "system".to_string());

    Ok(Json(ConfigResponse { theme }))
}

pub async fn set_theme(
    State(state): State<Arc<AppState>>,
    Json(input): Json<SetThemeRequest>,
) -> Result<Json<ConfigResponse>, StatusCode> {
    // Validate theme value
    match input.theme.as_str() {
        "light" | "dark" | "system" => {}
        _ => return Err(StatusCode::BAD_REQUEST),
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    conn.execute(
        "INSERT INTO config (key, value) VALUES ('theme', ?1)
         ON CONFLICT(key) DO UPDATE SET value = ?1",
        rusqlite::params![input.theme],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ConfigResponse {
        theme: input.theme,
    }))
}
