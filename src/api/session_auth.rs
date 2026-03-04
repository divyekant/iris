use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::sync::Arc;

use crate::AppState;

const SESSION_TOKEN_HEADER: &str = "x-session-token";

/// Middleware that checks X-Session-Token header against the startup token.
pub async fn session_auth_middleware(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(SESSION_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok());

    match token {
        Some(t) if t == state.session_token => next.run(request).await,
        _ => StatusCode::UNAUTHORIZED.into_response(),
    }
}

#[derive(Serialize)]
pub struct BootstrapResponse {
    pub token: String,
}

/// Returns the session token to same-origin browser requests only.
pub async fn bootstrap_token(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
) -> Result<Json<BootstrapResponse>, StatusCode> {
    // Only allow same-origin requests (browser Fetch API sets this)
    let fetch_site = request
        .headers()
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok());

    match fetch_site {
        Some("same-origin") | Some("same-site") | Some("none") => {
            Ok(Json(BootstrapResponse {
                token: state.session_token.clone(),
            }))
        }
        _ => Err(StatusCode::FORBIDDEN),
    }
}
