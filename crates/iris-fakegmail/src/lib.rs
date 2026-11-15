//! A fake Gmail API v1 server for tests, on top of the synthetic mailbox.
//!
//! `FakeGmail::start(mailbox)` serves on `127.0.0.1` at a random port, in a
//! background thread with its own runtime. It needs no network and no
//! credentials. It accepts any `Authorization: Bearer` token.
//!
//! Supported: `users.getProfile`, `users.labels.list`, `users.messages.list`
//! (paging, no `q`), `users.messages.get` (`raw`, `metadata`, `minimal`),
//! `users.messages.send` and `users.history.list`. The JSON has the shape of
//! the real API. The fake returns 400 for `format=full` and for an absent
//! format, because the real default is `full` and the fake does not build it.

// Handlers return a full `Response` as the error. The size does not matter in a test server.
#![allow(clippy::result_large_err)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use iris_synth::{base64, Mailbox, Sender, OWNER};
use serde::Deserialize;
use serde_json::{json, Value};

/// History ids start above this value, so they do not look like indexes.
const HISTORY_BASE: u64 = 100_000;

struct Store {
    mailbox: Mailbox,
    /// Messages from `messages.send`: (raw bytes, thread id).
    sent: Vec<(Vec<u8>, String)>,
    /// A `startHistoryId` below this value gets 404, as an expired id does.
    history_floor: u64,
    /// Each thread id, of the mailbox and of the sent messages.
    threads: std::collections::BTreeSet<String>,
}

impl Store {
    fn total(&self) -> usize {
        self.mailbox.len() + self.sent.len()
    }

    /// Message `i` has history id `HISTORY_BASE + i + 1`.
    fn latest_history(&self) -> u64 {
        HISTORY_BASE + self.total() as u64
    }

    fn raw(&self, i: usize) -> Vec<u8> {
        match i.checked_sub(self.mailbox.len()) {
            None => self.mailbox.raw(i),
            Some(k) => self.sent[k].0.clone(),
        }
    }

    fn thread_id(&self, i: usize) -> String {
        match i.checked_sub(self.mailbox.len()) {
            None => gmail_id(self.mailbox.truth(i).thread as u64),
            Some(k) => self.sent[k].1.clone(),
        }
    }

    fn labels(&self, i: usize) -> Vec<&'static str> {
        if i >= self.mailbox.len() {
            return vec!["SENT"];
        }
        let t = self.mailbox.truth(i);
        if t.sender == Sender::Owner {
            return vec!["SENT"];
        }
        let category = match t.form {
            iris_synth::Form::Feed => "CATEGORY_PROMOTIONS",
            iris_synth::Form::Record | iris_synth::Form::Signal => "CATEGORY_UPDATES",
            _ => "CATEGORY_PERSONAL",
        };
        let mut labels = vec!["INBOX", category];
        // The newest 1 % of the mailbox is unread.
        if i * 100 >= self.mailbox.len() * 99 {
            labels.push("UNREAD");
        }
        labels
    }

    fn index(&self, id: &str) -> Option<usize> {
        let i = u64::from_str_radix(id, 16).ok()?.checked_sub(1)? as usize;
        (i < self.total()).then_some(i)
    }

    fn summary(&self, i: usize) -> Value {
        json!({ "id": gmail_id(i as u64), "threadId": self.thread_id(i) })
    }
}

/// Gmail ids are lowercase hex strings. Message `i` has id `i + 1`.
fn gmail_id(n: u64) -> String {
    format!("{:016x}", n + 1)
}

/// A running fake server. The server stops when the process ends.
#[derive(Clone)]
pub struct FakeGmail {
    addr: SocketAddr,
    store: Arc<Mutex<Store>>,
}

impl FakeGmail {
    /// Start the server on a random port of `127.0.0.1`.
    pub fn start(mailbox: Mailbox) -> std::io::Result<FakeGmail> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        // The profile id of an empty mailbox is HISTORY_BASE, so it must be valid.
        let floor = HISTORY_BASE;
        let threads = (0..mailbox.len())
            .map(|i| gmail_id(mailbox.truth(i).thread as u64))
            .collect();
        let store = Arc::new(Mutex::new(Store {
            mailbox,
            sent: Vec::new(),
            history_floor: floor,
            threads,
        }));
        let app = router(store.clone());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()?;
        // shortcut: the thread runs until the process ends; add a shutdown if a test needs one.
        std::thread::spawn(move || {
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
                axum::serve(listener, app).await.expect("fake Gmail server");
            })
        });
        Ok(FakeGmail { addr, store })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The API root, for example `http://127.0.0.1:41234/gmail/v1`.
    pub fn base_url(&self) -> String {
        format!("http://{}/gmail/v1", self.addr)
    }

    /// Make each history id below `history_id` expired (404), as Gmail does
    /// after some days.
    pub fn expire_history_before(&self, history_id: u64) {
        self.store.lock().unwrap().history_floor = history_id;
    }
}

type Shared = Arc<Mutex<Store>>;

fn router(store: Shared) -> Router {
    Router::new()
        .route("/gmail/v1/users/{user}/profile", get(profile))
        .route("/gmail/v1/users/{user}/labels", get(labels))
        .route("/gmail/v1/users/{user}/messages", get(list))
        .route("/gmail/v1/users/{user}/messages/send", post(send))
        .route("/gmail/v1/users/{user}/messages/{id}", get(message))
        .route("/gmail/v1/users/{user}/history", get(history))
        .with_state(store)
}

fn error(code: StatusCode, message: &str) -> Response {
    let status = match code {
        StatusCode::BAD_REQUEST => "INVALID_ARGUMENT",
        StatusCode::UNAUTHORIZED => "UNAUTHENTICATED",
        _ => "NOT_FOUND",
    };
    let reason = match code {
        StatusCode::BAD_REQUEST => "badRequest",
        StatusCode::UNAUTHORIZED => "authError",
        _ => "notFound",
    };
    let body = json!({ "error": {
        "code": code.as_u16(),
        "message": message,
        "errors": [{ "message": message, "domain": "global", "reason": reason }],
        "status": status,
    }});
    (code, Json(body)).into_response()
}

/// Check the token and the user. The fake accepts any bearer token.
fn check(headers: &HeaderMap, user: &str) -> Result<(), Response> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if auth
        .strip_prefix("Bearer ")
        .is_none_or(|t| t.trim().is_empty())
    {
        return Err(error(
            StatusCode::UNAUTHORIZED,
            "Request is missing required authentication credential.",
        ));
    }
    if user != "me" && user != OWNER {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Requested entity was not found.",
        ));
    }
    Ok(())
}

/// Read `maxResults` (default 100, at most 500) and `pageToken` (an offset).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    max_results: Option<usize>,
    page_token: Option<String>,
    start_history_id: Option<String>,
    /// Not supported. The fake refuses these, so a test does not get an
    /// unfiltered list by mistake.
    q: Option<String>,
    label_ids: Option<String>,
}

impl Page {
    fn bounds(&self) -> Result<(usize, usize), Response> {
        let size = self.max_results.unwrap_or(100).clamp(1, 500);
        let start = match &self.page_token {
            None => 0,
            Some(t) => t
                .parse()
                .map_err(|_| error(StatusCode::BAD_REQUEST, "Invalid pageToken"))?,
        };
        Ok((start, size))
    }
}

async fn profile(State(s): State<Shared>, Path(user): Path<String>, h: HeaderMap) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let s = s.lock().unwrap();
    Json(json!({
        "emailAddress": OWNER,
        "messagesTotal": s.total(),
        "threadsTotal": s.threads.len(),
        "historyId": s.latest_history().to_string(),
    }))
    .into_response()
}

async fn labels(Path(user): Path<String>, h: HeaderMap) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let names = [
        "INBOX",
        "SENT",
        "UNREAD",
        "STARRED",
        "IMPORTANT",
        "DRAFT",
        "SPAM",
        "TRASH",
        "CATEGORY_PERSONAL",
        "CATEGORY_SOCIAL",
        "CATEGORY_PROMOTIONS",
        "CATEGORY_UPDATES",
        "CATEGORY_FORUMS",
    ];
    let labels: Vec<Value> = names
        .iter()
        .map(|n| json!({ "id": n, "name": n, "type": "system" }))
        .collect();
    Json(json!({ "labels": labels })).into_response()
}

async fn list(
    State(s): State<Shared>,
    Path(user): Path<String>,
    Query(page): Query<Page>,
    h: HeaderMap,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    if page.q.is_some() || page.label_ids.is_some() {
        return error(
            StatusCode::BAD_REQUEST,
            "The fake does not support q or labelIds.",
        );
    }
    let (start, size) = match page.bounds() {
        Ok(b) => b,
        Err(e) => return e,
    };
    let s = s.lock().unwrap();
    let total = s.total();
    // Newest first, as Gmail lists.
    let messages: Vec<Value> = (0..total)
        .rev()
        .skip(start)
        .take(size)
        .map(|i| s.summary(i))
        .collect();
    let mut body = json!({ "resultSizeEstimate": total });
    if !messages.is_empty() {
        body["messages"] = Value::Array(messages);
    }
    if start.saturating_add(size) < total {
        body["nextPageToken"] = Value::String((start + size).to_string());
    }
    Json(body).into_response()
}

/// Unfold the header block of a message into (name, value) pairs.
fn headers_of(raw: &[u8]) -> Vec<(String, String)> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..end]);
    let mut out: Vec<(String, String)> = Vec::new();
    for line in head.split("\r\n") {
        if line.starts_with([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                last.1.push_str(line);
            }
        } else if let Some((name, value)) = line.split_once(':') {
            out.push((name.to_string(), value.trim().to_string()));
        }
    }
    out
}

/// shortcut: the snippet is the start of the body bytes, with MIME parts not
/// decoded; decode the text part when a test needs a real snippet.
fn snippet(raw: &[u8]) -> String {
    let start = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map_or(raw.len(), |p| p + 4);
    let text = String::from_utf8_lossy(&raw[start..raw.len().min(start + 400)]);
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(100)
        .collect()
}

async fn message(
    State(s): State<Shared>,
    Path((user, id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    h: HeaderMap,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    // `metadataHeaders` repeats, so read the raw query.
    let mut format = None;
    let mut wanted: Vec<String> = Vec::new();
    for pair in query.as_deref().unwrap_or("").split('&') {
        match pair.split_once('=') {
            Some(("format", v)) => format = Some(v.to_string()),
            Some(("metadataHeaders", v)) => wanted.push(v.to_ascii_lowercase()),
            _ => {}
        }
    }
    let s = s.lock().unwrap();
    let Some(i) = s.index(&id) else {
        return error(StatusCode::NOT_FOUND, "Requested entity was not found.");
    };
    let raw = s.raw(i);
    let date_ms = match i < s.mailbox.len() {
        true => s.mailbox.truth(i).date * 1000,
        false => 1_767_225_600_000 + (i - s.mailbox.len()) as i64,
    };
    let mut body = json!({
        "id": gmail_id(i as u64),
        "threadId": s.thread_id(i),
        "labelIds": s.labels(i),
        "snippet": snippet(&raw),
        "sizeEstimate": raw.len(),
        "historyId": (HISTORY_BASE + i as u64 + 1).to_string(),
        "internalDate": date_ms.to_string(),
    });
    match format.as_deref() {
        Some("minimal") => {}
        Some("raw") => body["raw"] = Value::String(base64::encode_url(&raw)),
        Some("metadata") => {
            let headers = headers_of(&raw);
            let mime = headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
                .map(|(_, v)| v.split(';').next().unwrap_or("").trim().to_string())
                .unwrap_or_else(|| "text/plain".to_string());
            let headers: Vec<Value> = headers
                .into_iter()
                .filter(|(n, _)| wanted.is_empty() || wanted.contains(&n.to_ascii_lowercase()))
                .map(|(n, v)| json!({ "name": n, "value": v }))
                .collect();
            body["payload"] =
                json!({ "partId": "", "mimeType": mime, "filename": "", "headers": headers });
        }
        _ => {
            return error(
                StatusCode::BAD_REQUEST,
                "The fake supports only format=raw, metadata or minimal.",
            )
        }
    }
    Json(body).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendBody {
    raw: String,
    thread_id: Option<String>,
}

async fn send(
    State(s): State<Shared>,
    Path(user): Path<String>,
    h: HeaderMap,
    body: Result<Json<SendBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let Ok(Json(body)) = body else {
        return error(StatusCode::BAD_REQUEST, "Invalid JSON payload received.");
    };
    let Some(raw) = base64::decode(&body.raw).filter(|r| !r.is_empty()) else {
        return error(StatusCode::BAD_REQUEST, "Invalid value for ByteString: raw");
    };
    let mut s = s.lock().unwrap();
    let i = s.total();
    let thread = body
        .thread_id
        .unwrap_or_else(|| format!("{:016x}", 0x8000_0000u64 + i as u64));
    s.threads.insert(thread.clone());
    s.sent.push((raw, thread));
    Json(json!({ "id": gmail_id(i as u64), "threadId": s.thread_id(i), "labelIds": ["SENT"] }))
        .into_response()
}

async fn history(
    State(s): State<Shared>,
    Path(user): Path<String>,
    Query(page): Query<Page>,
    h: HeaderMap,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let (offset, size) = match page.bounds() {
        Ok(b) => b,
        Err(e) => return e,
    };
    let Some(start) = page.start_history_id.and_then(|v| v.parse::<u64>().ok()) else {
        return error(StatusCode::BAD_REQUEST, "Missing or invalid startHistoryId");
    };
    let s = s.lock().unwrap();
    if start < s.history_floor {
        return error(StatusCode::NOT_FOUND, "Requested entity was not found.");
    }
    let latest = s.latest_history();
    // Each message makes one history record: messagesAdded, oldest first.
    let first = start.saturating_sub(HISTORY_BASE) as usize;
    let records: Vec<Value> = (first..s.total())
        .skip(offset)
        .take(size)
        .map(|i| {
            let mut m = s.summary(i);
            m["labelIds"] = json!(s.labels(i));
            json!({
                "id": (HISTORY_BASE + i as u64 + 1).to_string(),
                "messages": [s.summary(i)],
                "messagesAdded": [{ "message": m }],
            })
        })
        .collect();
    let mut body = json!({ "historyId": latest.to_string() });
    if first.saturating_add(offset).saturating_add(size) < s.total() {
        body["nextPageToken"] = Value::String((offset + size).to_string());
    }
    if !records.is_empty() {
        body["history"] = Value::Array(records);
    }
    Json(body).into_response()
}
