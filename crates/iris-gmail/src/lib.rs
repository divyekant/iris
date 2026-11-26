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

pub use iris_provider::{
    Change, Draft, DraftRaw, Error, Header, History, HistoryMessage, HistoryPage, Label,
    MessagePage, MessageRef, Metadata, Minimal, Profile, Provider, RawMessage, Result, Sent,
    BATCH_LIMIT, DRAFT_LIMIT, MAX_GETS,
};
pub use raw::encode_url;

/// The API root of Gmail.
pub const API: &str = "https://gmail.googleapis.com/gmail/v1";
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
    pub const DRAFTS_CREATE: u32 = 10;
    pub const DRAFTS_GET: u32 = 5;
    pub const DRAFTS_LIST: u32 = 5;
    pub const DRAFTS_SEND: u32 = 100;
    pub const DRAFTS_UPDATE: u32 = 15;
}

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileJson {
    email_address: String,
    #[serde(default)]
    messages_total: u64,
    #[serde(default)]
    threads_total: u64,
    #[serde(deserialize_with = "number")]
    history_id: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFields {
    id: String,
    thread_id: String,
    #[serde(default)]
    label_ids: Vec<String>,
    #[serde(deserialize_with = "number")]
    internal_date: i64,
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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct DraftPage {
    #[serde(default)]
    drafts: Vec<Draft>,
    next_page_token: Option<String>,
}

/// A sink that keeps at most [`DRAFT_LIMIT`] bytes, and then only counts.
#[derive(Default)]
struct Capped {
    bytes: Vec<u8>,
    over: bool,
}

impl Write for Capped {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        if !self.over && self.bytes.len() + data.len() <= DRAFT_LIMIT {
            self.bytes.extend_from_slice(data);
        } else if !self.over {
            self.over = true;
            self.bytes = Vec::new();
        }
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
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
    /// A POST that the client sends one time, also after a rate limit
    /// (spec `docs/specs/2026-11-16-send.md`, section 5.2).
    PostOnce(&'a [u8]),
    /// A PUT that the client sends one time, as [`Method::PostOnce`].
    PutOnce(&'a [u8]),
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
        let p: ProfileJson = self.json(token, "profile", &[], cost::PROFILE)?;
        Ok(Profile {
            email_address: p.email_address,
            messages_total: p.messages_total,
            threads_total: p.threads_total,
            cursor: p.history_id.to_string(),
        })
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

    /// One page of `users.messages.list` for the Gmail search `q`, with
    /// the spam and the trash. Recovery finds a sent message with
    /// `rfc822msgid:<id>` (spec R3-01, section 6).
    pub fn search(&self, token: &str, q: &str, page_token: Option<&str>) -> Result<MessagePage> {
        let mut query = vec![
            ("maxResults", PAGE_SIZE.to_string()),
            ("q", q.to_string()),
            ("includeSpamTrash", "true".into()),
        ];
        if let Some(t) = page_token {
            query.push(("pageToken", t.into()));
        }
        self.json(token, "messages", &query, cost::MESSAGES_LIST)
    }

    /// Each draft of `users.drafts.list` for the Gmail search `q`. The
    /// client reads every page, so recovery does not miss a draft on a
    /// later page (spec R3-01, section 6).
    pub fn drafts(&self, token: &str, q: &str) -> Result<Vec<Draft>> {
        let mut all = Vec::new();
        let mut next: Option<String> = None;
        loop {
            let mut query = vec![("maxResults", PAGE_SIZE.to_string()), ("q", q.to_string())];
            if let Some(t) = next.take() {
                query.push(("pageToken", t));
            }
            let page: DraftPage = self.json(token, "drafts", &query, cost::DRAFTS_LIST)?;
            all.extend(page.drafts);
            match page.next_page_token {
                Some(t) if !t.is_empty() => next = Some(t),
                _ => return Ok(all),
            }
        }
    }

    /// `users.drafts.get` with `format=raw`. HTTP 404 gives
    /// [`Error::NotFound`]: the draft is gone, deleted or sent. The client
    /// streams the answer, so a large draft (for example with an
    /// attachment that the person added) does not fail: its `raw` is
    /// `None` when the message is larger than [`DRAFT_LIMIT`].
    pub fn draft_raw(&self, token: &str, id: &str) -> Result<DraftRaw> {
        #[derive(Deserialize)]
        struct Answer {
            id: String,
            message: MessageRef,
        }
        let response = self.call(
            token,
            Method::Get,
            &format!("drafts/{}", segment(id)),
            &[("format", "raw".into())],
            cost::DRAFTS_GET,
        )?;
        let mut sink = Capped::default();
        let (rest, _) = raw::split_draft(response.into_body().into_reader(), &mut sink).map_err(
            |e| match e {
                raw::SplitError::Read(e) => Error::Transport(e.to_string()),
                raw::SplitError::Write(e) => Error::Sink(e),
                raw::SplitError::Format(e) => Error::Decode(e.to_string()),
            },
        )?;
        let a: Answer = parse(&rest)?;
        Ok(DraftRaw {
            id: a.id,
            message: a.message,
            raw: (!sink.over).then_some(sink.bytes),
        })
    }

    /// `users.drafts.create` with a raw RFC 5322 message, in the thread
    /// `thread_id` for a reply. One request, as [`Gmail::send_draft`].
    pub fn create_draft(&self, token: &str, raw: &[u8], thread_id: Option<&str>) -> Result<Draft> {
        let body = draft_body(None, raw, thread_id);
        self.write(
            token,
            Method::PostOnce(body.as_bytes()),
            "drafts",
            cost::DRAFTS_CREATE,
            "draft create",
        )
    }

    /// `users.drafts.update`: replaces the message of the draft `id` with
    /// `raw`, in the thread `thread_id` for a reply. One request, as
    /// [`Gmail::send_draft`]: the caller never makes the call again after an
    /// unknown result (`docs/specs/2026-11-25-draft-change.md`, section 4).
    pub fn update_draft(
        &self,
        token: &str,
        id: &str,
        raw: &[u8],
        thread_id: Option<&str>,
    ) -> Result<Draft> {
        let body = draft_body(Some(id), raw, thread_id);
        self.write(
            token,
            Method::PutOnce(body.as_bytes()),
            &format!("drafts/{}", segment(id)),
            cost::DRAFTS_UPDATE,
            "draft update",
        )
    }

    /// `users.drafts.send` of the draft `id`, with `raw` as the message of
    /// the draft, so that Gmail sends these bytes. One request: the client
    /// never tries again, also after a rate limit, and the caller never
    /// makes the call again after an unknown result (spec 2, section 7.5).
    pub fn send_draft(
        &self,
        token: &str,
        id: &str,
        raw: &[u8],
        thread_id: Option<&str>,
    ) -> Result<Sent> {
        let body = draft_body(Some(id), raw, thread_id);
        self.write(
            token,
            Method::PostOnce(body.as_bytes()),
            "drafts/send",
            cost::DRAFTS_SEND,
            "draft send",
        )
    }

    /// A write call with a JSON answer. Gmail accepted the call when the
    /// status is 2xx, so a bad answer leaves the result unknown.
    fn write<T: DeserializeOwned>(
        &self,
        token: &str,
        method: Method<'_>,
        path: &str,
        units: u32,
        what: &str,
    ) -> Result<T> {
        let response = self.call(token, method, path, &[], units)?;
        read_all(response)
            .and_then(|b| parse(&b))
            .map_err(|e| Error::Transport(format!("answer of an accepted {what}: {e}")))
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
            internal_date: f.internal_date,
            size,
        })
    }

    /// One page of `users.history.list` after the history ID `start`. An
    /// empty `types` gives each type. [`Error::HistoryExpired`] means: do a
    /// full resync. A `start` that is not a history ID gives
    /// [`Error::Decode`], and the client sends no call.
    pub fn history(
        &self,
        token: &str,
        start: &str,
        types: &[HistoryType],
        page_token: Option<&str>,
    ) -> Result<HistoryPage> {
        let mut q = vec![
            ("startHistoryId", parse_history(start)?.to_string()),
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
                History { changes }
            })
            .collect();
        Ok(HistoryPage {
            history,
            cursor: page.history_id.to_string(),
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
                Method::Post(body) | Method::PostOnce(body) => {
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
                Method::PutOnce(body) => {
                    let mut r = self
                        .agent
                        .put(&url)
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
            let once = matches!(method, Method::PostOnce(_) | Method::PutOnce(_));
            if (rate_limit && !once) || (is_read && status >= 500) {
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

/// The JSON body of `drafts.create`, `drafts.update` and `drafts.send`.
fn draft_body(id: Option<&str>, raw: &[u8], thread_id: Option<&str>) -> String {
    let mut message = serde_json::json!({ "raw": encode_url(raw) });
    if let Some(t) = thread_id {
        message["threadId"] = t.into();
    }
    let mut body = serde_json::json!({ "message": message });
    if let Some(id) = id {
        body["id"] = id.into();
    }
    body.to_string()
}

/// Escape an ID for one segment of the URL path.
/// A history ID of Gmail, from the cursor text of the mirror.
fn parse_history(cursor: &str) -> Result<u64> {
    // `u64::from_str` also takes a leading `+`, which is not a history ID.
    cursor
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| cursor.parse().ok())
        .flatten()
        .ok_or_else(|| Error::Decode(format!("not a Gmail history ID: {cursor:?}")))
}

impl Provider for Gmail {
    fn name(&self) -> &'static str {
        "Gmail"
    }
    fn profile(&self, token: &str) -> Result<Profile> {
        Gmail::profile(self, token)
    }
    fn labels(&self, token: &str) -> Result<Vec<Label>> {
        Gmail::labels(self, token)
    }
    fn messages(
        &self,
        token: &str,
        label: Option<&str>,
        page: Option<&str>,
    ) -> Result<MessagePage> {
        Gmail::messages(self, token, label.as_slice(), true, page)
    }
    fn changes(&self, token: &str, from: &str, page: Option<&str>) -> Result<HistoryPage> {
        Gmail::history(self, token, from, &HistoryType::ALL, page)
    }
    fn minimal(&self, token: &str, id: &str) -> Result<Minimal> {
        Gmail::minimal(self, token, id)
    }
    fn metadata(&self, token: &str, id: &str, headers: &[&str]) -> Result<Metadata> {
        Gmail::metadata(self, token, id, headers)
    }
    fn raw(&self, token: &str, id: &str, sink: &mut dyn Write) -> Result<RawMessage> {
        Gmail::raw(self, token, id, &mut { sink })
    }
    fn messages_with_id(&self, token: &str, message_id: &str) -> Result<Vec<MessageRef>> {
        let q = format!("rfc822msgid:{message_id}");
        let mut all = Vec::new();
        let mut next: Option<String> = None;
        loop {
            let page = Gmail::search(self, token, &q, next.as_deref())?;
            all.extend(page.messages);
            match page.next_page_token {
                Some(t) if !t.is_empty() => next = Some(t),
                _ => return Ok(all),
            }
        }
    }
    fn modify(&self, token: &str, ids: &[&str], add: &[&str], remove: &[&str]) -> Result<()> {
        Gmail::batch_modify(self, token, ids, add, remove)
    }
    fn trash(&self, token: &str, id: &str) -> Result<Minimal> {
        Gmail::trash(self, token, id)
    }
    fn untrash(&self, token: &str, id: &str) -> Result<Minimal> {
        Gmail::untrash(self, token, id)
    }
    fn create_label(&self, token: &str, name: &str) -> Result<Label> {
        Gmail::create_label(self, token, name)
    }
    fn drafts_with_id(&self, token: &str, message_id: &str) -> Result<Vec<Draft>> {
        Gmail::drafts(self, token, &format!("rfc822msgid:{message_id}"))
    }
    fn draft_raw(&self, token: &str, id: &str) -> Result<DraftRaw> {
        Gmail::draft_raw(self, token, id)
    }
    fn create_draft(&self, token: &str, raw: &[u8], thread: Option<&str>) -> Result<Draft> {
        Gmail::create_draft(self, token, raw, thread)
    }
    fn send_draft(&self, token: &str, id: &str, raw: &[u8], thread: Option<&str>) -> Result<Sent> {
        Gmail::send_draft(self, token, id, raw, thread)
    }
    fn update_draft(
        &self,
        token: &str,
        id: &str,
        raw: &[u8],
        thread: Option<&str>,
    ) -> Result<Draft> {
        Gmail::update_draft(self, token, id, raw, thread)
    }
    fn updates_drafts(&self) -> bool {
        true
    }
    fn sets_draft_message_id(&self) -> bool {
        true
    }
}

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
    (
        reason_of(&v),
        v["error"]["message"].as_str().unwrap_or("").to_string(),
    )
}

/// The reason of a Gmail error answer: `error.errors[0].reason`, else the
/// reason of the first `error.details` entry (the newer `ErrorInfo` form,
/// for example `ACCESS_TOKEN_SCOPE_INSUFFICIENT`).
fn reason_of(v: &serde_json::Value) -> String {
    let e = &v["error"];
    let first = |r: &serde_json::Value| r.as_str().filter(|s| !s.is_empty()).map(str::to_string);
    first(&e["errors"][0]["reason"])
        .or_else(|| {
            e["details"]
                .as_array()
                .into_iter()
                .flatten()
                .find_map(|d| first(&d["reason"]))
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reason_comes_from_both_error_forms() {
        let old = serde_json::json!({"error": {"code": 403, "errors": [
            {"reason": "insufficientPermissions"}]}});
        assert_eq!(reason_of(&old), "insufficientPermissions");
        let new = serde_json::json!({"error": {"code": 403, "status": "PERMISSION_DENIED",
            "details": [{"@type": "type.googleapis.com/google.rpc.ErrorInfo",
                         "reason": "ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}});
        assert_eq!(reason_of(&new), "ACCESS_TOKEN_SCOPE_INSUFFICIENT");
        let both = serde_json::json!({"error": {"errors": [{"reason": "rateLimitExceeded"}],
            "details": [{"reason": "RATE_LIMIT_EXCEEDED"}]}});
        assert_eq!(
            reason_of(&both),
            "rateLimitExceeded",
            "the old form comes first"
        );
        for none in [
            serde_json::json!({}),
            serde_json::json!({"error": {"details": "x"}}),
            serde_json::json!({"error": {"errors": [{"reason": 3}]}}),
        ] {
            assert_eq!(reason_of(&none), "");
        }
    }

    /// Property: for generated error answers, `reason_of` gives the first
    /// non-empty string reason of `error.errors[0]`, else of the first
    /// `error.details` entry that has one, else "". It never panics.
    #[test]
    fn generated_error_answers() {
        use serde_json::{json, Value};
        let reasons: [Value; 6] = [
            json!("insufficientPermissions"),
            json!("ACCESS_TOKEN_SCOPE_INSUFFICIENT"),
            json!(""),
            json!(7),
            json!(null),
            json!({"x": 1}),
        ];
        let mut rng = Rng(0x5eed);
        for _ in 0..10_000 {
            let mut pick = |n: usize| rng.next() % n;
            let entry = |r: &Value, with: bool| {
                if with {
                    json!({"reason": r})
                } else {
                    json!({})
                }
            };
            let errors: Vec<Value> = (0..pick(3))
                .map(|_| entry(&reasons[pick(6)], pick(4) > 0))
                .collect();
            let details: Vec<Value> = (0..pick(4))
                .map(|_| entry(&reasons[pick(6)], pick(4) > 0))
                .collect();
            let mut e = serde_json::Map::new();
            match pick(3) {
                0 => {}
                1 => drop(e.insert("errors".into(), json!(errors.clone()))),
                _ => drop(e.insert("errors".into(), json!("not a list"))),
            }
            match pick(3) {
                0 => {}
                1 => drop(e.insert("details".into(), json!(details.clone()))),
                _ => drop(e.insert("details".into(), json!({"reason": "x"}))),
            }
            let v = match pick(4) {
                0 => json!([1, 2]),
                1 => json!({"other": e}),
                _ => json!({ "error": e }),
            };
            let s = |r: &Value| r.as_str().filter(|s| !s.is_empty()).map(str::to_string);
            let err = &v["error"];
            let want = err["errors"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|x| s(&x["reason"]))
                .or_else(|| {
                    err["details"]
                        .as_array()
                        .and_then(|d| d.iter().find_map(|x| s(&x["reason"])))
                })
                .unwrap_or_default();
            assert_eq!(reason_of(&v), want, "{v}");
        }
    }

    /// A small random number generator with a fixed seed.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 16) as usize
        }

        fn below(&mut self, n: usize) -> usize {
            self.next() % n.max(1)
        }

        /// A short text of digits, signs, letters, spaces and other bytes.
        fn text(&mut self) -> String {
            const PARTS: [&str; 10] = [
                "0",
                "7",
                "9",
                "18446744073709551615",
                "-",
                "+",
                "a",
                " ",
                ".",
                "é",
            ];
            (0..self.below(6))
                .map(|_| PARTS[self.below(PARTS.len())])
                .collect()
        }
    }

    // Property: a cursor is a history ID if and only if it is the decimal
    // text of a u64, and the call then carries that number.
    #[test]
    fn a_cursor_is_accepted_only_as_a_u64() {
        let mut rng = Rng(0x5eed_1d0c);
        for _ in 0..20_000 {
            let text = rng.text();
            let number = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
            if let Err(e) = parse_history(&text) {
                assert!(matches!(e, Error::Decode(_)), "{text:?}: {e}");
            }
            assert_eq!(
                parse_history(&text).ok(),
                text.parse::<u64>().ok().filter(|_| number)
            );
        }
    }

    // Property: a profile answer gives a profile whose cursor is the decimal
    // text of its history ID, or Error::Decode. It never gives another
    // cursor, and never panics.
    #[test]
    fn a_profile_answer_gives_a_decimal_cursor_or_a_decode_error() {
        let mut rng = Rng(0x0dd_ba11);
        for _ in 0..20_000 {
            let id = rng.text();
            let value = match rng.below(4) {
                0 => serde_json::Value::String(id.clone()),
                1 => id
                    .parse::<u64>()
                    .map(serde_json::Value::from)
                    .unwrap_or(serde_json::Value::Null),
                2 => serde_json::Value::Bool(true),
                _ => serde_json::Value::from(-1),
            };
            let mut answer =
                serde_json::json!({ "emailAddress": "a@example.com", "historyId": value });
            if rng.below(8) == 0 {
                answer.as_object_mut().unwrap().remove("emailAddress");
            }
            let bytes = answer.to_string();
            match parse::<ProfileJson>(bytes.as_bytes()) {
                Ok(p) => {
                    let cursor = p.history_id.to_string();
                    assert_eq!(parse_history(&cursor).unwrap(), p.history_id);
                    assert!(answer["emailAddress"].is_string(), "{bytes}");
                }
                Err(Error::Decode(_)) => {}
                Err(e) => panic!("{bytes}: {e}"),
            }
        }
    }

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
}
