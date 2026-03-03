use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
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
