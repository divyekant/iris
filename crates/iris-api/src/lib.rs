//! The local API of Iris (tracker item R1-17): read-only JSON from the store
//! for the web client and the read verbs. `crates/iris-api/README.md` gives
//! the endpoints.
//!
//! The guard of [`Auth`] refuses each request whose `Host` is not the bound
//! address, or that has no bearer token of the sign-in.

use std::fmt;
use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    extract::{rejection::QueryRejection, Path, Query, State},
    http::{header, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{any, get},
    Json, Router,
};
use iris_store::Store;
use serde::Deserialize;

mod auth;
mod read;
pub use auth::{Auth, SIGN_IN};
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

pub(crate) fn error(code: StatusCode, text: &str) -> Response {
    (code, Json(serde_json::json!({ "error": text }))).into_response()
}

fn not_found() -> Response {
    error(StatusCode::NOT_FOUND, "not found")
}

/// The routes of the API, under `/api`. Merge them before the fallback of
/// `iris_web::router()`. Each request needs the bearer token of the sign-in
/// of `auth` (see [`Auth`]).
pub fn router(store: Arc<Store>, auth: Arc<Auth>) -> Router {
    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/brief", get(get_brief))
        .route("/api/brief/{section}", get(get_brief_section))
        .route("/api/proposal", get(get_proposal))
        .route("/api/items", get(get_items))
        .route("/api/items/{id}", get(get_item))
        .route("/api/search", get(get_search))
        .route("/api/messages/{id}", get(get_message))
        .route("/api/messages/{id}/raw", get(get_raw))
        // No fallback: `iris_web::router()` has the fallback of the server.
        .route("/api", any(|| async { not_found() }))
        .route("/api/{*rest}", any(|| async { not_found() }))
        .with_state(store)
        // The code exchange has no handler: the guard answers it.
        .route(SIGN_IN, any(|| async { not_found() }))
        .layer(middleware::from_fn_with_state(auth, auth::guard))
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

async fn get_proposal(State(s): State<Arc<Store>>) -> Response {
    // The changeset streams from a blocking thread through a channel of a
    // few parts, so memory stays flat for a large proposal.
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let now = now_ms();
    tokio::task::spawn_blocking(move || {
        let mut w = ChanWriter {
            buf: Vec::with_capacity(CHUNK),
            tx,
        };
        let res = write_proposal(&s, now, &mut w)
            .and_then(|()| std::io::Write::flush(&mut w).map_err(|e| Error::Store(e.into())));
        if let Err(e) = res {
            // The status is sent; an error ends the body, so the JSON is cut.
            let _ =
                w.tx.blocking_send(Err(std::io::Error::other(e.to_string())));
        }
    });
    // The status waits for the first part: an error before it is a 500.
    let mut rx = rx;
    let first = match rx.recv().await {
        Some(Ok(part)) => part,
        Some(Err(e)) => return error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
        None => return error(StatusCode::INTERNAL_SERVER_ERROR, "the proposal stopped"),
    };
    let rest = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|part| (part, rx))
    });
    let parts = futures_util::StreamExt::chain(futures_util::stream::iter([Ok(first)]), rest);
    (
        [(header::CONTENT_TYPE, "application/json")],
        Body::from_stream(parts),
    )
        .into_response()
}

/// The size of one part of a streamed answer.
const CHUNK: usize = 64 * 1024;

/// A writer that sends parts of [`CHUNK`] bytes to a channel.
struct ChanWriter {
    buf: Vec<u8>,
    tx: tokio::sync::mpsc::Sender<std::io::Result<Bytes>>,
}

impl std::io::Write for ChanWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        if self.buf.len() >= CHUNK {
            self.flush()?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let part = Bytes::from(std::mem::replace(&mut self.buf, Vec::with_capacity(CHUNK)));
        self.tx
            .blocking_send(Ok(part))
            .map_err(|_| std::io::Error::other("the client closed the connection"))
    }
}

#[derive(Deserialize)]
struct PageQuery {
    limit: Option<u32>,
    offset: Option<u64>,
}

async fn get_brief_section(
    State(s): State<Arc<Store>>,
    Path(section): Path<String>,
    q: std::result::Result<Query<PageQuery>, QueryRejection>,
) -> Result<Response> {
    let q = query(q)?;
    let page = brief_section(
        &s,
        now_ms(),
        &section,
        q.limit.unwrap_or(50),
        q.offset.unwrap_or(0),
    )?;
    Ok(page.map_or_else(not_found, |p| Json(p).into_response()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chan_writer_sends_parts_of_chunk_size() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let data: Vec<u8> = (0..CHUNK * 3 + 100).map(|i| i as u8).collect();
        std::thread::spawn(move || {
            let mut w = ChanWriter {
                buf: Vec::new(),
                tx,
            };
            for part in data.chunks(1000) {
                std::io::Write::write_all(&mut w, part).unwrap();
            }
            std::io::Write::flush(&mut w).unwrap();
        })
        .join()
        .unwrap();
        let mut got = Vec::new();
        let mut parts = 0;
        while let Ok(part) = rx.try_recv() {
            let part = part.unwrap();
            assert!(part.len() < CHUNK + 1000);
            got.extend_from_slice(&part);
            parts += 1;
        }
        assert!(parts >= 3, "{parts}");
        assert_eq!(
            got,
            (0..CHUNK * 3 + 100).map(|i| i as u8).collect::<Vec<u8>>()
        );
    }
}
