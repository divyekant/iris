pub mod hub;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use std::sync::Arc;

use crate::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    match crate::api::session_auth::extract_session_token(&headers) {
        Some((token, transport)) if token == state.session_token => {
            if transport == crate::api::session_auth::SessionTransport::Cookie
                && !crate::api::session_auth::is_same_origin_browser_context(&headers)
            {
                return Err(StatusCode::FORBIDDEN);
            }
        }
        _ => return Err(StatusCode::UNAUTHORIZED),
    }

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state)))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.ws_hub.subscribe();

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(text) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "WebSocket client lagged, continuing");
                        continue;
                    }
                    Err(_) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Ping(data))) => {
                        let _ = socket.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
