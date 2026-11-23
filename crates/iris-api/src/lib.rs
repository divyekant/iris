//! The local API of Iris (tracker item R1-17): JSON from the store
//! for the web client and the read verbs. `crates/iris-api/README.md` gives
//! the endpoints.
//!
//! The guard of [`Auth`] refuses each request whose `Host` is not the bound
//! address, or that has no bearer token of the sign-in.

use std::fmt;
use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    extract::{
        rejection::{JsonRejection, QueryRejection},
        Path, Query, State,
    },
    http::{header, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use iris_store::Store;
use serde::Deserialize;

mod ask;
mod auth;
mod drafts;
mod export;
pub mod gate;
mod jobs;
mod ledger;
mod matters;
pub mod measures;
mod notice;
mod outbox;
pub mod policy;
mod read;
mod review;
mod verbs;
mod watch;
mod write;
pub use ask::{answer, AskAnswer, AskSource, Connect};
pub use auth::{Auth, CODE_LIFE, SIGN_IN};
pub use drafts::write_drafts;
pub use export::{export, Exported};
pub use jobs::*;
pub use ledger::{ledger, undo_entry, LedgerEntry, LedgerPage};
pub use matters::{confirm_matter, decline_matter, matter, matters};
pub use notice::*;
pub use outbox::{cancel_send, outbox, Cancelled, MAX_SENDS};
pub use read::*;
pub use review::*;
pub use verbs::*;
pub use watch::*;

#[derive(Debug)]
pub enum Error {
    Store(iris_store::Error),
    /// The input is not valid: a search text, an ID or a filter.
    Query(String),
    /// The request cannot apply to the state of now, for example a policy
    /// that changed after the person saw it.
    Conflict(String),
    /// The engine cannot do the request now, for example with no model or
    /// no budget.
    Unavailable(String),
    /// The model failed or gave an answer that is not valid.
    Model(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Store(e) => write!(f, "{e}"),
            Error::Query(why)
            | Error::Conflict(why)
            | Error::Unavailable(why)
            | Error::Model(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<iris_store::Error> for Error {
    fn from(e: iris_store::Error) -> Error {
        match e {
            iris_store::Error::Query(why) => Error::Query(why),
            e @ iris_store::Error::Watch(_) => Error::Query(e.to_string()),
            e => Error::Store(e),
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let code = match self {
            Error::Query(_) => StatusCode::BAD_REQUEST,
            Error::Conflict(_) => StatusCode::CONFLICT,
            Error::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Error::Model(_) => StatusCode::BAD_GATEWAY,
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

/// The routes of the API, under `/api`, with no model: `POST /api/ask`
/// answers 503. See [`router_with_model`].
pub fn router(store: Arc<Store>, auth: Arc<Auth>) -> Router {
    routes(store, auth, None)
}

/// The routes of the API, under `/api`. Merge them before the fallback of
/// `iris_web::router()`. Each request needs the bearer token of the sign-in
/// of `auth` (see [`Auth`]). `connect` makes the model client of
/// `POST /api/ask`.
pub fn router_with_model(store: Arc<Store>, auth: Arc<Auth>, connect: Connect) -> Router {
    routes(store, auth, Some(connect))
}

fn routes(store: Arc<Store>, auth: Arc<Auth>, connect: Option<Connect>) -> Router {
    let ask = Router::new()
        .route("/api/ask", post(post_ask))
        .route("/api/threads/{id}/draft", post(post_draft))
        .with_state((store.clone(), connect));
    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/brief", get(get_brief))
        .route("/api/brief/{section}", get(get_brief_section))
        .route("/api/handled", get(get_handled))
        .route("/api/ledger", get(get_ledger))
        .route("/api/ledger/{id}/undo", post(post_undo))
        .route("/api/gate", get(get_gate))
        .route("/api/gate/declined", get(get_declined))
        .route("/api/senders/{id}/{answer}", post(post_answer))
        .route("/api/records", get(get_records))
        .route("/api/items", get(get_items))
        .route("/api/items/{id}", get(get_item))
        .route("/api/items/{id}/job", get(get_start))
        .route("/api/items/{id}/{verb}", post(post_verb))
        .route("/api/search", get(get_search))
        .route("/api/changesets", get(get_changesets))
        .route("/api/changesets/{id}", get(get_changeset))
        .route("/api/changesets/{id}/drafts", get(get_drafts))
        .route("/api/changesets/{id}/decide", post(post_decide))
        .route("/api/changesets/{id}/withdraw", post(post_withdraw))
        .route(
            "/api/changesets/{id}/comments",
            get(get_comments).post(post_comment),
        )
        .route("/api/messages", get(get_messages))
        .route("/api/messages/{id}", get(get_message))
        .route("/api/messages/{id}/raw", get(get_raw))
        .route("/api/measures", get(get_measures))
        .route("/api/policy", get(get_policy))
        .route("/api/policy/confirm", post(post_confirm))
        .route("/api/policy/discard", post(post_discard))
        .route("/api/policy/mode", post(post_mode))
        .route("/api/policy/remove", post(post_remove))
        .route("/api/policy/grant", post(post_grant))
        .route("/api/jobs", get(get_jobs).post(post_job))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/jobs/{id}/answer", post(post_job_answer))
        .route("/api/policy/accept", post(post_accept))
        .route("/api/policy/dismiss", post(post_dismiss))
        .route("/api/notices", get(get_notices))
        .route("/api/notices/{id}/done", post(post_notice_done))
        .route("/api/watches", get(get_watches).post(post_watch))
        .route("/api/watches/{id}/remove", post(post_watch_remove))
        .route("/api/stop", get(get_stop).post(post_stop))
        .route("/api/resume", post(post_resume))
        .route("/api/outbox", get(get_outbox))
        .route("/api/outbox/{draft}/cancel", post(post_outbox_cancel))
        .route("/api/write", post(post_write))
        .route("/api/write/{id}", get(get_write))
        .route("/api/threads/{id}/reply", get(get_reply))
        .route("/api/threads/{id}/draft", get(get_draft))
        .route("/api/matters", get(get_matters))
        .route("/api/matters/{id}", get(get_matter))
        .route("/api/matters/{id}/confirm", post(post_matter_confirm))
        .route("/api/matters/{id}/decline", post(post_matter_decline))
        // No fallback: `iris_web::router()` has the fallback of the server.
        .route("/api", any(|| async { not_found() }))
        .route("/api/{*rest}", any(|| async { not_found() }))
        .with_state(store.clone())
        .merge(ask)
        // The code exchange has no handler: the guard answers it.
        .route(SIGN_IN, any(|| async { not_found() }))
        // Inside the guard: only a request with the sign-in counts.
        .layer(middleware::from_fn_with_state(
            measures::Counter::new(store),
            measures::count,
        ))
        .layer(middleware::from_fn_with_state(auth, auth::guard))
}

// shortcut: the handlers read SQLite on the runtime thread (searches take
// up to about 50 ms at 100k messages); move them to spawn_blocking if the
// API gets more than one client at a time.

async fn get_status(State(s): State<Arc<Store>>) -> Result<Response> {
    Ok(Json(status(&s)?).into_response())
}

async fn get_records(State(s): State<Arc<Store>>) -> Result<Response> {
    Ok(Json(records(&s)?).into_response())
}

async fn get_brief(State(s): State<Arc<Store>>) -> Result<Response> {
    let now = now_ms();
    let b = brief(&s, now)?;
    // The first brief with content ends the measure "time to the first brief".
    measures::mark_brief(&s, &b, now)?;
    Ok(Json(b).into_response())
}

async fn get_handled(
    State(s): State<Arc<Store>>,
    q: std::result::Result<Query<PageQuery>, QueryRejection>,
) -> Result<Response> {
    let q = query(q)?;
    let page = handled(&s, q.limit.unwrap_or(50), q.after.as_deref())?;
    Ok(Json(page).into_response())
}

async fn get_ledger(
    State(s): State<Arc<Store>>,
    q: std::result::Result<Query<PageQuery>, QueryRejection>,
) -> Result<Response> {
    let q = query(q)?;
    Ok(Json(ledger(&s, q.limit.unwrap_or(50), q.after.as_deref())?).into_response())
}

/// The person undoes a ledger entry (specification 2, section 7.4).
async fn post_undo(
    State(s): State<Arc<Store>>,
    Path(id): Path<String>,
    _: NoBody,
) -> Result<Response> {
    Ok(undo_entry(&s, &id, now_ms())?.map_or_else(not_found, |e| Json(e).into_response()))
}

async fn get_changesets(State(s): State<Arc<Store>>) -> Response {
    stream(move |w| write_open_changesets(&s, w)).await
}

async fn get_changeset(State(s): State<Arc<Store>>, Path(id): Path<String>) -> Result<Response> {
    let Some(row) = changeset_row(&s, &id)? else {
        return Ok(not_found());
    };
    Ok(
        stream(move |w| {
            iris_changeset::write_document(&s, row, w).map_err(review::changeset_error)
        })
        .await,
    )
}

/// The draft of each `draft` and `send` action that waits, with the parts
/// that gave its flags (card R4-07).
async fn get_drafts(State(s): State<Arc<Store>>, Path(id): Path<String>) -> Result<Response> {
    let Some(row) = changeset_row(&s, &id)? else {
        return Ok(not_found());
    };
    Ok(stream(move |w| drafts::write_drafts(&s, row, w)).await)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecideBody {
    actions: Vec<Decision>,
}

/// The decision of the person on one changeset. The answer is the
/// changeset after the decision.
async fn post_decide(
    State(s): State<Arc<Store>>,
    Path(id): Path<String>,
    b: std::result::Result<Json<DecideBody>, JsonRejection>,
) -> Result<Response> {
    let Some(row) = changeset_row(&s, &id)? else {
        return Ok(not_found());
    };
    let b = body(b)?;
    decide(&s, row, &b.actions, now_ms())?;
    Ok(
        stream(move |w| {
            iris_changeset::write_document(&s, row, w).map_err(review::changeset_error)
        })
        .await,
    )
}

/// The withdraw of the person (section 5.5). The answer is the changeset
/// after the withdraw. The request has no body.
async fn post_withdraw(
    State(s): State<Arc<Store>>,
    Path(id): Path<String>,
    _: NoBody,
) -> Result<Response> {
    let Some(row) = changeset_row(&s, &id)? else {
        return Ok(not_found());
    };
    review::withdraw(&s, row)?;
    Ok(
        stream(move |w| {
            iris_changeset::write_document(&s, row, w).map_err(review::changeset_error)
        })
        .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommentBody {
    revision: i64,
    text: String,
    key: String,
}

/// A review comment of the person on the newest revision of a changeset of
/// a job (docs/specs/2026-11-17-review-comments.md). The answer is the
/// comment. A retry with the same key gives the same comment.
async fn post_comment(
    State(s): State<Arc<Store>>,
    Path(id): Path<String>,
    b: std::result::Result<Json<CommentBody>, JsonRejection>,
) -> Result<Response> {
    let Some(row) = changeset_row(&s, &id)? else {
        return Ok(not_found());
    };
    let b = body(b)?;
    let c = review::comment(&s, row, b.revision, &b.text, &b.key, now_ms())?;
    Ok(Json(c).into_response())
}

/// Each review comment of a changeset, oldest first.
async fn get_comments(State(s): State<Arc<Store>>, Path(id): Path<String>) -> Result<Response> {
    let Some(row) = changeset_row(&s, &id)? else {
        return Ok(not_found());
    };
    Ok(Json(review::comments(&s, row)?).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskBody {
    question: String,
}

/// One question of the person. The model call runs on a blocking thread,
/// so the other requests go on.
async fn post_ask(
    State((s, connect)): State<(Arc<Store>, Option<Connect>)>,
    b: std::result::Result<Json<AskBody>, JsonRejection>,
) -> Result<Response> {
    let b = body(b)?;
    let connect =
        connect.ok_or_else(|| Error::Unavailable("This engine has no model client.".into()))?;
    let a = tokio::task::spawn_blocking(move || answer(&s, &connect, &b.question, now_ms()))
        .await
        .map_err(|e| Error::Store(std::io::Error::other(e.to_string()).into()))??;
    Ok(Json(a).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftBody {
    /// What the reply must say, in the words of the person. Empty: Iris
    /// writes the reply from the thread alone.
    #[serde(default)]
    intent: String,
    /// The mail that the form answers (`m-N`, the `parent` of the reply
    /// start values). A newer parent makes a 409 before the call.
    parent: String,
    /// The recipients of the form. The first one is the person of the
    /// reply. Empty: the person of the newest mail of the thread.
    #[serde(default)]
    to: Vec<String>,
}

/// A draft of the reply in the voice of the person (R3-04 intent path,
/// R3-05). The model call runs on a blocking thread, so the other requests
/// go on. The draft is saved in the store; nothing goes to Gmail.
async fn post_draft(
    State((s, connect)): State<(Arc<Store>, Option<Connect>)>,
    Path(id): Path<String>,
    b: std::result::Result<Json<DraftBody>, JsonRejection>,
) -> Result<Response> {
    let b = body(b)?;
    let connect =
        connect.ok_or_else(|| Error::Unavailable("This engine has no model client.".into()))?;
    let d = tokio::task::spawn_blocking(move || {
        write::draft(&s, &connect, &id, &b.parent, &b.intent, &b.to, now_ms)
    })
    .await
    .map_err(|e| Error::Store(std::io::Error::other(e.to_string()).into()))??;
    Ok(d.map_or_else(not_found, |v| Json(v).into_response()))
}

#[derive(Deserialize)]
struct IdsQuery {
    ids: String,
}

async fn get_messages(
    State(s): State<Arc<Store>>,
    q: std::result::Result<Query<IdsQuery>, QueryRejection>,
) -> Result<Response> {
    let q = query(q)?;
    let messages = messages_by_id(&s, &q.ids)?;
    Ok(Json(serde_json::json!({ "messages": messages })).into_response())
}

/// Stream the JSON that `write` makes from a blocking thread, through a
/// channel of a few parts, so memory stays flat for a large answer.
async fn stream<F>(write: F) -> Response
where
    F: FnOnce(&mut ChanWriter) -> Result<()> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    tokio::task::spawn_blocking(move || {
        let mut w = ChanWriter {
            buf: Vec::with_capacity(CHUNK),
            tx,
        };
        let res = write(&mut w)
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
        None => return error(StatusCode::INTERNAL_SERVER_ERROR, "the answer stopped"),
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
    after: Option<String>,
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
        q.after.as_deref(),
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

/// A state verb of the person on one item: `close`, `decline`, `defer`,
/// `skip`, `mute` or `dismiss` (specification 2, section 4). The answer is the item after the change.
async fn post_verb(
    State(s): State<Arc<Store>>,
    Path((id, verb)): Path<(String, String)>,
    b: std::result::Result<Json<VerbBody>, JsonRejection>,
) -> Result<Response> {
    if !matches!(
        verb.as_str(),
        "close" | "decline" | "defer" | "skip" | "mute" | "dismiss"
    ) {
        return Ok(not_found());
    }
    let b = body(b)?;
    Ok(apply_verb(&s, &id, &verb, &b, now_ms())?
        .map_or_else(not_found, |d| Json(d).into_response()))
}

/// The senders that wait at the stranger gate.
async fn get_gate(
    State(s): State<Arc<Store>>,
    q: std::result::Result<Query<PageQuery>, QueryRejection>,
) -> Result<Response> {
    let q = query(q)?;
    let page = gate::gate_page(
        &s,
        now_ms(),
        false,
        q.limit.unwrap_or(50),
        q.after.as_deref(),
    )?;
    Ok(Json(page).into_response())
}

/// The senders that the person declined at the gate.
async fn get_declined(
    State(s): State<Arc<Store>>,
    q: std::result::Result<Query<PageQuery>, QueryRejection>,
) -> Result<Response> {
    let q = query(q)?;
    let page = gate::gate_page(
        &s,
        now_ms(),
        true,
        q.limit.unwrap_or(50),
        q.after.as_deref(),
    )?;
    Ok(Json(page).into_response())
}

/// The answer of the person at the stranger gate: `accept`, `decline` or
/// `reopen` (specification 1, section 6.7). The answer is the sender row.
async fn post_answer(
    State(s): State<Arc<Store>>,
    Path((id, answer)): Path<(String, String)>,
    _: NoBody,
) -> Result<Response> {
    Ok(gate::answer(&s, &id, &answer, now_ms())?
        .map_or_else(not_found, |d| Json(d).into_response()))
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

async fn get_measures(State(s): State<Arc<Store>>) -> Result<Response> {
    let rss = tokio::task::spawn_blocking(measures::rss_kib)
        .await
        .ok()
        .flatten();
    Ok(Json(measures::measures(&s, rss)?).into_response())
}

async fn get_policy(State(s): State<Arc<Store>>) -> Result<Response> {
    Ok(Json(policy::policy(&s)?).into_response())
}

/// The body of a route that takes none: zero bytes, or a JSON text that is
/// the empty object `{}` (the web client sends `{}`). `serde_json` parses
/// the JSON. Any other body, a body of only spaces too, gives 400 before the
/// route changes anything. The read stops at the first frame past
/// `NO_BODY_MAX` bytes, so a large body does not stay in memory.
struct NoBody;

/// The longest body that a route with no body reads: `{}` with some space.
const NO_BODY_MAX: usize = 64;

impl<S: Send + Sync> axum::extract::FromRequest<S> for NoBody {
    type Rejection = Error;

    async fn from_request(req: axum::extract::Request, _: &S) -> Result<Self> {
        let b = axum::body::to_bytes(req.into_body(), NO_BODY_MAX)
            .await
            .map_err(|_| Error::Query("this route takes no body".into()))?;
        let none = b.is_empty()
            || serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&b)
                .is_ok_and(|m| m.is_empty());
        if none {
            Ok(NoBody)
        } else {
            Err(Error::Query("this route takes no body".into()))
        }
    }
}

/// A JSON body that does not parse gives the JSON error of the API.
fn body<T>(b: std::result::Result<Json<T>, JsonRejection>) -> Result<T> {
    b.map(|Json(v)| v).map_err(|e| Error::Query(e.body_text()))
}

#[derive(Deserialize)]
struct ConfirmBody {
    sha256: String,
}

/// Runs `f` on a blocking thread. Each policy writer waits for the backup
/// lock (a backup can hold it for minutes), and the server has one runtime
/// thread: a wait there stops each other request (issue 143).
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| Error::Store(std::io::Error::other(e.to_string()).into()))?
}

/// [`policy::change`] on a blocking thread, see [`blocking`].
async fn change_blocking(
    s: Arc<Store>,
    version: Option<i64>,
    change: iris_policy::Change,
) -> Result<serde_json::Value> {
    blocking(move || policy::change(&s, version, change, now_s())).await
}

async fn post_confirm(
    State(s): State<Arc<Store>>,
    b: std::result::Result<Json<ConfirmBody>, JsonRejection>,
) -> Result<Response> {
    let b = body(b)?;
    let v = blocking(move || policy::confirm(&s, &b.sha256, now_s())).await?;
    Ok(Json(v).into_response())
}

async fn post_discard(State(s): State<Arc<Store>>, _: NoBody) -> Result<Response> {
    Ok(Json(blocking(move || policy::discard(&s)).await?).into_response())
}

/// A field that must be in the body, also when its value is `null`: with
/// `deserialize_with`, serde gives no default for an omitted `Option` (#145).
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<i64>, D::Error> {
    Option::deserialize(d)
}

/// `version` is the active version that the person saw, `null` for the
/// policy of the first start. A body with no `version` field is refused.
#[derive(Deserialize)]
struct ModeBody {
    #[serde(deserialize_with = "present")]
    version: Option<i64>,
    mode: String,
}

async fn post_mode(
    State(s): State<Arc<Store>>,
    b: std::result::Result<Json<ModeBody>, JsonRejection>,
) -> Result<Response> {
    let b = body(b)?;
    let change = iris_policy::Change::Mode(policy::mode(&b.mode)?);
    Ok(Json(change_blocking(s, b.version, change).await?).into_response())
}

#[derive(Deserialize)]
struct RemoveBody {
    #[serde(deserialize_with = "present")]
    version: Option<i64>,
    id: u32,
}

async fn post_remove(
    State(s): State<Arc<Store>>,
    b: std::result::Result<Json<RemoveBody>, JsonRejection>,
) -> Result<Response> {
    let b = body(b)?;
    let change = iris_policy::Change::Remove(b.id);
    Ok(Json(change_blocking(s, b.version, change).await?).into_response())
}

#[derive(Deserialize)]
struct JobsQuery {
    state: Option<String>,
    limit: Option<u32>,
    after: Option<String>,
}

async fn get_jobs(
    State(s): State<Arc<Store>>,
    q: std::result::Result<Query<JobsQuery>, QueryRejection>,
) -> Result<Response> {
    let q = query(q)?;
    let state = q.state.as_deref().unwrap_or("open");
    let page = jobs(&s, state, q.limit.unwrap_or(50), q.after.as_deref())?;
    Ok(Json(page).into_response())
}

/// The mail that a start from the item grants now.
async fn get_start(State(s): State<Arc<Store>>, Path(id): Path<String>) -> Result<Response> {
    Ok(start_preview(&s, &id)?.map_or_else(not_found, |v| Json(v).into_response()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartBody {
    item: String,
    goal: String,
    due: Option<String>,
    messages: Vec<String>,
}

/// Start a job from one item ("Handle this"). The answer is 201 with the
/// job document. If an open job has the item, 409 names that job.
async fn post_job(
    State(s): State<Arc<Store>>,
    b: std::result::Result<Json<StartBody>, JsonRejection>,
) -> Result<Response> {
    let b = body(b)?;
    Ok(
        match start_job(
            &s,
            &b.item,
            &b.goal,
            b.due.as_deref(),
            &b.messages,
            now_ms(),
        )? {
            Started::Created(doc) => (StatusCode::CREATED, Json(doc)).into_response(),
            Started::NotFound => not_found(),
            Started::Open(job) => (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "A job that is not done has this item already.",
                    "job": job,
                })),
            )
                .into_response(),
        },
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantBody {
    #[serde(deserialize_with = "present")]
    version: Option<i64>,
    job: String,
    verbs: Vec<String>,
}

/// A grant inside one open job: one `allow` rule with `job`.
async fn post_grant(
    State(s): State<Arc<Store>>,
    b: std::result::Result<Json<GrantBody>, JsonRejection>,
) -> Result<Response> {
    let b = body(b)?;
    Ok(
        blocking(move || policy::grant(&s, &b.job, &b.verbs, b.version, now_s()))
            .await?
            .map_or_else(not_found, |v| Json(v).into_response()),
    )
}

async fn get_job(State(s): State<Arc<Store>>, Path(id): Path<String>) -> Result<Response> {
    let Some(row) = job_row(&s, &id)? else {
        return Ok(not_found());
    };
    Ok(stream(move |w| write_job(&s, row, w)).await)
}

#[derive(Deserialize)]
struct JobAnswerBody {
    answer: String,
    /// The question that the person answers, and the `updated` of the job
    /// when the person saw it. An answer to an older question gets 409.
    question: String,
    updated: String,
}

/// The answer of the person to the question of a job (card R5-05). The
/// answer is the job document.
async fn post_job_answer(
    State(s): State<Arc<Store>>,
    Path(id): Path<String>,
    b: std::result::Result<Json<JobAnswerBody>, JsonRejection>,
) -> Result<Response> {
    let b = body(b)?;
    let Some(row) = job_row(&s, &id)? else {
        return Ok(not_found());
    };
    let asked = iris_job::Asked {
        question: &b.question,
        updated: &b.updated,
    };
    answer_job(&s, row, &b.answer, Some(asked), now_ms())?;
    Ok(stream(move |w| write_job(&s, row, w)).await)
}

/// `id` is the ID of an open proposal (spec 2, section 6.6).
#[derive(Deserialize)]
struct ProposalBody {
    #[serde(deserialize_with = "present")]
    version: Option<i64>,
    id: i64,
}

async fn post_accept(
    State(s): State<Arc<Store>>,
    b: std::result::Result<Json<ProposalBody>, JsonRejection>,
) -> Result<Response> {
    let b = body(b)?;
    let change = iris_policy::Change::Accept(b.id);
    Ok(Json(change_blocking(s, b.version, change).await?).into_response())
}

#[derive(Deserialize)]
struct DismissBody {
    id: i64,
}

async fn post_dismiss(
    State(s): State<Arc<Store>>,
    b: std::result::Result<Json<DismissBody>, JsonRejection>,
) -> Result<Response> {
    let b = body(b)?;
    Ok(Json(policy::dismiss(&s, b.id, now_s())?).into_response())
}

async fn get_notices(State(s): State<Arc<Store>>) -> Result<Response> {
    Ok(Json(notices(&s)?).into_response())
}

async fn post_notice_done(
    State(s): State<Arc<Store>>,
    Path(id): Path<String>,
    _: NoBody,
) -> Result<Response> {
    if !notice_done(&s, &id, now_ms())? {
        return Ok(not_found());
    }
    Ok(Json(serde_json::json!({ "done": id })).into_response())
}

async fn get_outbox(
    State(s): State<Arc<Store>>,
    q: std::result::Result<Query<PageQuery>, QueryRejection>,
) -> Result<Response> {
    let q = query(q)?;
    Ok(Json(outbox(
        &s,
        q.limit.unwrap_or(MAX_SENDS),
        q.after.as_deref(),
    )?)
    .into_response())
}

/// `GET /api/stop`: `{"stopped": bool, "since": time|null, "sessions": 0,
/// "live": n, "busy": bool}` (docs/specs/2026-11-17-stop.md, section 5).
/// `live` and `busy` are measured now, also while Iris runs, so a client
/// says that no mail leaves only for a complete stop.
async fn get_stop(State(s): State<Arc<Store>>) -> Result<Response> {
    let since = s.read(iris_store::stopped)?;
    Ok(Json(stop_answer(&s, since)?).into_response())
}

/// The answer of each Stop route with no stop in it: `sessions` is 0, and
/// `live` and `busy` are measured now (issue 569).
fn stop_answer(s: &Store, since: Option<i64>) -> Result<serde_json::Value> {
    let (live, busy) = iris_apply::stop_measure(s).map_err(apply_error)?;
    Ok(serde_json::json!({
        "stopped": since.is_some(),
        "since": since.map(rfc3339),
        "sessions": 0,
        "live": live,
        "busy": busy,
    }))
}

/// `POST /api/stop`: Stop all agent work. The answer adds `sessions` (the
/// sessions that this stop ended), `live` (the sessions that are not reaped
/// yet) and `busy` (a Gmail call still ran). The stop is complete only when
/// `live` is 0 and `busy` is false. The wait of up to 20 seconds runs on a
/// blocking thread, so the other routes still answer.
async fn post_stop(State(s): State<Arc<Store>>, _: NoBody) -> Result<Response> {
    let done = tokio::task::spawn_blocking(move || iris_apply::stop(&s))
        .await
        .map_err(|e| Error::Store(std::io::Error::other(e.to_string()).into()))?
        .map_err(apply_error)?;
    Ok(Json(serde_json::json!({
        "stopped": true,
        "since": rfc3339(done.since),
        "sessions": done.sessions,
        "live": done.live,
        "busy": done.busy,
    }))
    .into_response())
}

/// `POST /api/resume`: end the stop, with the fields of [`stop_answer`].
/// `resumed` is false if Iris was not stopped; then nothing changed.
async fn post_resume(State(s): State<Arc<Store>>, _: NoBody) -> Result<Response> {
    let resumed = iris_apply::resume(&s).map_err(apply_error)?;
    let mut v = stop_answer(&s, None)?;
    v["resumed"] = resumed.into();
    Ok(Json(v).into_response())
}

fn apply_error(e: iris_apply::Error) -> Error {
    match e {
        iris_apply::Error::Store(e) => Error::Store(e),
        other => Error::Unavailable(other.to_string()),
    }
}

/// Cancel the send of a draft (`d-N`) in the outbox. 409 if the release
/// claimed it, 404 if the draft has no send that waits.
async fn post_outbox_cancel(
    State(s): State<Arc<Store>>,
    Path(draft): Path<String>,
    _: NoBody,
) -> Result<Response> {
    let Some((iris_store::Kind::Draft, row)) = iris_store::parse_id(&draft) else {
        return Ok(not_found());
    };
    Ok(match cancel_send(&s, row, now_ms())? {
        Cancelled::Done => Json(serde_json::json!({ "cancelled": draft })).into_response(),
        Cancelled::Started => error(
            StatusCode::CONFLICT,
            "The send started. It cannot be cancelled.",
        ),
        Cancelled::NotOpen => not_found(),
    })
}

/// The press of Send in the write view (docs/specs/2026-11-17-write.md,
/// section 2): 201 for a new write, 200 for an earlier try of the same
/// press, 409 with `flags` for warnings that the person did not see.
async fn post_write(
    State(s): State<Arc<Store>>,
    b: std::result::Result<Json<write::WriteBody>, JsonRejection>,
) -> Result<Response> {
    let b = body(b)?;
    Ok(match write::write(&s, &b, now_ms())? {
        write::Pressed::New(cs) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "write": cs })),
        )
            .into_response(),
        write::Pressed::Again(cs) => Json(serde_json::json!({ "write": cs })).into_response(),
        write::Pressed::Conflict(text, flags) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": text, "flags": flags })),
        )
            .into_response(),
    })
}

/// The state of a write (`cs-N`), section 4. 404 for a changeset that is
/// not a write.
async fn get_write(State(s): State<Arc<Store>>, Path(id): Path<String>) -> Result<Response> {
    let Some(row) = iris_changeset::parse_changeset_id(&id) else {
        return Ok(not_found());
    };
    Ok(match write::state(&s, row, now_ms())? {
        Some(v) => Json(v).into_response(),
        None => not_found(),
    })
}

/// The draft of a thread (`t-N`) that the person can use now: a reply in
/// the voice of the owner (R3-05) or a nudge (R3-07), with the address of
/// the person that owes (`to`) and its commitment (`item`). 404 when there is none, or when it is
/// stale ([`iris_voice::ready_sql`]): a newer mail, or for a nudge, a
/// change of its commitment or its person. Nothing is sent.
async fn get_draft(State(s): State<Arc<Store>>, Path(id): Path<String>) -> Result<Response> {
    let Some((iris_store::Kind::Thread, thread)) = iris_store::parse_id(&id) else {
        return Ok(not_found());
    };
    let draft = s
        .read(|c| Ok(iris_voice::ready(c, thread)?))
        .map_err(Error::from)?;
    Ok(match draft {
        Some(d) => Json(serde_json::json!({
            "thread": id,
            "based_on": iris_store::format_id(iris_store::Kind::Message, d.based_on),
            "nudge": d.to.is_some(),
            "to": d.to,
            "item": d.item.map(|i| iris_store::format_id(iris_store::Kind::Commitment, i)),
            "body": d.body,
        }))
        .into_response(),
        None => not_found(),
    })
}

/// The start values of a reply in a thread (`t-N`), section 5.
async fn get_reply(State(s): State<Arc<Store>>, Path(id): Path<String>) -> Result<Response> {
    Ok(match write::reply(&s, &id)? {
        Some(v) => Json(v).into_response(),
        None => not_found(),
    })
}

async fn get_watches(State(s): State<Arc<Store>>) -> Result<Response> {
    Ok(Json(watches(&s)?).into_response())
}

#[derive(Deserialize)]
struct WatchBody {
    text: String,
}

/// Add a watch. The answer is the new watch.
async fn post_watch(
    State(s): State<Arc<Store>>,
    b: std::result::Result<Json<WatchBody>, JsonRejection>,
) -> Result<Response> {
    let id = add_watch(&s, &body(b)?.text, now_ms())?;
    let new = watches(&s)?.into_iter().find(|w| w.id == id);
    Ok((StatusCode::CREATED, Json(new)).into_response())
}

async fn post_watch_remove(
    State(s): State<Arc<Store>>,
    Path(id): Path<String>,
    _: NoBody,
) -> Result<Response> {
    if !remove_watch(&s, &id)? {
        return Ok(not_found());
    }
    Ok(Json(serde_json::json!({ "removed": id })).into_response())
}

/// The valid proposals and the confirmed matters.
async fn get_matters(State(s): State<Arc<Store>>) -> Result<Response> {
    Ok(Json(matters(&s)?).into_response())
}

/// One matter with its members. A proposal that is not valid now is 404.
async fn get_matter(State(s): State<Arc<Store>>, Path(id): Path<String>) -> Result<Response> {
    Ok(matter(&s, &id)?.map_or_else(not_found, |d| Json(d).into_response()))
}

#[derive(Deserialize)]
struct MatterConfirm {
    title: String,
    items: Vec<String>,
}

/// The person confirms a proposal: the title and the members to keep.
async fn post_matter_confirm(
    State(s): State<Arc<Store>>,
    Path(id): Path<String>,
    b: std::result::Result<Json<MatterConfirm>, JsonRejection>,
) -> Result<Response> {
    let b = body(b)?;
    Ok(confirm_matter(&s, &id, &b.title, &b.items, now_ms())?
        .map_or_else(not_found, |d| Json(d).into_response()))
}

/// The person declines a proposal.
async fn post_matter_decline(
    State(s): State<Arc<Store>>,
    Path(id): Path<String>,
    _: NoBody,
) -> Result<Response> {
    Ok(decline_matter(&s, &id, now_ms())?.map_or_else(not_found, |d| Json(d).into_response()))
}

fn now_s() -> i64 {
    now_ms() / 1000
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
