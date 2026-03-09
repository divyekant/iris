use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::label::{self, Label};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateLabelRequest {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateLabelRequest {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Serialize)]
pub struct LabelWithCount {
    #[serde(flatten)]
    pub label: Label,
    pub message_count: i64,
}

pub async fn list_labels(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<LabelWithCount>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let labels = label::list(&conn).map_err(|e| {
        tracing::error!("Failed to list labels: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let mut result = Vec::with_capacity(labels.len());
    for l in labels {
        let count = label::message_count(&conn, &l.name).unwrap_or(0);
        result.push(LabelWithCount { label: l, message_count: count });
    }
    Ok(Json(result))
}

pub async fn create_label(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateLabelRequest>,
) -> Result<(StatusCode, Json<Label>), StatusCode> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let color = body.color.trim();
    if color.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let l = label::create(&conn, name, color).map_err(|e| {
        tracing::error!("Failed to create label: {e}");
        // UNIQUE constraint violation = duplicate name
        if e.to_string().contains("UNIQUE") {
            StatusCode::CONFLICT
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })?;
    Ok((StatusCode::CREATED, Json(l)))
}

pub async fn update_label(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateLabelRequest>,
) -> Result<Json<Label>, StatusCode> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let l = label::update(&conn, &id, name, body.color.trim()).map_err(|e| {
        tracing::error!("Failed to update label: {e}");
        match e {
            rusqlite::Error::QueryReturnedNoRows => StatusCode::NOT_FOUND,
            _ => {
                if e.to_string().contains("UNIQUE") {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }
        }
    })?;
    Ok(Json(l))
}

pub async fn delete_label(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let deleted = label::delete(&conn, &id).map_err(|e| {
        tracing::error!("Failed to delete label: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
