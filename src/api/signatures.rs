use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;

use crate::models::signature::{CreateSignature, Signature, UpdateSignature};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub account_id: String,
}

pub async fn list_signatures(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<Signature>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let sigs = Signature::list_for_account(&conn, &params.account_id);
    Ok(Json(sigs))
}

pub async fn create_signature(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateSignature>,
) -> Result<(StatusCode, Json<Signature>), (StatusCode, Json<serde_json::Value>)> {
    if input.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name is required"})),
        ));
    }
    if input.account_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "account_id is required"})),
        ));
    }

    let conn = state.db.get().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    let sig = Signature::create(&conn, &input);
    Ok((StatusCode::CREATED, Json(sig)))
}

pub async fn update_signature(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<UpdateSignature>,
) -> Result<Json<Signature>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match Signature::update(&conn, &id, &input) {
        Some(sig) => Ok(Json(sig)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn delete_signature(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if Signature::delete(&conn, &id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
