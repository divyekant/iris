use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;

use crate::api::trust;
use crate::models::message::MessageDetail;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct Participant {
    pub email: String,
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ThreadMessageResponse {
    #[serde(flatten)]
    pub message: MessageDetail,
    pub trust: trust::TrustIndicators,
    pub tracking_pixels: Vec<trust::TrackingPixel>,
}

#[derive(Debug, Serialize)]
pub struct ThreadResponse {
    pub thread_id: String,
    pub subject: Option<String>,
    pub participants: Vec<Participant>,
    pub message_count: usize,
    pub messages: Vec<ThreadMessageResponse>,
}

pub async fn get_thread(
    State(state): State<Arc<AppState>>,
    Path(thread_id): Path<String>,
) -> Result<Json<ThreadResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let messages = MessageDetail::list_by_thread(&conn, &thread_id);

    if messages.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    let subject = messages[0].subject.clone();

    let mut seen = HashSet::new();
    let mut participants = Vec::new();
    for msg in &messages {
        if let Some(ref email) = msg.from_address {
            if seen.insert(email.clone()) {
                participants.push(Participant {
                    email: email.clone(),
                    name: msg.from_name.clone(),
                });
            }
        }
    }

    // Batch-query raw_headers for all messages in this thread
    let message_ids: Vec<&str> = messages.iter().map(|m| m.id.as_str()).collect();
    let placeholders: Vec<String> = (1..=message_ids.len()).map(|i| format!("?{i}")).collect();
    let in_clause = placeholders.join(", ");
    let query = format!("SELECT id, raw_headers FROM messages WHERE id IN ({in_clause})");
    let mut stmt = conn
        .prepare(&query)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let params: Vec<&dyn rusqlite::types::ToSql> =
        message_ids.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();
    let headers_map: std::collections::HashMap<String, Option<String>> = stmt
        .query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let message_count = messages.len();
    let enriched: Vec<ThreadMessageResponse> = messages
        .into_iter()
        .map(|msg| {
            let raw_headers = headers_map
                .get(&msg.id)
                .and_then(|h| h.as_deref())
                .unwrap_or("");
            let trust_indicators = trust::extract_trust_indicators(raw_headers);
            let tracking_pixels =
                trust::detect_tracking_pixels(msg.body_html.as_deref().unwrap_or(""));
            ThreadMessageResponse {
                message: msg,
                trust: trust_indicators,
                tracking_pixels,
            }
        })
        .collect();

    Ok(Json(ThreadResponse {
        thread_id,
        subject,
        participants,
        message_count,
        messages: enriched,
    }))
}
