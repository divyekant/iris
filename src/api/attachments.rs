use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct AttachmentInfo {
    pub id: String,
    pub message_id: String,
    pub filename: Option<String>,
    pub content_type: String,
    pub size: i64,
    pub content_id: Option<String>,
}

/// GET /api/messages/:id/attachments — list attachments for a message (metadata only, no data).
pub async fn list_attachments(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> Result<Json<Vec<AttachmentInfo>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut stmt = conn
        .prepare(
            "SELECT id, message_id, filename, content_type, size, content_id
             FROM attachments WHERE message_id = ?1",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let attachments: Vec<AttachmentInfo> = stmt
        .query_map(rusqlite::params![message_id], |row| {
            Ok(AttachmentInfo {
                id: row.get("id")?,
                message_id: row.get("message_id")?,
                filename: row.get("filename")?,
                content_type: row.get("content_type")?,
                size: row.get("size")?,
                content_id: row.get("content_id")?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(attachments))
}

/// GET /api/attachments/:id/download — download attachment binary.
pub async fn download_attachment(
    State(state): State<Arc<AppState>>,
    Path(attachment_id): Path<String>,
) -> Result<Response, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let row = conn
        .query_row(
            "SELECT filename, content_type, data FROM attachments WHERE id = ?1",
            rusqlite::params![attachment_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>("filename")?,
                    row.get::<_, String>("content_type")?,
                    row.get::<_, Vec<u8>>("data")?,
                ))
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let (filename, content_type, data) = row;
    let safe_filename = filename.unwrap_or_else(|| "download".to_string());

    // Sanitize filename for Content-Disposition
    let disposition = format!("attachment; filename=\"{}\"", safe_filename.replace('"', "'"));

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::CACHE_CONTROL, "private, max-age=86400")
        .body(Body::from(data))
        .unwrap())
}
