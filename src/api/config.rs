use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use regex_lite::Regex;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub theme: String,
    pub view_mode: String,
}

#[derive(Debug, Deserialize)]
pub struct SetThemeRequest {
    pub theme: String,
}

#[derive(Debug, Deserialize)]
pub struct SetViewModeRequest {
    pub view_mode: String,
}

// --- Appearance settings ---

static RE_HEX_COLOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#[0-9a-fA-F]{6}$").unwrap());

const ALLOWED_FONTS: &[&str] = &[
    "System Default", "Inter", "Roboto", "Open Sans", "Lato",
    "Source Sans 3", "IBM Plex Sans", "Noto Sans",
];

const ALLOWED_MONO_FONTS: &[&str] = &[
    "System Mono", "JetBrains Mono", "Fira Code",
    "Source Code Pro", "IBM Plex Mono", "Ubuntu Mono",
];

#[derive(Debug, Serialize)]
pub struct AppearanceResponse {
    pub accent_color: String,
    pub font_family: String,
    pub font_mono: String,
}

#[derive(Debug, Deserialize)]
pub struct SetAppearanceRequest {
    pub accent_color: Option<String>,
    pub font_family: Option<String>,
    pub font_mono: Option<String>,
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

    let view_mode: String = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'view_mode'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "traditional".to_string());

    Ok(Json(ConfigResponse { theme, view_mode }))
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

    let view_mode: String = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'view_mode'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "traditional".to_string());

    Ok(Json(ConfigResponse {
        theme: input.theme,
        view_mode,
    }))
}

pub async fn set_view_mode(
    State(state): State<Arc<AppState>>,
    Json(input): Json<SetViewModeRequest>,
) -> Result<Json<ConfigResponse>, StatusCode> {
    match input.view_mode.as_str() {
        "traditional" | "messaging" => {}
        _ => return Err(StatusCode::BAD_REQUEST),
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    conn.execute(
        "INSERT INTO config (key, value) VALUES ('view_mode', ?1)
         ON CONFLICT(key) DO UPDATE SET value = ?1",
        rusqlite::params![input.view_mode],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let theme: String = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'theme'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "system".to_string());

    Ok(Json(ConfigResponse {
        theme,
        view_mode: input.view_mode,
    }))
}

// --- Appearance endpoints ---

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
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        rusqlite::params![key, value],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

pub async fn get_appearance(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AppearanceResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(AppearanceResponse {
        accent_color: get_config_value(&conn, "accent_color", "#d4af37"),
        font_family: get_config_value(&conn, "font_family", "Inter"),
        font_mono: get_config_value(&conn, "font_mono", "System Mono"),
    }))
}

pub async fn set_appearance(
    State(state): State<Arc<AppState>>,
    Json(input): Json<SetAppearanceRequest>,
) -> Result<Json<AppearanceResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(ref color) = input.accent_color {
        if !RE_HEX_COLOR.is_match(color) {
            return Err(StatusCode::BAD_REQUEST);
        }
        set_config_value(&conn, "accent_color", color)?;
    }

    if let Some(ref font) = input.font_family {
        if !ALLOWED_FONTS.contains(&font.as_str()) {
            return Err(StatusCode::BAD_REQUEST);
        }
        set_config_value(&conn, "font_family", font)?;
    }

    if let Some(ref font) = input.font_mono {
        if !ALLOWED_MONO_FONTS.contains(&font.as_str()) {
            return Err(StatusCode::BAD_REQUEST);
        }
        set_config_value(&conn, "font_mono", font)?;
    }

    Ok(Json(AppearanceResponse {
        accent_color: get_config_value(&conn, "accent_color", "#d4af37"),
        font_family: get_config_value(&conn, "font_family", "Inter"),
        font_mono: get_config_value(&conn, "font_mono", "System Mono"),
    }))
}
