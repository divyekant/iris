use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::account::{Account, CreateAccount};
use crate::AppState;

pub async fn list_accounts(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Account>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(Account::list(&conn)))
}

pub async fn get_account(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Account>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Account::get_by_id(&conn, &id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn create_account(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateAccount>,
) -> Result<(StatusCode, Json<Account>), StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let account = Account::create(&conn, &input);
    Ok((StatusCode::CREATED, Json(account)))
}

pub async fn delete_account(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Check that the account exists first
    Account::get_by_id(&conn, &id).ok_or(StatusCode::NOT_FOUND)?;

    conn.execute(
        "UPDATE accounts SET is_active = 0, updated_at = unixepoch() WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Per-account notification control
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct NotificationResponse {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct SetNotificationRequest {
    pub enabled: bool,
}

pub async fn get_notifications(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<NotificationResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Verify account exists
    Account::get_by_id(&conn, &id).ok_or(StatusCode::NOT_FOUND)?;

    let key = format!("notifications_{}", id);
    let value: String = conn
        .query_row(
            "SELECT value FROM config WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "enabled".to_string());

    Ok(Json(NotificationResponse {
        enabled: value != "disabled",
    }))
}

pub async fn set_notifications(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<SetNotificationRequest>,
) -> Result<Json<NotificationResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Verify account exists
    Account::get_by_id(&conn, &id).ok_or(StatusCode::NOT_FOUND)?;

    let key = format!("notifications_{}", id);
    let value = if input.enabled { "enabled" } else { "disabled" };

    conn.execute(
        "INSERT INTO config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        rusqlite::params![key, value],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(NotificationResponse {
        enabled: input.enabled,
    }))
}
