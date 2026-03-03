use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;

use crate::models::message::MessageDetail;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct Participant {
    pub email: String,
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ThreadResponse {
    pub thread_id: String,
    pub subject: Option<String>,
    pub participants: Vec<Participant>,
    pub message_count: usize,
    pub messages: Vec<MessageDetail>,
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

    Ok(Json(ThreadResponse {
        thread_id,
        subject,
        participants,
        message_count: messages.len(),
        messages,
    }))
}
