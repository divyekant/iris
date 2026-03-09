use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;

use crate::models::saved_search;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateSavedSearchRequest {
    pub name: String,
    pub query: String,
    pub account_id: Option<String>,
}

pub async fn list_saved_searches(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<saved_search::SavedSearch>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let searches = saved_search::list(&conn).map_err(|e| {
        tracing::error!("Failed to list saved searches: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(searches))
}

pub async fn create_saved_search(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateSavedSearchRequest>,
) -> Result<(StatusCode, Json<saved_search::SavedSearch>), StatusCode> {
    let name = body.name.trim();
    let query = body.query.trim();

    if name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if query.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let search = saved_search::create(&conn, name, query, body.account_id.as_deref())
        .map_err(|e| {
            tracing::error!("Failed to create saved search: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok((StatusCode::CREATED, Json(search)))
}

pub async fn delete_saved_search(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let deleted = saved_search::delete(&conn, &id).map_err(|e| {
        tracing::error!("Failed to delete saved search: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
