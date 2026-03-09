use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

use crate::models::muted_thread;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct MuteStatusResponse {
    pub muted: bool,
}

/// PUT /api/threads/{id}/mute — mute a thread
pub async fn mute_thread(
    State(state): State<Arc<AppState>>,
    Path(thread_id): Path<String>,
) -> Result<Json<MuteStatusResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    muted_thread::mute(&conn, &thread_id);
    Ok(Json(MuteStatusResponse { muted: true }))
}

/// DELETE /api/threads/{id}/mute — unmute a thread
pub async fn unmute_thread(
    State(state): State<Arc<AppState>>,
    Path(thread_id): Path<String>,
) -> Result<Json<MuteStatusResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    muted_thread::unmute(&conn, &thread_id);
    Ok(Json(MuteStatusResponse { muted: false }))
}

/// GET /api/threads/{id}/mute — check if a thread is muted
pub async fn get_mute_status(
    State(state): State<Arc<AppState>>,
    Path(thread_id): Path<String>,
) -> Result<Json<MuteStatusResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let muted = muted_thread::is_muted(&conn, &thread_id);
    Ok(Json(MuteStatusResponse { muted }))
}

/// GET /api/muted-threads — list all muted thread IDs
pub async fn list_muted(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let muted = muted_thread::list_muted(&conn);
    Ok(Json(muted))
}
