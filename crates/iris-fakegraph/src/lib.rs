//! A fake Microsoft Graph mail server for tests, on top of the synthetic
//! mailbox (spec `docs/specs/2026-11-17-outlook-provider.md`).
//!
//! `FakeGraph::start(mailbox)` serves on `127.0.0.1` at a random port, in a
//! background thread with its own runtime. It needs no network and no
//! credentials. It accepts any `Authorization: Bearer` token.
//!
//! Supported, under `/v1.0`: `GET /me`, `GET /me/mailFolders` (paging with
//! `$top` and `$skip`), `GET /me/mailFolders/{id or well-known name}`,
//! `GET /me/mailFolders/{id}/childFolders`,
//! `GET /me/mailFolders/{id}/messages/delta` (paging, delta links) and
//! `GET /me/messages/{id}` and `GET /me/messages/{id}/$value`.
//!
//! Each request must send `Prefer: IdType="ImmutableId"`: the fake answers
//! HTTP 400 without it, because the real IDs then change on a move.
//!
//! Delta, as the Learn pages say (spec, A1): a round with no token lists the
//! messages of the folder; a round with a token gives each message that
//! changed in the folder since the token, and `@removed` for each message
//! that left the folder (a move or a delete). A move keeps the ID (A3). A
//! token below the floor of [`FakeGraph::expire_delta_before`] gives HTTP 410.

// Handlers return a full `Response` as the error. The size does not matter in a test server.
#![allow(clippy::result_large_err)]

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use iris_synth::{Form, Mailbox, Sender, OWNER};
use serde_json::{json, Value};

/// The folders of a new fake: (id, display name, parent id, well-known name).
/// `Projects` is a child of `Receipts`, and `Old` a child of `Projects`, so
/// that the tree has three levels. `Hidden` is a hidden child folder, and
/// `Unread mail` a search folder.
const FOLDERS: [(&str, &str, Option<&str>, Option<&str>); 11] = [
    ("F-inbox", "Inbox", None, Some("inbox")),
    ("F-sent", "Sent Items", None, Some("sentitems")),
    ("F-drafts", "Drafts", None, Some("drafts")),
    ("F-deleted", "Deleted Items", None, Some("deleteditems")),
    ("F-junk", "Junk Email", None, Some("junkemail")),
    ("F-archive", "Archive", None, Some("archive")),
    ("F-receipts", "Receipts", None, None),
    ("F-projects", "Projects", Some("F-receipts"), None),
    ("F-old", "Old", Some("F-projects"), None),
    ("F-hidden", "Hidden", Some("F-receipts"), None),
    ("F-search", "Unread mail", None, None),
];

/// Graph lists a hidden folder only with `includeHiddenFolders=true`.
const HIDDEN: &str = "F-hidden";
/// A search folder: a view of messages in other folders.
const SEARCH: &str = "F-search";

/// The largest page of the fake, and the page with no `Prefer` size.
const PAGE_MAX: usize = 1000;

struct Msg {
    folder: String,
    thread: u32,
    date: i64,
    categories: Vec<String>,
    read: bool,
    flagged: bool,
    /// `isDraft`. It does not follow the folder: a received message moved
    /// to Drafts stays `false` (spec section 5).
    draft: bool,
    /// The `Message-ID` header, with no angle brackets.
    mid: String,
    /// `None`: the raw message comes from the synthetic mailbox.
    raw: Option<Vec<u8>>,
    deleted: bool,
}

/// One change of a message. `from` is the folder of the message before the
/// change, `None` for a new message.
struct Event {
    seq: u64,
    msg: usize,
    from: Option<String>,
}

struct Store {
    mailbox: Mailbox,
    msgs: Vec<Msg>,
    events: Vec<Event>,
    seq: u64,
    /// Delta tokens below this value are expired.
    floor: u64,
    page_limit: usize,
    failures: VecDeque<(u16, Option<u64>)>,
    /// Bodies of 200 answers in place of the real answers.
    bodies: VecDeque<String>,
    requests: Vec<String>,
    running: usize,
    most_running: usize,
    delay: Duration,
    /// The host of the links in the answers, if not the fake itself.
    link_host: Option<String>,
}

impl Store {
    fn new(mailbox: Mailbox) -> Store {
        let n = mailbox.len();
        let msgs = (0..n)
            .map(|i| {
                let t = mailbox.truth(i);
                Msg {
                    folder: first_folder(i, t.form, t.sender).into(),
                    thread: t.thread,
                    date: t.date,
                    categories: first_categories(i, t.form),
                    read: !i.is_multiple_of(3),
                    flagged: i % 30 == 2,
                    draft: first_folder(i, t.form, t.sender) == "F-drafts",
                    mid: t.message_id,
                    raw: None,
                    deleted: false,
                }
            })
            .collect();
        let events = (0..n)
            .map(|i| Event {
                seq: i as u64 + 1,
                msg: i,
                from: None,
            })
            .collect();
        Store {
            mailbox,
            msgs,
            events,
            seq: n as u64,
            floor: 0,
            page_limit: PAGE_MAX,
            failures: VecDeque::new(),
            bodies: VecDeque::new(),
            requests: Vec::new(),
            running: 0,
            most_running: 0,
            delay: Duration::ZERO,
            link_host: None,
        }
    }

    /// Record a change of message `i`, before the change.
    fn touch(&mut self, i: usize) {
        self.seq += 1;
        let from = Some(self.msgs[i].folder.clone());
        self.events.push(Event {
            seq: self.seq,
            msg: i,
            from,
        });
    }

    fn index(&self, id: &str) -> Option<usize> {
        let i = id.strip_prefix("AAMk")?;
        let i = usize::from_str_radix(i, 16).ok()?.checked_sub(1)?;
        (i < self.msgs.len() && !self.msgs[i].deleted).then_some(i)
    }

    fn raw(&self, i: usize) -> Vec<u8> {
        match &self.msgs[i].raw {
            Some(r) => r.clone(),
            None => self.mailbox.raw(i),
        }
    }

    fn message_json(&self, i: usize) -> Value {
        let m = &self.msgs[i];
        let date = chrono_utc(m.date);
        json!({
            "id": message_id(i),
            "conversationId": format!("AAQk{:08x}", m.thread),
            "parentFolderId": m.folder,
            "categories": m.categories,
            "isRead": m.read,
            "flag": { "flagStatus": if m.flagged { "flagged" } else { "notFlagged" } },
            "isDraft": m.draft,
            "receivedDateTime": date,
            "internetMessageId": format!("<{}>", m.mid),
        })
    }

    fn link(&self, base: &str, rest: &str) -> String {
        match &self.link_host {
            Some(h) => format!("{h}/v1.0{rest}"),
            None => format!("{base}/v1.0{rest}"),
        }
    }
}

/// The folder of message `i` of the synthetic mailbox.
fn first_folder(i: usize, form: Form, sender: Sender) -> &'static str {
    if sender == Sender::Owner {
        // Each 25th message of the owner is a draft, not sent mail.
        return if i % 25 == 3 { "F-drafts" } else { "F-sent" };
    }
    if sender == Sender::Hostile && i.is_multiple_of(2) {
        return "F-junk";
    }
    match i % 40 {
        1 => "F-deleted",
        5 => "F-archive",
        9 if form == Form::Record => "F-receipts",
        13 if form == Form::Conversation => "F-projects",
        17 => "F-old",
        21 => "F-hidden",
        _ => "F-inbox",
    }
}

fn first_categories(i: usize, form: Form) -> Vec<String> {
    let mut c = Vec::new();
    if form == Form::Record {
        c.push("Receipts".to_string());
    }
    if i.is_multiple_of(7) {
        c.push("Blue category".to_string());
    }
    c
}

/// Message `i` has the immutable ID `AAMk` and the hex of `i + 1`.
fn message_id(i: usize) -> String {
    format!("AAMk{:08x}", i + 1)
}

/// Seconds since 1970 as `YYYY-MM-DDTHH:MM:SSZ`.
fn chrono_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    // Civil from days (H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

type Shared = Arc<Mutex<Store>>;

/// A running fake server. The server stops when the process ends.
#[derive(Clone)]
pub struct FakeGraph {
    addr: SocketAddr,
    store: Shared,
}

impl FakeGraph {
    /// Start the server on a random port of `127.0.0.1`.
    pub fn start(mailbox: Mailbox) -> std::io::Result<FakeGraph> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let store = Arc::new(Mutex::new(Store::new(mailbox)));
        let app = router(store.clone());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()?;
        // shortcut: the thread runs until the process ends; add a shutdown if a test needs one.
        std::thread::spawn(move || {
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
                axum::serve(listener, app).await.expect("fake Graph server");
            })
        });
        Ok(FakeGraph { addr, store })
    }

    /// The API root, for example `http://127.0.0.1:41234/v1.0`.
    pub fn base_url(&self) -> String {
        format!("http://{}/v1.0", self.addr)
    }

    /// The immutable ID of message `index` of the synthetic mailbox.
    pub fn id_of(index: usize) -> String {
        message_id(index)
    }

    /// The ID of the folder with this display name. Panics on an unknown name.
    pub fn folder_id(name: &str) -> &'static str {
        FOLDERS
            .iter()
            .find(|f| f.1 == name)
            .map(|f| f.0)
            .expect("a folder of the fake")
    }

    /// The folder of each message that is not deleted: (ID, folder ID).
    pub fn messages(&self) -> BTreeMap<String, String> {
        let s = self.store.lock().unwrap();
        s.msgs
            .iter()
            .enumerate()
            .filter(|(_, m)| !m.deleted)
            .map(|(i, m)| (message_id(i), m.folder.clone()))
            .collect()
    }

    /// Add a message to a folder, as a delivery does. Returns the ID.
    pub fn add_message(&self, raw: &[u8], folder: &str) -> String {
        let mut s = self.store.lock().unwrap();
        let i = s.msgs.len();
        s.msgs.push(Msg {
            folder: folder.to_string(),
            thread: u32::MAX - i as u32,
            date: 1_767_225_600,
            categories: Vec::new(),
            read: false,
            flagged: false,
            draft: folder == "F-drafts",
            mid: format!("added-{i}@fake.example"),
            raw: Some(raw.to_vec()),
            deleted: false,
        });
        s.seq += 1;
        let seq = s.seq;
        s.events.push(Event {
            seq,
            msg: i,
            from: None,
        });
        message_id(i)
    }

    /// Move a message to a folder. The ID stays (spec, A3).
    pub fn move_message(&self, id: &str, folder: &str) -> bool {
        self.change(id, |m| m.folder = folder.to_string())
    }

    pub fn delete_message(&self, id: &str) -> bool {
        self.change(id, |m| m.deleted = true)
    }

    pub fn set_read(&self, id: &str, read: bool) -> bool {
        self.change(id, |m| m.read = read)
    }

    pub fn set_categories(&self, id: &str, categories: &[&str]) -> bool {
        self.change(id, |m| {
            m.categories = categories.iter().map(|c| c.to_string()).collect()
        })
    }

    fn change(&self, id: &str, f: impl FnOnce(&mut Msg)) -> bool {
        let mut s = self.store.lock().unwrap();
        let Some(i) = s.index(id) else {
            return false;
        };
        s.touch(i);
        f(&mut s.msgs[i]);
        true
    }

    /// The sequence number of the last change. A delta link of the fake
    /// holds such a number.
    pub fn seq(&self) -> u64 {
        self.store.lock().unwrap().seq
    }

    /// Make each delta token below `seq` expired: HTTP 410.
    pub fn expire_delta_before(&self, seq: u64) {
        self.store.lock().unwrap().floor = seq;
    }

    /// The most items in one page, below the `Prefer` size of the client.
    pub fn set_page_limit(&self, n: usize) {
        self.store.lock().unwrap().page_limit = n.max(1);
    }

    /// The next requests get these HTTP status codes, each with an optional
    /// `Retry-After` in seconds, in place of an answer.
    pub fn fail_next(&self, failures: &[(u16, Option<u64>)]) {
        self.store.lock().unwrap().failures.extend(failures);
    }

    /// The next requests get a 200 answer with these JSON bodies, in place
    /// of the real answers. A test sends answers in a wrong shape this way.
    pub fn answer_next(&self, bodies: &[&str]) {
        let mut s = self.store.lock().unwrap();
        s.bodies.extend(bodies.iter().map(|b| b.to_string()));
    }

    /// Each request waits this long before its answer, so that a test sees
    /// the requests that run at the same time.
    pub fn set_delay(&self, delay: Duration) {
        self.store.lock().unwrap().delay = delay;
    }

    /// The links of the answers point to this host, for example
    /// `http://127.0.0.1:1`.
    pub fn set_link_host(&self, host: &str) {
        self.store.lock().unwrap().link_host = Some(host.to_string());
    }

    /// Each request so far, as `GET /v1.0/me/...`.
    pub fn requests(&self) -> Vec<String> {
        self.store.lock().unwrap().requests.clone()
    }

    /// The most requests that ran at the same time.
    pub fn most_running(&self) -> usize {
        self.store.lock().unwrap().most_running
    }
}

fn router(store: Shared) -> Router {
    Router::new()
        .route("/v1.0/me", get(me))
        .route("/v1.0/me/mailFolders", get(top_folders))
        .route("/v1.0/me/mailFolders/{id}", get(folder))
        .route("/v1.0/me/mailFolders/{id}/childFolders", get(child_folders))
        .route("/v1.0/me/mailFolders/{id}/messages/delta", get(delta))
        .route("/v1.0/me/messages/{id}", get(message))
        .route("/v1.0/me/messages/{id}/$value", get(value))
        .layer(middleware::from_fn_with_state(store.clone(), gate))
        .with_state(store)
}

/// Log, count, check the token and the `Prefer` header, and answer with an
/// injected failure if one is queued.
async fn gate(State(s): State<Shared>, req: Request, next: Next) -> Response {
    let (failure, body, delay) = {
        let mut s = s.lock().unwrap();
        s.requests.push(format!("{} {}", req.method(), req.uri()));
        s.running += 1;
        s.most_running = s.most_running.max(s.running);
        let failure = s.failures.pop_front();
        let body = if failure.is_none() {
            s.bodies.pop_front()
        } else {
            None
        };
        (failure, body, s.delay)
    };
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let answer = check(req.headers()).err();
    let response = match (answer, failure) {
        (Some(e), _) => e,
        (None, Some((status, after))) => {
            let mut r = error(
                StatusCode::from_u16(status).unwrap(),
                "Injected",
                "an injected failure",
            );
            if (300..400).contains(&status) {
                r.headers_mut().insert(
                    "Location",
                    HeaderValue::from_static("http://127.0.0.1:1/v1.0/me"),
                );
            }
            if let Some(a) = after {
                r.headers_mut().insert("Retry-After", HeaderValue::from(a));
            }
            r
        }
        (None, None) => match body {
            Some(b) => ([("Content-Type", "application/json")], b).into_response(),
            None => next.run(req).await,
        },
    };
    s.lock().unwrap().running -= 1;
    response
}

fn check(h: &HeaderMap) -> Result<(), Response> {
    let auth = h.get("authorization").and_then(|v| v.to_str().ok());
    if auth
        .and_then(|a| a.strip_prefix("Bearer "))
        .is_none_or(|t| t.trim().is_empty())
    {
        return Err(error(
            StatusCode::UNAUTHORIZED,
            "InvalidAuthenticationToken",
            "Access token is empty.",
        ));
    }
    let prefer = h.get("prefer").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !prefer.contains("IdType=\"ImmutableId\"") {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "FakeNeedsImmutableId",
            "the fake answers only with immutable IDs",
        ));
    }
    Ok(())
}

/// A Graph error body (spec, A8).
fn error(status: StatusCode, code: &str, message: &str) -> Response {
    let body = json!({ "error": { "code": code, "message": message } });
    (status, Json(body)).into_response()
}

fn not_found() -> Response {
    error(
        StatusCode::NOT_FOUND,
        "ErrorItemNotFound",
        "The specified object was not found in the store.",
    )
}

fn base(h: &HeaderMap) -> String {
    let host = h.get("host").and_then(|v| v.to_str().ok()).unwrap_or("");
    format!("http://{host}")
}

/// The page size: the `Prefer: odata.maxpagesize` of the client, at most
/// the page limit of the fake.
fn page_size(h: &HeaderMap, limit: usize) -> usize {
    let prefer = h.get("prefer").and_then(|v| v.to_str().ok()).unwrap_or("");
    let asked = prefer
        .split(',')
        .find_map(|p| p.trim().strip_prefix("odata.maxpagesize="))
        .and_then(|n| n.parse().ok())
        .unwrap_or(PAGE_MAX);
    asked.min(limit).max(1)
}

async fn me() -> Response {
    Json(json!({ "id": "fake-user", "mail": OWNER, "userPrincipalName": OWNER })).into_response()
}

fn folder_json(s: &Store, f: &(&str, &str, Option<&str>, Option<&str>)) -> Value {
    let live = s.msgs.iter().filter(|m| !m.deleted && m.folder == f.0);
    let (total, unread) = live.fold((0, 0), |(t, u), m| (t + 1, u + usize::from(!m.read)));
    let children = FOLDERS.iter().filter(|c| c.2 == Some(f.0)).count();
    let mut v = json!({
        "id": f.0,
        "displayName": f.1,
        "parentFolderId": f.2.unwrap_or("F-root"),
        "childFolderCount": children,
        "totalItemCount": total,
        "unreadItemCount": unread,
    });
    if f.0 == SEARCH {
        v["@odata.type"] = "#microsoft.graph.mailSearchFolder".into();
    }
    v
}

async fn folder(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let s = s.lock().unwrap();
    match FOLDERS
        .iter()
        .find(|f| f.0 == id || f.3 == Some(id.as_str()))
    {
        Some(f) => Json(folder_json(&s, f)).into_response(),
        None => not_found(),
    }
}

async fn top_folders(
    State(s): State<Shared>,
    Query(q): Query<BTreeMap<String, String>>,
    h: HeaderMap,
) -> Response {
    folder_page(&s, None, &q, &h, "/me/mailFolders")
}

async fn child_folders(
    State(s): State<Shared>,
    Path(id): Path<String>,
    Query(q): Query<BTreeMap<String, String>>,
    h: HeaderMap,
) -> Response {
    if !FOLDERS.iter().any(|f| f.0 == id) {
        return not_found();
    }
    let path = format!("/me/mailFolders/{id}/childFolders");
    folder_page(&s, Some(&id), &q, &h, &path)
}

fn folder_page(
    s: &Shared,
    parent: Option<&str>,
    q: &BTreeMap<String, String>,
    h: &HeaderMap,
    path: &str,
) -> Response {
    let s = s.lock().unwrap();
    let top: usize = q.get("$top").and_then(|t| t.parse().ok()).unwrap_or(10);
    // The fake gives at most 2 folders in a page, so that a test reads more than one page.
    let top = top.clamp(1, 2);
    let skip: usize = q.get("$skip").and_then(|t| t.parse().ok()).unwrap_or(0);
    let hidden = q.get("includeHiddenFolders").is_some_and(|v| v == "true");
    let all: Vec<_> = FOLDERS
        .iter()
        .filter(|f| f.2 == parent && (hidden || f.0 != HIDDEN))
        .collect();
    let value: Vec<Value> = all
        .iter()
        .skip(skip)
        .take(top)
        .map(|f| folder_json(&s, f))
        .collect();
    let mut body = json!({ "value": value });
    if skip + top < all.len() {
        let rest = format!(
            "{path}?includeHiddenFolders=true&$top={top}&$skip={}",
            skip + top
        );
        body["@odata.nextLink"] = s.link(&base(h), &rest).into();
    }
    Json(body).into_response()
}

/// A delta link of the fake: `$deltatoken=<seq>`. A next link:
/// `$skiptoken=<from>.<to>.<last index>`: the round lists the changes in
/// (from, to] after the message index `last`. A first round has `from` 0
/// and lists the messages in the folder.
async fn delta(
    State(s): State<Shared>,
    Path(id): Path<String>,
    Query(q): Query<BTreeMap<String, String>>,
    h: HeaderMap,
) -> Response {
    let s = s.lock().unwrap();
    if !FOLDERS.iter().any(|f| f.0 == id) {
        return not_found();
    }
    let bad = || {
        error(
            StatusCode::BAD_REQUEST,
            "BadRequest",
            "The delta token is not valid.",
        )
    };
    let (from, to, after) = if let Some(t) = q.get("$skiptoken") {
        let p: Vec<u64> = match t.split('.').map(str::parse).collect() {
            Ok(p) => p,
            Err(_) => return bad(),
        };
        let [from, to, after] = p[..] else {
            return bad();
        };
        (from, to, Some(after as usize))
    } else if let Some(t) = q.get("$deltatoken") {
        match t.parse::<u64>() {
            Ok(from) => (from, s.seq, None),
            Err(_) => return bad(),
        }
    } else {
        (0, s.seq, None)
    };
    if from > 0 && from < s.floor {
        return error(
            StatusCode::GONE,
            "SyncStateNotFound",
            "The sync state is not found.",
        );
    }
    // The messages of the round, by index: true if the message is in the
    // folder now, false if it left the folder.
    let mut round: BTreeMap<usize, bool> = BTreeMap::new();
    for e in s.events.iter().filter(|e| e.seq > from && e.seq <= to) {
        let m = &s.msgs[e.msg];
        let here = !m.deleted && m.folder == id;
        if here {
            round.insert(e.msg, true);
        } else if from > 0 && e.from.as_deref() == Some(id.as_str()) {
            round.insert(e.msg, false);
        }
    }
    if from == 0 {
        // A first round lists the folder as it is now.
        round.retain(|_, here| *here);
    }
    let size = page_size(&h, s.page_limit);
    let start = after.map_or(0, |a| a + 1);
    let mut value = Vec::new();
    let mut last = None;
    for (&i, &here) in round.range(start..).take(size) {
        value.push(if here {
            s.message_json(i)
        } else {
            json!({ "id": message_id(i), "@removed": { "reason": "deleted" } })
        });
        last = Some(i);
    }
    let more = last.is_some_and(|l| round.range(l + 1..).next().is_some());
    let path = format!("/me/mailFolders/{id}/messages/delta");
    let mut body = json!({ "value": value });
    if more {
        let rest = format!("{path}?$skiptoken={from}.{to}.{}", last.unwrap());
        body["@odata.nextLink"] = s.link(&base(&h), &rest).into();
    } else {
        let rest = format!("{path}?$deltatoken={to}");
        body["@odata.deltaLink"] = s.link(&base(&h), &rest).into();
    }
    Json(body).into_response()
}

async fn message(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let s = s.lock().unwrap();
    match s.index(&id) {
        Some(i) => Json(s.message_json(i)).into_response(),
        None => not_found(),
    }
}

async fn value(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let s = s.lock().unwrap();
    match s.index(&id) {
        Some(i) => ([("content-type", "text/plain")], s.raw(i)).into_response(),
        None => not_found(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_utc_text() {
        assert_eq!(chrono_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(chrono_utc(1_767_225_600), "2026-01-01T00:00:00Z");
        assert_eq!(chrono_utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn ids_go_both_ways() {
        let s = Store::new(Mailbox::new(1, 10));
        assert_eq!(s.index(&message_id(9)), Some(9));
        assert_eq!(s.index(&message_id(10)), None);
        assert_eq!(s.index("AAMk00000000"), None);
    }
}
