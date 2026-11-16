//! The local API of Iris (tracker item R1-17): read-only JSON from the store
//! for the web client and the read verbs. `crates/iris-api/README.md` gives
//! the endpoints.
//!
//! Release 1 has no sign-in on the API, so the router refuses each request
//! whose `Host` or `Origin` is not local. A web page on another site cannot
//! read the API through DNS rebinding.

use std::fmt;
use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    extract::{rejection::QueryRejection, Path, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get},
    Json, Router,
};
use iris_store::Store;
use serde::Deserialize;

mod read;
pub use read::*;

#[derive(Debug)]
pub enum Error {
    Store(iris_store::Error),
    /// The input is not valid: a search text, an ID or a filter.
    Query(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Store(e) => write!(f, "{e}"),
            Error::Query(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<iris_store::Error> for Error {
    fn from(e: iris_store::Error) -> Error {
        match e {
            iris_store::Error::Query(why) => Error::Query(why),
            e => Error::Store(e),
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let code = match self {
            Error::Query(_) => StatusCode::BAD_REQUEST,
            Error::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        error(code, &self.to_string())
    }
}

fn error(code: StatusCode, text: &str) -> Response {
    (code, Json(serde_json::json!({ "error": text }))).into_response()
}

fn not_found() -> Response {
    error(StatusCode::NOT_FOUND, "not found")
}

/// The routes of the API, under `/api`. Merge them before the fallback of
/// `iris_web::router()`.
pub fn router(store: Arc<Store>) -> Router {
    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/brief", get(get_brief))
        .route("/api/items", get(get_items))
        .route("/api/items/{id}", get(get_item))
        .route("/api/search", get(get_search))
        .route("/api/messages/{id}", get(get_message))
        .route("/api/messages/{id}/raw", get(get_raw))
        // No fallback: `iris_web::router()` has the fallback of the server.
        .route("/api", any(|| async { not_found() }))
        .route("/api/{*rest}", any(|| async { not_found() }))
        .with_state(store)
        .layer(middleware::from_fn(local_only))
}

// shortcut: the handlers read SQLite on the runtime thread (searches take
// up to about 50 ms at 100k messages); move them to spawn_blocking if the
// API gets more than one client at a time.

async fn get_status(State(s): State<Arc<Store>>) -> Result<Response> {
    Ok(Json(status(&s)?).into_response())
}

async fn get_brief(State(s): State<Arc<Store>>) -> Result<Response> {
    Ok(Json(brief(&s, now_ms())?).into_response())
}

#[derive(Deserialize)]
struct ItemsQuery {
    form: Option<String>,
    status: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    before: Option<String>,
    limit: Option<u32>,
}

/// A query string that does not parse gives the JSON error of the API.
fn query<T>(q: std::result::Result<Query<T>, QueryRejection>) -> Result<T> {
    q.map(|Query(v)| v).map_err(|e| Error::Query(e.body_text()))
}

async fn get_items(
    State(s): State<Arc<Store>>,
    q: std::result::Result<Query<ItemsQuery>, QueryRejection>,
) -> Result<Response> {
    let q = query(q)?;
    let filter = Filter {
        form: q.form.as_deref(),
        status: q.status.as_deref(),
        kind: q.kind.as_deref(),
        before: q.before.as_deref(),
    };
    Ok(Json(items(&s, &filter, q.limit.unwrap_or(50))?).into_response())
}

async fn get_item(State(s): State<Arc<Store>>, Path(id): Path<String>) -> Result<Response> {
    Ok(item(&s, &id)?.map_or_else(not_found, |d| Json(d).into_response()))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    limit: Option<u32>,
}

async fn get_search(
    State(s): State<Arc<Store>>,
    q: std::result::Result<Query<SearchQuery>, QueryRejection>,
) -> Result<Response> {
    let q = query(q)?;
    let messages = search(&s, &q.q, q.limit.unwrap_or(50))?;
    Ok(Json(serde_json::json!({ "messages": messages })).into_response())
}

async fn get_message(State(s): State<Arc<Store>>, Path(id): Path<String>) -> Result<Response> {
    Ok(message(&s, &id)?.map_or_else(not_found, |m| Json(m).into_response()))
}

async fn get_raw(State(s): State<Arc<Store>>, Path(id): Path<String>) -> Result<Response> {
    let Some(path) = raw(&s, &id)? else {
        return Ok(not_found());
    };
    let file = std::fs::File::open(path).map_err(|e| Error::Store(e.into()))?;
    // Stream in parts of 64 KiB: a raw message can be tens of MB.
    let parts = futures_util::stream::unfold(file, |mut f| async move {
        let mut buf = vec![0; 64 * 1024];
        match std::io::Read::read(&mut f, &mut buf) {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok(Bytes::from(buf)), f))
            }
            Err(e) => Some((Err(e), f)),
        }
    });
    // A download, never a page: mail text must not run in the origin of Iris.
    let headers = [
        (header::CONTENT_TYPE, "message/rfc822".to_string()),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{id}.eml\""),
        ),
    ];
    Ok((headers, Body::from_stream(parts)).into_response())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Refuse each request with a `Host` or an `Origin` that is not local.
async fn local_only(req: Request, next: Next) -> Response {
    let mut res = if is_local(req.headers()) {
        next.run(req).await
    } else {
        error(StatusCode::FORBIDDEN, "only local requests are allowed")
    };
    let h = res.headers_mut();
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

fn is_local(headers: &HeaderMap) -> bool {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let Some(host) = host else {
        return false;
    };
    if !local_host(host) {
        return false;
    }
    match headers.get(header::ORIGIN) {
        None => true,
        Some(v) => v
            .to_str()
            .ok()
            .and_then(|o| {
                o.strip_prefix("http://")
                    .or_else(|| o.strip_prefix("https://"))
            })
            .is_some_and(local_host),
    }
}

/// `127.0.0.1`, `localhost` or `[::1]`, with an optional port.
fn local_host(text: &str) -> bool {
    let name = match text.rsplit_once(':') {
        Some((name, port)) if !text.ends_with(']') => {
            if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
                return false;
            }
            name
        }
        _ => text,
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "[::1]"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_host_accepts_only_loopback_names() {
        for ok in [
            "127.0.0.1",
            "127.0.0.1:4800",
            "localhost:5173",
            "LOCALHOST",
            "[::1]",
            "[::1]:4800",
        ] {
            assert!(local_host(ok), "{ok}");
        }
        for bad in [
            "evil.example",
            "127.0.0.1.evil.example",
            "localhost.evil.example:4800",
            "127.0.0.1:",
            "127.0.0.1:80x",
            "[::1]x",
            "0.0.0.0:4800",
            "",
        ] {
            assert!(!local_host(bad), "{bad}");
        }
    }
}
