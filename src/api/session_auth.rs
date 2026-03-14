use axum::{
    body::Body,
    extract::State,
    http::{
        header::{HOST, ORIGIN, REFERER, SET_COOKIE},
        HeaderMap, Method, Request, StatusCode,
    },
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::sync::Arc;

use crate::AppState;

pub const SESSION_TOKEN_HEADER: &str = "x-session-token";
pub const SESSION_COOKIE_NAME: &str = "iris_session";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionTransport {
    Header,
    Cookie,
}

pub fn extract_session_token(headers: &HeaderMap) -> Option<(String, SessionTransport)> {
    if let Some(token) = headers
        .get(SESSION_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
    {
        return Some((token.to_string(), SessionTransport::Header));
    }

    extract_cookie(headers, SESSION_COOKIE_NAME).map(|token| (token, SessionTransport::Cookie))
}

pub fn is_same_origin_browser_context(headers: &HeaderMap) -> bool {
    let fetch_site = headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok());

    if let Some(site) = fetch_site {
        if !matches!(site, "same-origin" | "none") {
            return false;
        }
    }

    let Some(expected_origin) = expected_origin(headers) else {
        return fetch_site.is_some();
    };

    if let Some(origin) = headers.get(ORIGIN).and_then(|v| v.to_str().ok()) {
        if origin != expected_origin {
            return false;
        }
    }

    if let Some(referer) = headers.get(REFERER).and_then(|v| v.to_str().ok()) {
        let same_origin = referer == expected_origin
            || referer
                .strip_prefix(&expected_origin)
                .is_some_and(|suffix| suffix.starts_with('/'));
        if !same_origin {
            return false;
        }
    }

    fetch_site.is_some() || headers.contains_key(ORIGIN) || headers.contains_key(REFERER)
}

pub fn build_session_cookie(token: &str) -> String {
    format!(
        "{SESSION_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict"
    )
}

fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    cookie_header.split(';').find_map(|part| {
        let (cookie_name, cookie_value) = part.trim().split_once('=')?;
        (cookie_name == name).then(|| cookie_value.to_string())
    })
}

fn expected_origin(headers: &HeaderMap) -> Option<String> {
    let host = headers.get(HOST).and_then(|v| v.to_str().ok())?;
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    Some(format!("{scheme}://{host}"))
}

fn is_safe_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Middleware that checks X-Session-Token header against the startup token.
pub async fn session_auth_middleware(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let auth = extract_session_token(request.headers());

    match auth {
        Some((token, transport)) if token == state.session_token => {
            if transport == SessionTransport::Cookie
                && !is_safe_method(request.method())
                && !is_same_origin_browser_context(request.headers())
            {
                return StatusCode::FORBIDDEN.into_response();
            }

            next.run(request).await
        }
        _ => StatusCode::UNAUTHORIZED.into_response(),
    }
}

#[derive(Serialize)]
pub struct BootstrapResponse {
    pub authenticated: bool,
}

/// Issues an HttpOnly session cookie for same-origin browser requests only.
pub async fn bootstrap_token(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
) -> Result<impl IntoResponse, StatusCode> {
    if !is_same_origin_browser_context(request.headers()) {
        return Err(StatusCode::FORBIDDEN);
    }

    let cookie = build_session_cookie(&state.session_token);
    let response = (
        [(SET_COOKIE, cookie)],
        Json(BootstrapResponse {
            authenticated: true,
        }),
    );

    Ok(response)
}
