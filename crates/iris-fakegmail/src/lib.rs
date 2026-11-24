//! A fake Gmail API v1 server for tests, on top of the synthetic mailbox.
//!
//! `FakeGmail::start(mailbox)` serves on `127.0.0.1` at a random port, in a
//! background thread with its own runtime. It needs no network and no
//! credentials. It accepts any `Authorization: Bearer` token.
//!
//! Supported: `users.getProfile`, `users.labels.list`, `users.labels.get`,
//! `users.messages.list` (paging, `labelIds`, `includeSpamTrash`, and `q`
//! only as `rfc822msgid:<id>`), `users.messages.get` (`raw`, `metadata`,
//! `minimal`), `users.messages.send`, `users.drafts.create`,
//! `.get` (`raw`), `.list` (`q` as above) and `.send`,
//! `users.messages.batchModify`, `users.messages.trash`,
//! `users.messages.untrash`, `users.labels.create` and
//! `users.history.list` (paging, `historyTypes`). The JSON has the shape
//! of the real API. The fake returns 400 for `format=full` and for an absent
//! format, because the real default is `full` and the fake does not build it.
//!
//! A state layer on top of the mailbox gives each message a label set:
//! `SENT` or `DRAFT` for the mail of the owner, `SPAM` for some hostile mail,
//! `TRASH` for some other mail, and the user labels `Receipts` and
//! `Work/Projects`. A test changes the state with the methods of
//! [`FakeGmail`]. Each change makes the history record that Gmail makes.
//!
//! `messages.get` returns only the fields that the Gmail documentation
//! promises for each format, so that a test cannot pass on a field that Gmail
//! does not send:
//!
//! - `minimal`: "only email message ID and labels": `id` and `labelIds`.
//! - `metadata`: "only email message ID, labels, and email headers": `id`,
//!   `labelIds` and `payload.headers`.
//! - `raw`: "the full email message data with body content in the raw field":
//!   each field of the Message resource except `payload`.
//!
//! Like Gmail, the fake leaves out `labelIds` when the set is empty.

// Handlers return a full `Response` as the error. The size does not matter in a test server.
#![allow(clippy::result_large_err)]

use std::collections::{BTreeSet, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, RawQuery, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use iris_synth::{base64, Form, Mailbox, Sender, OWNER};
use serde::Deserialize;
use serde_json::{json, Value};

/// History ids start above this value, so they do not look like indexes.
const HISTORY_BASE: u64 = 100_000;

/// The system labels of Gmail.
const SYSTEM_LABELS: [&str; 13] = [
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

/// The user labels of a new fake: (id, name).
const USER_LABELS: [(&str, &str); 2] = [("Label_1", "Receipts"), ("Label_2", "Work/Projects")];

struct Label {
    id: String,
    name: String,
    system: bool,
}

struct Msg {
    thread: String,
    labels: BTreeSet<String>,
    /// The id of the last history record that names the message.
    history: u64,
    date_ms: i64,
    /// `None`: the raw message comes from the synthetic mailbox.
    raw: Option<Vec<u8>>,
    deleted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Added,
    Deleted,
    LabelAdded,
    LabelRemoved,
}

impl Kind {
    fn parse(name: &str) -> Option<Kind> {
        match name {
            "messageAdded" => Some(Kind::Added),
            "messageDeleted" => Some(Kind::Deleted),
            "labelAdded" => Some(Kind::LabelAdded),
            "labelRemoved" => Some(Kind::LabelRemoved),
            _ => None,
        }
    }
}

struct Event {
    id: u64,
    msg: usize,
    kind: Kind,
    /// The labels that a label change added or removed.
    labels: Vec<String>,
}

struct Store {
    mailbox: Mailbox,
    /// The `emailAddress` of the profile.
    address: String,
    msgs: Vec<Msg>,
    labels: Vec<Label>,
    events: Vec<Event>,
    next_label: u32,
    /// A `startHistoryId` below this value gets 404, as an expired id does.
    history_floor: u64,
    /// The next requests get these HTTP status codes in place of an answer.
    failures: VecDeque<u16>,
    /// The next write requests (POST) get these status codes in place of an
    /// answer: `(status, applied)`. With `applied`, the fake makes the change
    /// first, as when Gmail applies a call and the answer is lost.
    write_failures: VecDeque<(u16, bool)>,
    /// A failure for the next request whose log line contains the text.
    path_failure: Option<(String, u16)>,
    /// Each write request gets HTTP 403 `insufficientPermissions`, as when
    /// the token has `gmail.readonly` only.
    read_only: bool,
    /// A step that runs before the answer of the next request whose log
    /// line contains the text: a change by another process during a call.
    before_matching: Option<(String, Box<dyn FnOnce() + Send>)>,
    /// The most items on one list page, below `maxResults`.
    page_limit: usize,
    /// A draft edit that the next `drafts.get` of the draft starts, after
    /// its answer: the person edits the draft between a read and a send.
    edit_after_get: Option<(String, Vec<u8>)>,
    /// True: the sent copy of `drafts.send` has no `Bcc` header.
    drop_sent_bcc: bool,
    /// Each request: method, path and query.
    log: Vec<String>,
    /// The drafts: (draft id, message index). A sent or deleted draft
    /// leaves the list.
    drafts: Vec<(String, usize)>,
    next_draft: u64,
}

impl Store {
    fn new(mailbox: Mailbox) -> Store {
        let n = mailbox.len();
        // The newest 1 % (at least one) of the mail in the inbox is unread.
        let mut unread = n.div_ceil(100);
        let mut msgs: Vec<Msg> = Vec::with_capacity(n);
        for i in 0..n {
            let t = mailbox.truth(i);
            msgs.push(Msg {
                thread: gmail_id(t.thread as u64),
                labels: first_labels(i, t.form, t.sender),
                history: HISTORY_BASE + i as u64 + 1,
                date_ms: t.date * 1000,
                raw: None,
                deleted: false,
            });
        }
        for m in msgs.iter_mut().rev() {
            if unread == 0 {
                break;
            }
            if m.labels.contains("INBOX") {
                m.labels.insert("UNREAD".into());
                unread -= 1;
            }
        }
        let events = (0..n)
            .map(|i| Event {
                id: HISTORY_BASE + i as u64 + 1,
                msg: i,
                kind: Kind::Added,
                labels: Vec::new(),
            })
            .collect();
        let mut labels: Vec<Label> = SYSTEM_LABELS
            .iter()
            .map(|n| Label {
                id: n.to_string(),
                name: n.to_string(),
                system: true,
            })
            .collect();
        labels.extend(USER_LABELS.iter().map(|(id, name)| Label {
            id: id.to_string(),
            name: name.to_string(),
            system: false,
        }));
        Store {
            mailbox,
            address: OWNER.to_string(),
            msgs,
            labels,
            events,
            next_label: USER_LABELS.len() as u32 + 1,
            // The profile id of an empty mailbox is HISTORY_BASE, so it must be valid.
            history_floor: HISTORY_BASE,
            failures: VecDeque::new(),
            write_failures: VecDeque::new(),
            path_failure: None,
            read_only: false,
            before_matching: None,
            page_limit: usize::MAX,
            edit_after_get: None,
            drop_sent_bcc: false,
            log: Vec::new(),
            drafts: Vec::new(),
            next_draft: 1,
        }
    }

    fn latest_history(&self) -> u64 {
        self.events.last().map_or(HISTORY_BASE, |e| e.id)
    }

    fn record(&mut self, msg: usize, kind: Kind, labels: Vec<String>) {
        let id = self.latest_history() + 1;
        self.msgs[msg].history = id;
        self.events.push(Event {
            id,
            msg,
            kind,
            labels,
        });
    }

    fn raw(&self, i: usize) -> Vec<u8> {
        match &self.msgs[i].raw {
            None => self.mailbox.raw(i),
            Some(r) => r.clone(),
        }
    }

    /// The index of a message that is not deleted.
    fn index(&self, id: &str) -> Option<usize> {
        let i = u64::from_str_radix(id, 16).ok()?.checked_sub(1)? as usize;
        (i < self.msgs.len() && !self.msgs[i].deleted).then_some(i)
    }

    fn has_label(&self, id: &str) -> bool {
        self.labels.iter().any(|l| l.id == id)
    }

    fn live(&self) -> impl Iterator<Item = &Msg> {
        self.msgs.iter().filter(|m| !m.deleted)
    }

    fn summary(&self, i: usize) -> Value {
        json!({ "id": gmail_id(i as u64), "threadId": self.msgs[i].thread })
    }

    fn with_labels(&self, i: usize) -> Value {
        let mut v = self.summary(i);
        put_labels(&mut v, &self.msgs[i].labels);
        v
    }

    fn add(&mut self, raw: Vec<u8>, thread: Option<String>, labels: BTreeSet<String>) -> usize {
        let i = self.msgs.len();
        let thread = thread.unwrap_or_else(|| format!("{:016x}", 0x8000_0000u64 + i as u64));
        self.msgs.push(Msg {
            thread,
            labels,
            history: 0,
            date_ms: 1_767_225_600_000 + i as i64,
            raw: Some(raw),
            deleted: false,
        });
        self.record(i, Kind::Added, Vec::new());
        i
    }

    /// The message index of the draft `id`.
    fn draft(&self, id: &str) -> Option<usize> {
        self.drafts.iter().find(|(d, _)| d == id).map(|&(_, i)| i)
    }

    /// Replace the message of the draft `id` with `raw`, as Gmail does: a
    /// new message, and the old one is deleted. Returns false if there is
    /// no such draft.
    fn replace_draft(&mut self, id: &str, raw: Vec<u8>) -> bool {
        let Some(pos) = self.drafts.iter().position(|(d, _)| d == id) else {
            return false;
        };
        let old = self.drafts[pos].1;
        let thread = self.msgs[old].thread.clone();
        self.msgs[old].deleted = true;
        self.record(old, Kind::Deleted, Vec::new());
        let i = self.add(raw, Some(thread), BTreeSet::from(["DRAFT".to_string()]));
        self.drafts[pos].1 = i;
        true
    }

    fn draft_json(&self, id: &str, i: usize) -> Value {
        json!({ "id": id, "message": { "id": gmail_id(i as u64), "threadId": self.msgs[i].thread, "labelIds": ["DRAFT"] } })
    }

    /// True if the `Message-ID` of message `i` is `want` (with or without
    /// the angle brackets), as the Gmail search `rfc822msgid:` finds it.
    fn has_message_id(&self, i: usize, want: &str) -> bool {
        let bare = |v: &str| {
            v.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_ascii_lowercase()
        };
        headers_of(&self.raw(i))
            .iter()
            .any(|(n, v)| n.eq_ignore_ascii_case("message-id") && bare(v) == bare(want))
    }

    fn label_json(&self, l: &Label) -> Value {
        let kind = if l.system { "system" } else { "user" };
        json!({ "id": l.id, "name": l.name, "type": kind })
    }
}

/// The label set of message `i` of the synthetic mailbox.
fn first_labels(i: usize, form: Form, sender: Sender) -> BTreeSet<String> {
    let mut labels: Vec<&str> = Vec::new();
    if sender == Sender::Owner {
        // Each 25th message of the owner is a draft, not sent mail.
        labels.push(if i % 25 == 3 { "DRAFT" } else { "SENT" });
    } else if sender == Sender::Hostile && i.is_multiple_of(2) {
        // Gmail finds half of the hostile mail. The rest stays in the inbox.
        labels.push("SPAM");
    } else {
        labels.push(if i % 40 == 1 { "TRASH" } else { "INBOX" });
        labels.push(match form {
            Form::Feed => "CATEGORY_PROMOTIONS",
            Form::Record | Form::Signal => "CATEGORY_UPDATES",
            _ => "CATEGORY_PERSONAL",
        });
        if i % 30 == 2 {
            labels.push("STARRED");
        }
    }
    if form == Form::Record {
        labels.push("Label_1");
    }
    if form == Form::Conversation && i.is_multiple_of(5) {
        labels.push("Label_2");
    }
    labels.into_iter().map(String::from).collect()
}

/// Gmail leaves out an empty list.
fn put_labels(v: &mut Value, labels: &BTreeSet<String>) {
    if !labels.is_empty() {
        v["labelIds"] = json!(labels);
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
        let store = Arc::new(Mutex::new(Store::new(mailbox)));
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

    /// Give the profile this address in place of the owner of the synthetic
    /// mailbox: a second mailbox for a test with two accounts.
    pub fn set_address(&self, address: &str) {
        self.store.lock().unwrap().address = address.to_string();
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

    /// The Gmail id of message `index` of the synthetic mailbox.
    pub fn id_of(index: usize) -> String {
        gmail_id(index as u64)
    }

    /// Add a message, as a delivery does. Makes a `messageAdded` record.
    /// Returns the Gmail id. Panics on an unknown label.
    pub fn add_message(&self, raw: &[u8], labels: &[&str]) -> String {
        let mut s = self.store.lock().unwrap();
        let set = labels
            .iter()
            .map(|l| l.to_string())
            .collect::<BTreeSet<_>>();
        assert!(set.iter().all(|l| s.has_label(l)), "unknown label");
        let i = s.add(raw.to_vec(), None, set);
        gmail_id(i as u64)
    }

    /// Delete a message for good. Makes a `messageDeleted` record. Returns
    /// false if the message does not exist.
    pub fn delete_message(&self, id: &str) -> bool {
        let mut s = self.store.lock().unwrap();
        let Some(i) = s.index(id) else {
            return false;
        };
        s.msgs[i].deleted = true;
        s.record(i, Kind::Deleted, Vec::new());
        true
    }

    /// Add and remove labels of a message. Makes one `labelAdded` record and
    /// one `labelRemoved` record, each only if it changes a label. Returns
    /// false if the message does not exist. Panics on an unknown label.
    pub fn modify_labels(&self, id: &str, add: &[&str], remove: &[&str]) -> bool {
        let mut s = self.store.lock().unwrap();
        let Some(i) = s.index(id) else {
            return false;
        };
        assert!(
            add.iter().chain(remove).all(|l| s.has_label(l)),
            "unknown label"
        );
        let own = |v: &[&str]| v.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        change_labels(&mut s, i, &own(add), &own(remove));
        true
    }

    /// Make a user label. Returns its id. Gmail makes no history record.
    pub fn create_label(&self, name: &str) -> String {
        let mut s = self.store.lock().unwrap();
        let id = format!("Label_{}", s.next_label);
        s.next_label += 1;
        s.labels.push(Label {
            id: id.clone(),
            name: name.to_string(),
            system: false,
        });
        id
    }

    /// Give a user label a new name. Gmail makes no history record.
    pub fn rename_label(&self, id: &str, name: &str) -> bool {
        let mut s = self.store.lock().unwrap();
        match s.labels.iter_mut().find(|l| l.id == id && !l.system) {
            Some(l) => {
                l.name = name.to_string();
                true
            }
            None => false,
        }
    }

    /// Delete a user label, and remove it from each message. The fake makes
    /// no history record. Spec 1 section 4.3 does not need one: the next
    /// `labels.list` shows the delete.
    pub fn delete_label(&self, id: &str) -> bool {
        let mut s = self.store.lock().unwrap();
        let Some(pos) = s.labels.iter().position(|l| l.id == id && !l.system) else {
            return false;
        };
        s.labels.remove(pos);
        for m in &mut s.msgs {
            m.labels.remove(id);
        }
        true
    }

    /// The next requests get these HTTP status codes, one each, in place of
    /// an answer. 403 and 429 have the reason `rateLimitExceeded`.
    pub fn fail_next(&self, statuses: &[u16]) {
        self.store.lock().unwrap().failures.extend(statuses);
    }

    /// With `true`, each write request (not GET) gets HTTP 403 with the
    /// reason `insufficientPermissions` and makes no change, as Gmail
    /// answers a token with `gmail.readonly` only.
    pub fn set_read_only(&self, read_only: bool) {
        self.store.lock().unwrap().read_only = read_only;
    }

    /// The next request whose log line (for example
    /// `GET /gmail/v1/users/me/drafts?q=...`) contains `text` gets this
    /// HTTP status code in place of an answer.
    pub fn fail_next_matching(&self, text: &str, status: u16) {
        self.store.lock().unwrap().path_failure = Some((text.to_string(), status));
    }

    /// Run `step` before the answer of the next request whose log line
    /// contains `text`. The fake holds no lock while the step runs.
    pub fn before_next_matching(&self, text: &str, step: impl FnOnce() + Send + 'static) {
        self.store.lock().unwrap().before_matching = Some((text.to_string(), Box::new(step)));
    }

    /// The next write requests (POST) get these HTTP status codes, one
    /// each, in place of an answer. Status 0 lets one write pass. With
    /// `applied`, the fake makes the change first, so the result at the
    /// client is not known.
    pub fn fail_next_write(&self, statuses: &[u16], applied: bool) {
        let mut s = self.store.lock().unwrap();
        s.write_failures
            .extend(statuses.iter().map(|&c| (c, applied)));
    }

    /// Give at most `n` items on each page of `messages.list` and
    /// `drafts.list`, as Gmail can give fewer items than `maxResults`.
    pub fn set_page_limit(&self, n: usize) {
        self.store.lock().unwrap().page_limit = n.max(1);
    }

    /// Change the message of a draft, as the person does in Gmail. Returns
    /// false if there is no such draft. Makes no request log line.
    pub fn edit_draft(&self, id: &str, raw: &[u8]) -> bool {
        self.store.lock().unwrap().replace_draft(id, raw.to_vec())
    }

    /// Change the message of draft `id` just after the next `drafts.get`
    /// of it answers, as a person who edits the draft at that moment.
    pub fn edit_draft_after_get(&self, id: &str, raw: &[u8]) {
        self.store.lock().unwrap().edit_after_get = Some((id.to_string(), raw.to_vec()));
    }

    /// The sent copy of each later `drafts.send` has no `Bcc` header, as
    /// a provider that drops it from the copy of the sender.
    pub fn drop_sent_bcc(&self) {
        self.store.lock().unwrap().drop_sent_bcc = true;
    }

    /// Delete a draft, as the person does in Gmail. Returns false if there
    /// is no such draft.
    pub fn delete_draft(&self, id: &str) -> bool {
        let mut s = self.store.lock().unwrap();
        let Some(pos) = s.drafts.iter().position(|(d, _)| d == id) else {
            return false;
        };
        let (_, i) = s.drafts.remove(pos);
        s.msgs[i].deleted = true;
        s.record(i, Kind::Deleted, Vec::new());
        true
    }

    /// The IDs of the drafts now.
    pub fn draft_ids(&self) -> Vec<String> {
        let s = self.store.lock().unwrap();
        s.drafts.iter().map(|(d, _)| d.clone()).collect()
    }

    /// Each request so far, as `GET /gmail/v1/users/me/messages?maxResults=5`.
    pub fn requests(&self) -> Vec<String> {
        self.store.lock().unwrap().log.clone()
    }
}

type Shared = Arc<Mutex<Store>>;

fn router(store: Shared) -> Router {
    Router::new()
        .route("/gmail/v1/users/{user}/profile", get(profile))
        .route(
            "/gmail/v1/users/{user}/labels",
            get(labels).post(create_label),
        )
        .route("/gmail/v1/users/{user}/labels/{id}", get(label))
        .route("/gmail/v1/users/{user}/messages", get(list))
        .route("/gmail/v1/users/{user}/messages/send", post(send))
        .route(
            "/gmail/v1/users/{user}/messages/batchModify",
            post(batch_modify),
        )
        .route("/gmail/v1/users/{user}/messages/{id}/trash", post(trash))
        .route(
            "/gmail/v1/users/{user}/messages/{id}/untrash",
            post(untrash),
        )
        .route("/gmail/v1/users/{user}/messages/{id}", get(message))
        .route("/gmail/v1/users/{user}/history", get(history))
        .route(
            "/gmail/v1/users/{user}/drafts",
            get(list_drafts).post(create_draft),
        )
        .route("/gmail/v1/users/{user}/drafts/send", post(send_draft))
        .route("/gmail/v1/users/{user}/drafts/{id}", get(get_draft))
        .layer(middleware::from_fn_with_state(store.clone(), log_and_fail))
        .with_state(store)
}

/// Log each request, and answer with an injected failure if one is queued.
async fn log_and_fail(State(s): State<Shared>, req: Request, next: Next) -> Response {
    let step = {
        let mut s = s.lock().unwrap();
        let line = format!("{} {}", req.method(), req.uri());
        match &s.before_matching {
            Some((t, _)) if line.contains(t.as_str()) => s.before_matching.take().map(|(_, f)| f),
            _ => None,
        }
    };
    if let Some(step) = step {
        step();
    }
    let failure = {
        let mut s = s.lock().unwrap();
        let line = match req.uri().query() {
            Some(q) => format!("{} {}?{q}", req.method(), req.uri().path()),
            None => format!("{} {}", req.method(), req.uri().path()),
        };
        let matched = s
            .path_failure
            .as_ref()
            .is_some_and(|(t, _)| line.contains(t.as_str()));
        s.log.push(line);
        if s.read_only && req.method() != axum::http::Method::GET {
            let message = "Request had insufficient authentication scopes.";
            let body = json!({ "error": {
                "code": 403,
                "message": message,
                "errors": [{ "message": "Insufficient Permission", "domain": "global",
                             "reason": "insufficientPermissions" }],
                "status": "PERMISSION_DENIED",
            }});
            return (StatusCode::FORBIDDEN, Json(body)).into_response();
        }
        if matched {
            let (_, c) = s.path_failure.take().expect("checked");
            return error(
                StatusCode::from_u16(c).unwrap_or(StatusCode::BAD_REQUEST),
                "Injected failure.",
            );
        }
        match s.failures.pop_front() {
            Some(c) => Some((c, false)),
            None if req.method() != axum::http::Method::GET => s.write_failures.pop_front(),
            None => None,
        }
    };
    match failure.and_then(|(c, applied)| Some((StatusCode::from_u16(c).ok()?, applied))) {
        Some((code, applied)) => {
            if applied {
                next.run(req).await;
            }
            error(code, "Injected failure.")
        }
        None => next.run(req).await,
    }
}

fn error(code: StatusCode, message: &str) -> Response {
    let (status, reason) = match code.as_u16() {
        400 => ("INVALID_ARGUMENT", "badRequest"),
        401 => ("UNAUTHENTICATED", "authError"),
        403 | 429 => ("RESOURCE_EXHAUSTED", "rateLimitExceeded"),
        404 => ("NOT_FOUND", "notFound"),
        409 => ("ALREADY_EXISTS", "conflict"),
        _ => ("UNAVAILABLE", "backendError"),
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

/// The query parameters of the list calls. Some repeat, so the query is
/// decoded as a list of pairs.
#[derive(Default)]
struct Params {
    max_results: Option<usize>,
    page_token: Option<String>,
    start_history_id: Option<String>,
    label_ids: Vec<String>,
    history_types: Vec<String>,
    include_spam_trash: bool,
    format: Option<String>,
    metadata_headers: Vec<String>,
    /// The `Message-ID` of `q=rfc822msgid:<id>`.
    rfc822msgid: Option<String>,
}

impl Params {
    fn parse(query: Option<String>) -> Result<Params, Response> {
        let query = query.unwrap_or_default();
        let pairs = serde_urlencoded::from_str::<Vec<(String, String)>>(&query)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Invalid query string."))?;
        let mut p = Params::default();
        for (name, value) in pairs {
            match name.as_str() {
                "maxResults" => {
                    p.max_results = Some(
                        value
                            .parse()
                            .map_err(|_| error(StatusCode::BAD_REQUEST, "Invalid maxResults"))?,
                    )
                }
                "pageToken" => p.page_token = Some(value),
                "startHistoryId" => p.start_history_id = Some(value),
                "labelIds" => p.label_ids.push(value),
                "historyTypes" => p.history_types.push(value),
                "includeSpamTrash" => p.include_spam_trash = value == "true",
                "format" => p.format = Some(value),
                "metadataHeaders" => p.metadata_headers.push(value.to_ascii_lowercase()),
                // Other searches are refused, so a test does not get an
                // unfiltered list by mistake.
                "q" => match value.trim().strip_prefix("rfc822msgid:") {
                    Some(id) if !id.trim().is_empty() && !id.contains(char::is_whitespace) => {
                        p.rfc822msgid = Some(id.to_string())
                    }
                    _ => {
                        return Err(error(
                            StatusCode::BAD_REQUEST,
                            "The fake supports only q=rfc822msgid:<id>.",
                        ))
                    }
                },
                _ => {}
            }
        }
        Ok(p)
    }

    /// `maxResults` (default 100, at most 500) and `pageToken` (an offset).
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

/// One page of `items`, with `nextPageToken` if more items follow.
fn page(body: &mut Value, key: &str, items: Vec<Value>, start: usize, size: usize, total: usize) {
    if !items.is_empty() {
        body[key] = Value::Array(items);
    }
    if start.saturating_add(size) < total {
        body["nextPageToken"] = Value::String((start + size).to_string());
    }
}

async fn profile(State(s): State<Shared>, Path(user): Path<String>, h: HeaderMap) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let s = s.lock().unwrap();
    let threads: BTreeSet<&str> = s.live().map(|m| m.thread.as_str()).collect();
    Json(json!({
        "emailAddress": s.address,
        "messagesTotal": s.live().count(),
        "threadsTotal": threads.len(),
        "historyId": s.latest_history().to_string(),
    }))
    .into_response()
}

async fn labels(State(s): State<Shared>, Path(user): Path<String>, h: HeaderMap) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let s = s.lock().unwrap();
    let labels: Vec<Value> = s.labels.iter().map(|l| s.label_json(l)).collect();
    Json(json!({ "labels": labels })).into_response()
}

#[derive(Deserialize)]
struct CreateLabelBody {
    name: String,
}

/// `labels.create`. As Gmail, a name that a label has already, with no
/// regard to case (Unicode case folding), gives HTTP 409.
async fn create_label(
    State(s): State<Shared>,
    Path(user): Path<String>,
    h: HeaderMap,
    body: Result<Json<CreateLabelBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let Ok(Json(body)) = body else {
        return error(StatusCode::BAD_REQUEST, "Invalid JSON payload received.");
    };
    let mut s = s.lock().unwrap();
    // Unicode case folding, as the changeset store and the apply.
    let name = unicase::UniCase::new(body.name.as_str());
    if s.labels.iter().any(|l| {
        unicase::UniCase::new(l.name.as_str()) == name
            || unicase::UniCase::new(l.id.as_str()) == name
    }) {
        return error(StatusCode::CONFLICT, "Label name exists or conflicts");
    }
    let id = format!("Label_{}", s.next_label);
    s.next_label += 1;
    let label = Label {
        id,
        name: body.name,
        system: false,
    };
    let v = s.label_json(&label);
    s.labels.push(label);
    Json(v).into_response()
}

async fn label(
    State(s): State<Shared>,
    Path((user, id)): Path<(String, String)>,
    h: HeaderMap,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let s = s.lock().unwrap();
    let Some(l) = s.labels.iter().find(|l| l.id == id) else {
        return error(StatusCode::NOT_FOUND, "Requested entity was not found.");
    };
    let with: Vec<&Msg> = s.live().filter(|m| m.labels.contains(&l.id)).collect();
    let unread: Vec<&&Msg> = with
        .iter()
        .filter(|m| m.labels.contains("UNREAD"))
        .collect();
    let threads: BTreeSet<&str> = with.iter().map(|m| m.thread.as_str()).collect();
    let unread_threads: BTreeSet<&str> = unread.iter().map(|m| m.thread.as_str()).collect();
    let mut body = s.label_json(l);
    body["messagesTotal"] = json!(with.len());
    body["messagesUnread"] = json!(unread.len());
    body["threadsTotal"] = json!(threads.len());
    body["threadsUnread"] = json!(unread_threads.len());
    Json(body).into_response()
}

async fn list(
    State(s): State<Shared>,
    Path(user): Path<String>,
    RawQuery(query): RawQuery,
    h: HeaderMap,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let p = match Params::parse(query) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let (start, size) = match p.bounds() {
        Ok(b) => b,
        Err(e) => return e,
    };
    let s = s.lock().unwrap();
    let size = size.min(s.page_limit);
    let hidden = |m: &Msg| {
        !p.include_spam_trash && (m.labels.contains("SPAM") || m.labels.contains("TRASH"))
    };
    // Newest first, as Gmail lists.
    let matches: Vec<usize> = (0..s.msgs.len())
        .rev()
        .filter(|&i| {
            let m = &s.msgs[i];
            !m.deleted
                && !hidden(m)
                && p.label_ids.iter().all(|l| m.labels.contains(l))
                && p.rfc822msgid
                    .as_ref()
                    .is_none_or(|q| s.has_message_id(i, q))
        })
        .collect();
    let items = matches
        .iter()
        .skip(start)
        .take(size)
        .map(|&i| s.summary(i))
        .collect();
    let mut body = json!({ "resultSizeEstimate": matches.len() });
    page(&mut body, "messages", items, start, size, matches.len());
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
    let p = match Params::parse(query) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let s = s.lock().unwrap();
    let Some(i) = s.index(&id) else {
        return error(StatusCode::NOT_FOUND, "Requested entity was not found.");
    };
    let m = &s.msgs[i];
    let mut body = json!({ "id": gmail_id(i as u64) });
    put_labels(&mut body, &m.labels);
    match p.format.as_deref() {
        Some("minimal") => {}
        Some("raw") => {
            let raw = s.raw(i);
            body["threadId"] = json!(m.thread);
            body["snippet"] = json!(snippet(&raw));
            body["sizeEstimate"] = json!(raw.len());
            body["historyId"] = json!(m.history.to_string());
            body["internalDate"] = json!(m.date_ms.to_string());
            body["raw"] = Value::String(base64::encode_url(&raw));
        }
        Some("metadata") => {
            let wanted = &p.metadata_headers;
            let headers: Vec<Value> = headers_of(&s.raw(i))
                .into_iter()
                .filter(|(n, _)| wanted.is_empty() || wanted.contains(&n.to_ascii_lowercase()))
                .map(|(n, v)| json!({ "name": n, "value": v }))
                .collect();
            body["payload"] = json!({ "headers": headers });
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
    let i = s.add(raw, body.thread_id, BTreeSet::from(["SENT".to_string()]));
    Json(json!({ "id": gmail_id(i as u64), "threadId": s.msgs[i].thread, "labelIds": ["SENT"] }))
        .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchModifyBody {
    ids: Vec<String>,
    #[serde(default)]
    add_label_ids: Vec<String>,
    #[serde(default)]
    remove_label_ids: Vec<String>,
}

/// Like Gmail: 400 for more than 1,000 IDs or an unknown label. An ID that
/// does not exist also gives 400 here, and the fake changes nothing.
async fn batch_modify(
    State(s): State<Shared>,
    Path(user): Path<String>,
    h: HeaderMap,
    body: Result<Json<BatchModifyBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let Ok(Json(body)) = body else {
        return error(StatusCode::BAD_REQUEST, "Invalid JSON payload received.");
    };
    let mut s = s.lock().unwrap();
    if body.ids.len() > 1000 {
        return error(StatusCode::BAD_REQUEST, "Too many ids.");
    }
    let labels = body.add_label_ids.iter().chain(&body.remove_label_ids);
    if labels.clone().any(|l| !s.has_label(l)) {
        return error(StatusCode::BAD_REQUEST, "Invalid label.");
    }
    let Some(indexes) = body
        .ids
        .iter()
        .map(|id| s.index(id))
        .collect::<Option<Vec<usize>>>()
    else {
        return error(StatusCode::BAD_REQUEST, "Invalid id value.");
    };
    for i in indexes {
        change_labels(&mut s, i, &body.add_label_ids, &body.remove_label_ids);
    }
    StatusCode::NO_CONTENT.into_response()
}

/// shortcut: the fake trash adds `TRASH` and removes `INBOX`; check the
/// real label effect at Gmail before a test depends on `INBOX` after it
/// (issue #172).
async fn trash(
    State(s): State<Shared>,
    Path((user, id)): Path<(String, String)>,
    h: HeaderMap,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let mut s = s.lock().unwrap();
    let Some(i) = s.index(&id) else {
        return error(StatusCode::NOT_FOUND, "Requested entity was not found.");
    };
    change_labels(&mut s, i, &["TRASH".into()], &["INBOX".into()]);
    Json(s.with_labels(i)).into_response()
}

/// shortcut: the fake untrash removes `TRASH` and adds `INBOX`, the
/// inverse of the fake trash; check the real label effect at Gmail before a
/// test depends on `INBOX` after it (issue #172).
async fn untrash(
    State(s): State<Shared>,
    Path((user, id)): Path<(String, String)>,
    h: HeaderMap,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let mut s = s.lock().unwrap();
    let Some(i) = s.index(&id) else {
        return error(StatusCode::NOT_FOUND, "Requested entity was not found.");
    };
    change_labels(&mut s, i, &["INBOX".into()], &["TRASH".into()]);
    Json(s.with_labels(i)).into_response()
}

/// Add and remove labels of message `i`, with the history records.
fn change_labels(s: &mut Store, i: usize, add: &[String], remove: &[String]) {
    let added: Vec<String> = add
        .iter()
        .filter(|l| s.msgs[i].labels.insert(l.to_string()))
        .cloned()
        .collect();
    if !added.is_empty() {
        s.record(i, Kind::LabelAdded, added);
    }
    let removed: Vec<String> = remove
        .iter()
        .filter(|l| s.msgs[i].labels.remove(*l))
        .cloned()
        .collect();
    if !removed.is_empty() {
        s.record(i, Kind::LabelRemoved, removed);
    }
}

async fn history(
    State(s): State<Shared>,
    Path(user): Path<String>,
    RawQuery(query): RawQuery,
    h: HeaderMap,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let p = match Params::parse(query) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let (offset, size) = match p.bounds() {
        Ok(b) => b,
        Err(e) => return e,
    };
    let Some(start) = p.start_history_id.and_then(|v| v.parse::<u64>().ok()) else {
        return error(StatusCode::BAD_REQUEST, "Missing or invalid startHistoryId");
    };
    let mut kinds = Vec::new();
    for name in &p.history_types {
        match Kind::parse(name) {
            Some(k) => kinds.push(k),
            None => return error(StatusCode::BAD_REQUEST, "Invalid historyTypes"),
        }
    }
    let s = s.lock().unwrap();
    if start < s.history_floor {
        return error(StatusCode::NOT_FOUND, "Requested entity was not found.");
    }
    // Event ids grow, so the records after `start` are a tail of the list.
    let first = s.events.partition_point(|e| e.id <= start);
    let matches: Vec<&Event> = s.events[first..]
        .iter()
        .filter(|e| kinds.is_empty() || kinds.contains(&e.kind))
        .collect();
    let records = matches
        .iter()
        .skip(offset)
        .take(size)
        .map(|e| {
            let m = s.with_labels(e.msg);
            let mut r = json!({ "id": e.id.to_string(), "messages": [s.summary(e.msg)] });
            match e.kind {
                Kind::Added => r["messagesAdded"] = json!([{ "message": m }]),
                Kind::Deleted => r["messagesDeleted"] = json!([{ "message": m }]),
                Kind::LabelAdded => {
                    r["labelsAdded"] = json!([{ "message": m, "labelIds": e.labels }])
                }
                Kind::LabelRemoved => {
                    r["labelsRemoved"] = json!([{ "message": m, "labelIds": e.labels }])
                }
            }
            r
        })
        .collect();
    let mut body = json!({ "historyId": s.latest_history().to_string() });
    page(&mut body, "history", records, offset, size, matches.len());
    Json(body).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DraftMessage {
    raw: String,
    thread_id: Option<String>,
}

#[derive(Deserialize)]
struct DraftBody {
    message: DraftMessage,
}

fn draft_raw(body: &DraftBody) -> Result<Vec<u8>, Response> {
    base64::decode(&body.message.raw)
        .filter(|r| !r.is_empty())
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Invalid value for ByteString: raw"))
}

async fn create_draft(
    State(s): State<Shared>,
    Path(user): Path<String>,
    h: HeaderMap,
    body: Result<Json<DraftBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let Ok(Json(body)) = body else {
        return error(StatusCode::BAD_REQUEST, "Invalid JSON payload received.");
    };
    let raw = match draft_raw(&body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let mut s = s.lock().unwrap();
    let raw = gmail_draft_form(&raw, s.next_draft);
    let i = s.add(
        raw,
        body.message.thread_id,
        BTreeSet::from(["DRAFT".to_string()]),
    );
    let id = format!("r{:015}", s.next_draft);
    s.next_draft += 1;
    s.drafts.push((id.clone(), i));
    Json(s.draft_json(&id, i)).into_response()
}

/// The message of a new draft as real Gmail keeps it (send check of issue
/// #302): Gmail sets its own `Message-ID`, sets a new `Date` and adds a
/// `Received` header in front. The rest of the bytes stay.
fn gmail_draft_form(raw: &[u8], n: u64) -> Vec<u8> {
    const DATE: &str = "Tue, 17 Nov 2026 21:40:12 -0800";
    let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return raw.to_vec();
    };
    let mut out = format!(
        "Received: from 216739018898 named unknown by gmailapi.google.com with HTTPREST; {DATE}\r\n"
    )
    .into_bytes();
    let mut skip = false;
    for line in raw[..end + 2].split_inclusive(|&b| b == b'\n') {
        if skip && line.starts_with(b" ") || skip && line.starts_with(b"\t") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        skip = true;
        if lower.starts_with(b"message-id:") {
            out.extend(format!("Message-ID: <CACe_fake{n:010}@mail.gmail.com>\r\n").bytes());
        } else if lower.starts_with(b"date:") {
            out.extend(format!("Date: {DATE}\r\n").bytes());
        } else {
            skip = false;
            out.extend(line);
        }
    }
    out.extend(&raw[end + 2..]);
    out
}

async fn get_draft(
    State(s): State<Shared>,
    Path((user, id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    h: HeaderMap,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let p = match Params::parse(query) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if p.format.as_deref() != Some("raw") {
        return error(
            StatusCode::BAD_REQUEST,
            "The fake supports only format=raw for a draft.",
        );
    }
    let mut s = s.lock().unwrap();
    let Some(i) = s.draft(&id) else {
        return error(StatusCode::NOT_FOUND, "Requested entity was not found.");
    };
    let mut body = s.draft_json(&id, i);
    body["message"]["raw"] = Value::String(base64::encode_url(&s.raw(i)));
    if s.edit_after_get.as_ref().is_some_and(|(d, _)| *d == id) {
        let (_, raw) = s.edit_after_get.take().expect("checked");
        s.replace_draft(&id, raw);
    }
    Json(body).into_response()
}

async fn list_drafts(
    State(s): State<Shared>,
    Path(user): Path<String>,
    RawQuery(query): RawQuery,
    h: HeaderMap,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let p = match Params::parse(query) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let (start, size) = match p.bounds() {
        Ok(b) => b,
        Err(e) => return e,
    };
    let s = s.lock().unwrap();
    let size = size.min(s.page_limit);
    let matching: Vec<&(String, usize)> = s
        .drafts
        .iter()
        .filter(|&&(_, i)| {
            p.rfc822msgid
                .as_ref()
                .is_none_or(|q| s.has_message_id(i, q))
        })
        .collect();
    let total = matching.len();
    let items: Vec<Value> = matching
        .into_iter()
        .skip(start)
        .take(size)
        .map(|(d, i)| {
            let mut v = s.draft_json(d, *i);
            v["message"]
                .as_object_mut()
                .expect("object")
                .remove("labelIds");
            v
        })
        .collect();
    let mut body = json!({ "resultSizeEstimate": total });
    page(&mut body, "drafts", items, start, size, total);
    Json(body).into_response()
}

#[derive(Deserialize)]
struct SendDraftBody {
    id: String,
    message: Option<DraftMessage>,
}

/// Like Gmail: the draft leaves the drafts, and its text goes out as a new
/// message with the label `SENT` in the thread of the draft. A `message`
/// in the request replaces the text of the draft first, and its `threadId`
/// (or none: a new thread) is the thread of the sent message.
/// `raw` with no `Bcc` header field (and no folded line of it) in the
/// header block.
fn without_bcc(raw: &[u8]) -> Vec<u8> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map_or(raw.len(), |p| p + 2);
    let mut out = Vec::with_capacity(raw.len());
    let mut skip = false;
    for line in raw[..end].split_inclusive(|b| *b == b'\n') {
        let folded = line.first().is_some_and(|b| *b == b' ' || *b == b'\t');
        if !folded {
            skip = line.len() >= 4 && line[..4].eq_ignore_ascii_case(b"bcc:");
        }
        if !skip {
            out.extend_from_slice(line);
        }
    }
    out.extend_from_slice(&raw[end..]);
    out
}

async fn send_draft(
    State(s): State<Shared>,
    Path(user): Path<String>,
    h: HeaderMap,
    body: Result<Json<SendDraftBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err(e) = check(&h, &user) {
        return e;
    }
    let Ok(Json(body)) = body else {
        return error(StatusCode::BAD_REQUEST, "Invalid JSON payload received.");
    };
    let given = match &body.message {
        Some(m) => match base64::decode(&m.raw).filter(|r| !r.is_empty()) {
            Some(r) => Some(r),
            None => return error(StatusCode::BAD_REQUEST, "Invalid value for ByteString: raw"),
        },
        None => None,
    };
    let mut s = s.lock().unwrap();
    let Some(pos) = s.drafts.iter().position(|(d, _)| *d == body.id) else {
        return error(StatusCode::NOT_FOUND, "Requested entity was not found.");
    };
    if let Some(raw) = given {
        s.replace_draft(&body.id, raw);
    }
    let (_, old) = s.drafts.remove(pos);
    // A message in the request goes into the thread of its `threadId`, or
    // into a new thread with none, so a test sees a dropped thread ID.
    let thread = match &body.message {
        Some(m) => m.thread_id.clone(),
        None => Some(s.msgs[old].thread.clone()),
    };
    let mut raw = s.raw(old);
    if s.drop_sent_bcc {
        raw = without_bcc(&raw);
    }
    s.msgs[old].deleted = true;
    s.record(old, Kind::Deleted, Vec::new());
    let i = s.add(raw, thread, BTreeSet::from(["SENT".to_string()]));
    Json(json!({ "id": gmail_id(i as u64), "threadId": s.msgs[i].thread, "labelIds": ["SENT"] }))
        .into_response()
}
