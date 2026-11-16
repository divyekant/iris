//! A thin Gmail API v1 client for the Iris mirror (spec 1 section 4).
//!
//! The client is synchronous. The engine calls it from a blocking thread.
//! Each call takes the OAuth access token as a parameter: the client stores
//! no token. The client:
//!
//! - counts quota units for each call, and waits when the next call would go
//!   over the limit for each minute (spec 1 section 4.7);
//! - runs at most [`MAX_GETS`] `messages.get` calls at the same time;
//! - on HTTP 429, on HTTP 403 with the reason `rateLimitExceeded` or
//!   `userRateLimitExceeded`, and on HTTP 5xx of a read, waits and tries
//!   again: 1 second first, then double, up to 5 minutes, plus a random part
//!   of up to 50 percent;
//! - gives [`Error::Unauthorized`] on HTTP 401, so that the caller gets a new
//!   access token, and [`Error::HistoryExpired`] on HTTP 404 of
//!   `history.list`, the signal for a full resync (spec 1 section 4.4);
//! - streams the data of `format=raw` into a sink, and never holds the full
//!   message (spec 1 section 9).
//!
//! A list call returns one page. The caller follows `next_page_token`, so
//! that no list of all messages is in memory.

mod raw;

use std::io::Write;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

pub use raw::encode_url;

/// The API root of Gmail.
pub const API: &str = "https://gmail.googleapis.com/gmail/v1";
/// The most `messages.get` calls at the same time (spec 1 section 4.7).
pub const MAX_GETS: usize = 8;
/// The quota units for each minute for each user (spec 1, assumptions).
pub const DEFAULT_QUOTA: u32 = 6_000;
/// The page size of `messages.list` and `history.list` (spec 1 section 4.2).
pub const PAGE_SIZE: u32 = 500;
/// The largest JSON answer that the client reads into memory. A raw message
/// does not count: the client streams it.
const ANSWER_LIMIT: u64 = 16 * 1024 * 1024;

/// Quota units of each method (Gmail usage limits page).
mod cost {
    pub const PROFILE: u32 = 1;
    pub const LABELS_LIST: u32 = 1;
    pub const LABELS_GET: u32 = 1;
    pub const MESSAGES_LIST: u32 = 5;
    pub const MESSAGES_GET: u32 = 20;
    pub const MESSAGES_GET_RAW: u32 = 60;
    pub const HISTORY_LIST: u32 = 2;
    pub const MESSAGES_SEND: u32 = 100;
    pub const MESSAGES_BATCH_MODIFY: u32 = 50;
    pub const MESSAGES_TRASH: u32 = 5;
    pub const MESSAGES_UNTRASH: u32 = 5;
    pub const LABELS_CREATE: u32 = 5;
}

/// The most message IDs in one `messages.batchModify` call (Gmail limit).
pub const BATCH_LIMIT: usize = 1000;

/// The settings of a client.
#[derive(Clone, Debug)]
pub struct Config {
    /// The API root. Tests set the URL of the fake server.
    pub base_url: String,
    /// Quota units for each minute. A setting, so that a change at Google
    /// needs no new release.
    pub quota_per_minute: u32,
    /// The first wait after a refused call.
    pub backoff_start: Duration,
    /// The longest wait after a refused call.
    pub backoff_max: Duration,
    /// The longest time for one call, the body included.
    pub timeout: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            base_url: API.to_string(),
            quota_per_minute: DEFAULT_QUOTA,
            backoff_start: Duration::from_secs(1),
            backoff_max: Duration::from_secs(300),
            timeout: Duration::from_secs(300),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    /// HTTP 401. Get a new access token one time, then call again.
    Unauthorized,
    /// HTTP 404 for a message or a label: Gmail does not have it.
    NotFound,
    /// HTTP 404 of `history.list`: the history ID is too old. Do a full resync.
    HistoryExpired,
    /// Another HTTP error. `reason` is the first reason of the Gmail error.
    Api {
        status: u16,
        reason: String,
        message: String,
    },
    /// The network failed, or the answer stopped before its end.
    Transport(String),
    /// The answer is not in the shape of the Gmail API.
    Decode(String),
    /// The sink of a raw message failed.
    Sink(std::io::Error),
}

impl Error {
    /// For a write call (`messages.send`, `batchModify`, `trash`,
    /// `labels.create`): true if
    /// Gmail can have done the call (spec 2 section 7.2). A timeout, a lost
    /// connection and HTTP 5xx give an unknown result.
    pub fn write_result_unknown(&self) -> bool {
        match self {
            Error::Transport(_) => true,
            Error::Api { status, .. } => *status >= 500,
            _ => false,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unauthorized => write!(f, "Gmail refused the access token (HTTP 401)"),
            Error::NotFound => write!(f, "Gmail does not have the item (HTTP 404)"),
            Error::HistoryExpired => write!(f, "the history ID is too old (HTTP 404)"),
            Error::Api {
                status,
                reason,
                message,
            } => write!(f, "Gmail error {status} {reason}: {message}"),
            Error::Transport(e) => write!(f, "network error: {e}"),
            Error::Decode(e) => write!(f, "bad answer from Gmail: {e}"),
            Error::Sink(e) => write!(f, "cannot write the raw message: {e}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Gmail sends 64-bit numbers as strings.
fn number<'de, D: Deserializer<'de>, T: std::str::FromStr>(
    d: D,
) -> std::result::Result<T, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum N {
        S(String),
        U(u64),
    }
    let text = match N::deserialize(d)? {
        N::S(s) => s,
        N::U(n) => n.to_string(),
    };
    text.parse()
        .map_err(|_| serde::de::Error::custom("not a number"))
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub email_address: String,
    #[serde(default)]
    pub messages_total: u64,
    #[serde(default)]
    pub threads_total: u64,
    #[serde(deserialize_with = "number")]
    pub history_id: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub id: String,
    pub name: String,
    /// `system` or `user`.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// The counts come only from `labels.get`.
    pub messages_total: Option<u64>,
    pub messages_unread: Option<u64>,
    pub threads_total: Option<u64>,
    pub threads_unread: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageRef {
    pub id: String,
    pub thread_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    #[serde(default)]
    pub messages: Vec<MessageRef>,
    pub next_page_token: Option<String>,
}

/// The answer of `format=minimal`: only the ID and the labels.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Minimal {
    pub id: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// The answer of `format=metadata`: the ID, the labels and the headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub id: String,
    pub label_ids: Vec<String>,
    pub headers: Vec<Header>,
}

/// The fields of `format=raw`. The data went to the sink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawMessage {
    pub id: String,
    pub thread_id: String,
    pub label_ids: Vec<String>,
    pub history_id: u64,
    /// Milliseconds since 1970-01-01T00:00:00Z.
    pub internal_date: i64,
    /// The count of decoded bytes that went to the sink.
    pub size: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFields {
    id: String,
    thread_id: String,
    #[serde(default)]
    label_ids: Vec<String>,
    #[serde(deserialize_with = "number")]
    history_id: u64,
    #[serde(deserialize_with = "number")]
    internal_date: i64,
}

/// A message in a history record.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMessage {
    pub id: String,
    pub thread_id: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
}

/// The history types of `history.list`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryType {
    MessageAdded,
    MessageDeleted,
    LabelAdded,
    LabelRemoved,
}

impl HistoryType {
    /// The four types of an incremental sync (spec 1 section 4.3).
    pub const ALL: [HistoryType; 4] = [
        HistoryType::MessageAdded,
        HistoryType::MessageDeleted,
        HistoryType::LabelAdded,
        HistoryType::LabelRemoved,
    ];

    fn name(self) -> &'static str {
        match self {
            HistoryType::MessageAdded => "messageAdded",
            HistoryType::MessageDeleted => "messageDeleted",
            HistoryType::LabelAdded => "labelAdded",
            HistoryType::LabelRemoved => "labelRemoved",
        }
    }
}

/// One change in a history record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Added(HistoryMessage),
    Deleted(HistoryMessage),
    LabelsAdded(HistoryMessage, Vec<String>),
    LabelsRemoved(HistoryMessage, Vec<String>),
}

/// One history record. Apply the records in the order of the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    pub id: u64,
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryPage {
    pub history: Vec<History>,
    /// The current history ID of the mailbox.
    pub history_id: u64,
    pub next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct Wrapped {
    message: HistoryMessage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WrappedLabels {
    message: HistoryMessage,
    #[serde(default)]
    label_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryJson {
    #[serde(deserialize_with = "number")]
    id: u64,
    #[serde(default)]
    messages_added: Vec<Wrapped>,
    #[serde(default)]
    messages_deleted: Vec<Wrapped>,
    #[serde(default)]
    labels_added: Vec<WrappedLabels>,
    #[serde(default)]
    labels_removed: Vec<WrappedLabels>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryPageJson {
    #[serde(default)]
    history: Vec<HistoryJson>,
    #[serde(deserialize_with = "number")]
    history_id: u64,
    next_page_token: Option<String>,
}

/// The answer of `messages.send`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    pub id: String,
    pub thread_id: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
}

/// A token bucket of quota units, filled at the limit for each minute.
struct Quota {
    units: f64,
    at: Instant,
}

/// A count of the `messages.get` calls that run now.
struct Gate {
    running: Mutex<usize>,
    free: Condvar,
}

struct GateGuard<'a>(&'a Gate);

impl Gate {
    fn enter(&self) -> GateGuard<'_> {
        let mut n = self.running.lock().unwrap();
        while *n >= MAX_GETS {
            n = self.free.wait(n).unwrap();
        }
        *n += 1;
        GateGuard(self)
    }
}

impl Drop for GateGuard<'_> {
    fn drop(&mut self) {
        *self.0.running.lock().unwrap() -= 1;
        self.0.free.notify_one();
    }
}

enum Method<'a> {
    Get,
    Post(&'a [u8]),
}

/// A Gmail API client. Share one client between threads.
pub struct Gmail {
    agent: ureq::Agent,
    config: Config,
    quota: Mutex<Quota>,
    gets: Gate,
}

impl Gmail {
    pub fn new(config: Config) -> Gmail {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(config.timeout))
            .build()
            .into();
        Gmail {
            agent,
            quota: Mutex::new(Quota {
                units: config.quota_per_minute as f64,
                at: Instant::now(),
            }),
            config,
            gets: Gate {
                running: Mutex::new(0),
                free: Condvar::new(),
            },
        }
    }

    /// `users.getProfile`.
    pub fn profile(&self, token: &str) -> Result<Profile> {
        self.json(token, "profile", &[], cost::PROFILE)
    }

    /// `users.labels.list`.
    pub fn labels(&self, token: &str) -> Result<Vec<Label>> {
        #[derive(Deserialize)]
        struct L {
            #[serde(default)]
            labels: Vec<Label>,
        }
        let l: L = self.json(token, "labels", &[], cost::LABELS_LIST)?;
        Ok(l.labels)
    }

    /// `users.labels.get`, with the counts of the label.
    pub fn label(&self, token: &str, id: &str) -> Result<Label> {
        self.json(
            token,
            &format!("labels/{}", segment(id)),
            &[],
            cost::LABELS_GET,
        )
    }

    /// One page of `users.messages.list`, newest first. A message must have
    /// each label of `label_ids`. Spec 1 always sets `include_spam_trash`.
    pub fn messages(
        &self,
        token: &str,
        label_ids: &[&str],
        include_spam_trash: bool,
        page_token: Option<&str>,
    ) -> Result<MessagePage> {
        let mut q = vec![("maxResults", PAGE_SIZE.to_string())];
        q.extend(label_ids.iter().map(|l| ("labelIds", l.to_string())));
        if include_spam_trash {
            q.push(("includeSpamTrash", "true".into()));
        }
        if let Some(t) = page_token {
            q.push(("pageToken", t.into()));
        }
        self.json(token, "messages", &q, cost::MESSAGES_LIST)
    }

    /// `users.messages.get` with `format=minimal`.
    pub fn minimal(&self, token: &str, id: &str) -> Result<Minimal> {
        let _gate = self.gets.enter();
        self.json(
            token,
            &format!("messages/{}", segment(id)),
            &[("format", "minimal".into())],
            cost::MESSAGES_GET,
        )
    }

    /// `users.messages.get` with `format=metadata`. An empty `headers` gives
    /// each header.
    pub fn metadata(&self, token: &str, id: &str, headers: &[&str]) -> Result<Metadata> {
        #[derive(Deserialize)]
        struct Payload {
            #[serde(default)]
            headers: Vec<Header>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct M {
            id: String,
            #[serde(default)]
            label_ids: Vec<String>,
            payload: Option<Payload>,
        }
        let mut q = vec![("format", "metadata".to_string())];
        q.extend(headers.iter().map(|h| ("metadataHeaders", h.to_string())));
        let _gate = self.gets.enter();
        let m: M = self.json(
            token,
            &format!("messages/{}", segment(id)),
            &q,
            cost::MESSAGES_GET,
        )?;
        Ok(Metadata {
            id: m.id,
            label_ids: m.label_ids,
            headers: m.payload.map(|p| p.headers).unwrap_or_default(),
        })
    }

    /// `users.messages.get` with `format=raw`. The decoded message goes to
    /// `sink` through a buffer of 64 KiB. If the call fails after the first
    /// byte, the sink has a part of the message: discard it.
    pub fn raw<W: Write>(&self, token: &str, id: &str, sink: &mut W) -> Result<RawMessage> {
        let _gate = self.gets.enter();
        let response = self.call(
            token,
            Method::Get,
            &format!("messages/{}", segment(id)),
            &[("format", "raw".into())],
            cost::MESSAGES_GET_RAW,
        )?;
        let (rest, size) =
            raw::split(response.into_body().into_reader(), sink).map_err(|e| match e {
                raw::SplitError::Read(e) => Error::Transport(e.to_string()),
                raw::SplitError::Write(e) => Error::Sink(e),
                raw::SplitError::Format(e) => Error::Decode(e.to_string()),
            })?;
        let f: RawFields = parse(&rest)?;
        Ok(RawMessage {
            id: f.id,
            thread_id: f.thread_id,
            label_ids: f.label_ids,
            history_id: f.history_id,
            internal_date: f.internal_date,
            size,
        })
    }

    /// One page of `users.history.list` after `start`. An empty `types`
    /// gives each type. [`Error::HistoryExpired`] means: do a full resync.
    pub fn history(
        &self,
        token: &str,
        start: u64,
        types: &[HistoryType],
        page_token: Option<&str>,
    ) -> Result<HistoryPage> {
        let mut q = vec![
            ("startHistoryId", start.to_string()),
            ("maxResults", PAGE_SIZE.to_string()),
        ];
        q.extend(types.iter().map(|t| ("historyTypes", t.name().to_string())));
        if let Some(t) = page_token {
            q.push(("pageToken", t.into()));
        }
        let page: HistoryPageJson = match self.json(token, "history", &q, cost::HISTORY_LIST) {
            Err(Error::NotFound) => return Err(Error::HistoryExpired),
            other => other?,
        };
        let history = page
            .history
            .into_iter()
            .map(|h| {
                let mut changes: Vec<Change> = Vec::new();
                changes.extend(
                    h.messages_added
                        .into_iter()
                        .map(|w| Change::Added(w.message)),
                );
                changes.extend(
                    h.messages_deleted
                        .into_iter()
                        .map(|w| Change::Deleted(w.message)),
                );
                changes.extend(
                    h.labels_added
                        .into_iter()
                        .map(|w| Change::LabelsAdded(w.message, w.label_ids)),
                );
                changes.extend(
                    h.labels_removed
                        .into_iter()
                        .map(|w| Change::LabelsRemoved(w.message, w.label_ids)),
                );
                History { id: h.id, changes }
            })
            .collect();
        Ok(HistoryPage {
            history,
            history_id: page.history_id,
            next_page_token: page.next_page_token,
        })
    }

    /// `users.messages.send` with a raw RFC 5322 message. A write call: the
    /// client tries again only on a rate limit, never on HTTP 5xx or a
    /// network error. See [`Error::write_result_unknown`].
    pub fn send(&self, token: &str, raw: &[u8], thread_id: Option<&str>) -> Result<Sent> {
        let mut body = serde_json::json!({ "raw": encode_url(raw) });
        if let Some(t) = thread_id {
            body["threadId"] = t.into();
        }
        let body = body.to_string();
        let response = self.call(
            token,
            Method::Post(body.as_bytes()),
            "messages/send",
            &[],
            cost::MESSAGES_SEND,
        )?;
        // Gmail accepted the send, so a bad answer leaves the result unknown.
        read_all(response)
            .and_then(|b| parse(&b))
            .map_err(|e| Error::Transport(format!("answer of an accepted send: {e}")))
    }

    /// `users.messages.batchModify`: add and remove labels of at most
    /// [`BATCH_LIMIT`] messages. A write call, as [`Gmail::send`]: see
    /// [`Error::write_result_unknown`]. A list that is too long gives
    /// HTTP 400 at Gmail, and the client sends no call.
    pub fn batch_modify(
        &self,
        token: &str,
        ids: &[&str],
        add: &[&str],
        remove: &[&str],
    ) -> Result<()> {
        if ids.len() > BATCH_LIMIT {
            return Err(Error::Api {
                status: 400,
                reason: "invalidArgument".into(),
                message: format!("more than {BATCH_LIMIT} message IDs"),
            });
        }
        let body = serde_json::json!({ "ids": ids, "addLabelIds": add, "removeLabelIds": remove })
            .to_string();
        self.call(
            token,
            Method::Post(body.as_bytes()),
            "messages/batchModify",
            &[],
            cost::MESSAGES_BATCH_MODIFY,
        )?;
        Ok(())
    }

    /// `users.messages.trash`. Gives the labels of the message after the
    /// call. A write call, as [`Gmail::send`].
    pub fn trash(&self, token: &str, id: &str) -> Result<Minimal> {
        let response = self.call(
            token,
            Method::Post(b"{}"),
            &format!("messages/{}/trash", segment(id)),
            &[],
            cost::MESSAGES_TRASH,
        )?;
        // Gmail accepted the call, so a bad answer leaves the result unknown.
        read_all(response)
            .and_then(|b| parse(&b))
            .map_err(|e| Error::Transport(format!("answer of an accepted trash: {e}")))
    }

    /// `users.messages.untrash`. Gives the labels of the message after the
    /// call. A write call, as [`Gmail::send`].
    pub fn untrash(&self, token: &str, id: &str) -> Result<Minimal> {
        let response = self.call(
            token,
            Method::Post(b"{}"),
            &format!("messages/{}/untrash", segment(id)),
            &[],
            cost::MESSAGES_UNTRASH,
        )?;
        // Gmail accepted the call, so a bad answer leaves the result unknown.
        read_all(response)
            .and_then(|b| parse(&b))
            .map_err(|e| Error::Transport(format!("answer of an accepted untrash: {e}")))
    }

    /// `users.labels.create` of a user label with the name `name`. A write
    /// call, as [`Gmail::send`]. Gmail gives HTTP 409 if a label with the
    /// name exists.
    pub fn create_label(&self, token: &str, name: &str) -> Result<Label> {
        let body = serde_json::json!({ "name": name }).to_string();
        let response = self.call(
            token,
            Method::Post(body.as_bytes()),
            "labels",
            &[],
            cost::LABELS_CREATE,
        )?;
        // Gmail accepted the call, so a bad answer leaves the result unknown.
        read_all(response)
            .and_then(|b| parse(&b))
            .map_err(|e| Error::Transport(format!("answer of an accepted label create: {e}")))
    }

    fn json<T: DeserializeOwned>(
        &self,
        token: &str,
        path: &str,
        query: &[(&str, String)],
        units: u32,
    ) -> Result<T> {
        let response = self.call(token, Method::Get, path, query, units)?;
        parse(&read_all(response)?)
    }

    /// Make one call, with the quota wait and the backoff. Returns only a
    /// 2xx answer.
    fn call(
        &self,
        token: &str,
        method: Method<'_>,
        path: &str,
        query: &[(&str, String)],
        units: u32,
    ) -> Result<ureq::http::Response<ureq::Body>> {
        let url = format!("{}/users/me/{path}", self.config.base_url);
        let auth = format!("Bearer {token}");
        let is_read = matches!(method, Method::Get);
        let mut wait = self.config.backoff_start;
        loop {
            self.take_quota(units);
            let sent = match method {
                Method::Get => {
                    let mut r = self.agent.get(&url).header("Authorization", &auth);
                    for (k, v) in query {
                        r = r.query(*k, v);
                    }
                    r.call()
                }
                Method::Post(body) => {
                    let mut r = self
                        .agent
                        .post(&url)
                        .header("Authorization", &auth)
                        .content_type("application/json");
                    for (k, v) in query {
                        r = r.query(*k, v);
                    }
                    r.send(body)
                }
            };
            let response = sent.map_err(|e| Error::Transport(e.to_string()))?;
            let status = response.status().as_u16();
            if (200..300).contains(&status) {
                return Ok(response);
            }
            let (reason, message) = error_of(response);
            let rate_limit = status == 429
                || (status == 403
                    && (reason == "rateLimitExceeded" || reason == "userRateLimitExceeded"));
            if rate_limit || (is_read && status >= 500) {
                std::thread::sleep(jitter(wait));
                wait = (wait * 2).min(self.config.backoff_max);
                continue;
            }
            return Err(match status {
                401 => Error::Unauthorized,
                404 => Error::NotFound,
                _ => Error::Api {
                    status,
                    reason,
                    message,
                },
            });
        }
    }

    /// Wait for the quota, then take `units`.
    fn take_quota(&self, units: u32) {
        let limit = self.config.quota_per_minute.max(1) as f64;
        while let Some(wait) = reserve(
            &mut self.quota.lock().unwrap(),
            Instant::now(),
            limit,
            units as f64,
        ) {
            std::thread::sleep(wait);
        }
    }
}

/// Fill the bucket up to `now`. If it has `units`, or is full when `units`
/// is above the limit, take the full `units` and return `None`. The bucket
/// can go below zero, so that a call above the limit pays its full cost.
/// Else return the wait.
fn reserve(q: &mut Quota, now: Instant, limit: f64, units: f64) -> Option<Duration> {
    q.units =
        (q.units + now.saturating_duration_since(q.at).as_secs_f64() * limit / 60.0).min(limit);
    q.at = now;
    let need = units.min(limit);
    if q.units >= need {
        q.units -= units;
        return None;
    }
    Some(Duration::from_secs_f64((need - q.units) * 60.0 / limit))
}

/// Add a random part of up to 50 percent to a wait.
fn jitter(wait: Duration) -> Duration {
    use std::hash::{BuildHasher, Hasher};
    let r = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    wait + wait.mul_f64((r % 1000) as f64 / 2000.0)
}

/// Escape an ID for one segment of the URL path.
fn segment(id: &str) -> String {
    id.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn read_all(response: ureq::http::Response<ureq::Body>) -> Result<Vec<u8>> {
    response
        .into_body()
        .with_config()
        .limit(ANSWER_LIMIT)
        .read_to_vec()
        .map_err(|e| Error::Transport(e.to_string()))
}

fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|e| Error::Decode(e.to_string()))
}

/// The first reason and the message of a Gmail error answer.
fn error_of(response: ureq::http::Response<ureq::Body>) -> (String, String) {
    let body = response
        .into_body()
        .with_config()
        .limit(64 * 1024)
        .read_to_vec()
        .unwrap_or_default();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let e = &v["error"];
    let reason = e["errors"][0]["reason"].as_str().unwrap_or("").to_string();
    let message = e["message"].as_str().unwrap_or("").to_string();
    (reason, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_lets_at_most_max_gets_run() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let gate = Gate {
            running: Mutex::new(0),
            free: Condvar::new(),
        };
        let (now, most) = (AtomicUsize::new(0), AtomicUsize::new(0));
        std::thread::scope(|s| {
            for _ in 0..3 * MAX_GETS {
                s.spawn(|| {
                    let _g = gate.enter();
                    let n = now.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(n, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(5));
                    now.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        assert_eq!(most.load(Ordering::SeqCst), MAX_GETS);
    }

    #[test]
    fn a_call_above_the_limit_pays_its_full_cost() {
        let t0 = Instant::now();
        let mut q = Quota {
            units: 20.0,
            at: t0,
        };
        // Limit 20 for each minute; a raw get costs 60.
        assert_eq!(reserve(&mut q, t0, 20.0, 60.0), None);
        let wait = reserve(&mut q, t0, 20.0, 60.0).unwrap();
        assert_eq!(wait, Duration::from_secs(180));
        assert_eq!(reserve(&mut q, t0 + wait, 20.0, 60.0), None);
    }

    #[test]
    fn segments_escape_path_characters() {
        assert_eq!(segment("Label_1"), "Label_1");
        assert_eq!(segment("a/b?c"), "a%2Fb%3Fc");
    }

    #[test]
    fn jitter_adds_at_most_half() {
        for _ in 0..100 {
            let j = jitter(Duration::from_secs(2));
            assert!(j >= Duration::from_secs(2) && j < Duration::from_secs(3));
        }
    }

    #[test]
    fn only_unknown_results_of_a_write_say_so() {
        assert!(Error::Transport("reset".into()).write_result_unknown());
        let api = |status| Error::Api {
            status,
            reason: String::new(),
            message: String::new(),
        };
        assert!(api(503).write_result_unknown());
        assert!(!api(400).write_result_unknown());
        assert!(!Error::Unauthorized.write_result_unknown());
    }
}
