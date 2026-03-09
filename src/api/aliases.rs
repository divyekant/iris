use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;

use crate::models::alias::{self, Alias};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateAliasRequest {
    pub account_id: String,
    pub email: String,
    pub display_name: String,
    pub reply_to: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAliasRequest {
    pub email: String,
    pub display_name: String,
    pub reply_to: Option<String>,
    pub is_default: bool,
}

pub async fn list_aliases(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<Alias>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let account_id = params.get("account_id").map(|s| s.as_str());
    let aliases = alias::list(&conn, account_id).map_err(|e| {
        tracing::error!("Failed to list aliases: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(aliases))
}

pub async fn create_alias(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateAliasRequest>,
) -> Result<(StatusCode, Json<Alias>), StatusCode> {
    let email = body.email.trim();
    if email.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if body.account_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let a = alias::create(
        &conn,
        &body.account_id,
        email,
        &body.display_name,
        body.reply_to.as_deref(),
        body.is_default,
    )
    .map_err(|e| {
        tracing::error!("Failed to create alias: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok((StatusCode::CREATED, Json(a)))
}

pub async fn update_alias(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateAliasRequest>,
) -> Result<Json<Alias>, StatusCode> {
    let email = body.email.trim();
    if email.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let a = alias::update(
        &conn,
        &id,
        email,
        &body.display_name,
        body.reply_to.as_deref(),
        body.is_default,
    )
    .map_err(|e| {
        tracing::error!("Failed to update alias: {e}");
        match e {
            rusqlite::Error::QueryReturnedNoRows => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    })?;
    Ok(Json(a))
}

pub async fn delete_alias(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let deleted = alias::delete(&conn, &id).map_err(|e| {
        tracing::error!("Failed to delete alias: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
