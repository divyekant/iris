//! A thin Microsoft Graph mail client for the Iris mirror (spec
//! `docs/specs/2026-11-17-outlook-provider.md`).
//!
//! The client is synchronous, as `iris-gmail`. The engine calls it from a
//! blocking thread. Each call takes the OAuth access token as a parameter:
//! the client stores no token. The client:
//!
//! - sends `Prefer: IdType="ImmutableId"` on each request, so that a message
//!   keeps its ID when it moves between folders (spec, A3);
//! - runs at most [`MAX_RUNNING`] requests at the same time (spec, A5);
//! - on HTTP 429 and 503 of each call, and on HTTP 5xx of a read, waits for
//!   `Retry-After`, or else 1 second first, then double up to 5 minutes, plus
//!   a random part of up to 50 percent, and tries again;
//! - calls a `nextLink` or a `deltaLink` only if it starts with the base URL,
//!   so that an answer cannot send the token to another host;
//! - streams the MIME text of a message into a sink, and never holds the
//!   full message.

use std::io::{Read, Write};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

mod provider;
pub use provider::{label_ids, label_list, Outlook};

use serde::de::DeserializeOwned;
use serde::Deserialize;

/// The API root of Graph.
pub const API: &str = "https://graph.microsoft.com/v1.0";
/// The most requests at the same time for one app and one mailbox (spec, A5).
pub const MAX_RUNNING: usize = 4;
/// The page size that the client asks for. Graph can give fewer.
pub const PAGE_SIZE: u32 = 200;
/// The largest MIME text that [`Graph::raw`] copies to a sink.
pub const RAW_LIMIT: u64 = 150 * 1024 * 1024;
/// The largest JSON answer that the client reads into memory.
const ANSWER_LIMIT: u64 = 16 * 1024 * 1024;
/// The fields of a message that a delta query gives.
const SELECT: &str =
    "id,conversationId,parentFolderId,categories,isRead,flag,isDraft,receivedDateTime,createdDateTime,internetMessageId";
/// The well-known folders that the mirror maps (spec section 6).
pub const WELL_KNOWN: [&str; 7] = [
    "inbox",
    "sentitems",
    "drafts",
    "deleteditems",
    "junkemail",
    "archive",
    "outbox",
];

/// The settings of a client.
#[derive(Clone, Debug)]
pub struct Config {
    /// The API root. Tests set the URL of the fake server.
    pub base_url: String,
    /// The first wait after a refused call with no `Retry-After`.
    pub backoff_start: Duration,
    /// The longest wait of the own backoff. A `Retry-After` of Graph is kept
    /// as sent, inside the time limit of the call.
    pub backoff_max: Duration,
    /// The longest time for one call: each try, each wait after a refused
    /// try and the body.
    pub timeout: Duration,
    /// The largest raw message. Tests set a small value.
    pub raw_limit: u64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            base_url: API.to_string(),
            backoff_start: Duration::from_secs(1),
            backoff_max: Duration::from_secs(300),
            timeout: Duration::from_secs(300),
            raw_limit: RAW_LIMIT,
        }
    }
}

#[derive(Debug)]
pub enum Error {
    /// HTTP 401. Get a new access token one time, then call again.
    Unauthorized,
    /// HTTP 404: Graph does not have the item.
    NotFound,
    /// Graph does not accept the stored delta link any more (HTTP 410, or
    /// another 4xx of a stored link). The provider gives `HistoryExpired`: a full resync.
    DeltaReset,
    /// Another HTTP error. `code` is the code of the Graph error body.
    Api {
        status: u16,
        code: String,
        message: String,
    },
    /// The network failed, or the answer stopped before its end.
    Transport(String),
    /// The answer is not in the shape of the Graph API, or is too large.
    Decode(String),
    /// The sink of a raw message failed.
    Sink(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unauthorized => write!(f, "Graph refused the access token (HTTP 401)"),
            Error::NotFound => write!(f, "Graph does not have the item (HTTP 404)"),
            Error::DeltaReset => write!(f, "Graph does not accept the delta link any more"),
            Error::Api {
                status,
                code,
                message,
            } => write!(f, "Graph error {status} {code}: {message}"),
            Error::Transport(e) => write!(f, "network error: {e}"),
            Error::Decode(e) => write!(f, "bad answer from Graph: {e}"),
            Error::Sink(e) => write!(f, "cannot write the raw message: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// For a write: true if Graph can have done the call (spec section 1).
    /// A time-out, a lost answer, a bad answer to an accepted write, 429,
    /// 5xx and an unfollowed 3xx give an unknown result.
    pub fn write_result_unknown(&self) -> bool {
        match self {
            Error::Transport(_) => true,
            Error::Api { status, .. } => *status >= 500 || *status == 429 || *status < 400,
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// A page of [`Graph::all_mail`]: (ID, conversation ID) pairs and the next link.
pub type AllMail = (Vec<(String, String)>, Option<String>);

/// The account of the token: `GET /me`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub id: String,
    pub mail: Option<String>,
    pub user_principal_name: String,
}

impl Me {
    /// The address of the mailbox: `mail`, else `userPrincipalName`.
    pub fn address(&self) -> &str {
        match &self.mail {
            Some(m) if !m.is_empty() => m,
            _ => &self.user_principal_name,
        }
    }
}

/// One mail folder.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    /// `#microsoft.graph.mailSearchFolder` for a search folder.
    #[serde(rename = "@odata.type", default)]
    pub odata_type: Option<String>,
    pub id: String,
    pub display_name: String,
    pub parent_folder_id: Option<String>,
    pub child_folder_count: u64,
    pub total_item_count: u64,
    pub unread_item_count: u64,
    /// The well-known name, from [`WELL_KNOWN`]. Graph does not send it: the
    /// client sets it.
    #[serde(skip)]
    pub well_known: Option<&'static str>,
}

/// The follow-up flag of a message.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Flag {
    pub flag_status: String,
}

/// The fields of a message in a delta page.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub parent_folder_id: String,
    pub categories: Vec<String>,
    pub is_read: bool,
    pub flag: Flag,
    pub is_draft: bool,
    /// ISO 8601 in UTC, for example `2026-11-17T10:00:00Z`.
    pub received_date_time: Option<String>,
    /// ISO 8601 in UTC: when the item was made, for a draft too.
    #[serde(default)]
    pub created_date_time: Option<String>,
    pub internet_message_id: Option<String>,
    /// The version of the message. A category write sends it in `If-Match`.
    #[serde(rename = "@odata.etag")]
    pub etag: Option<String>,
}

/// A message of the mailbox with a given `Message-ID`, from
/// [`Graph::sent_by_message_id`].
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Copy {
    pub id: String,
    pub is_draft: bool,
    pub sent_date_time: Option<String>,
    pub parent_folder_id: String,
}

#[derive(Deserialize)]
struct CopyPage {
    value: Vec<Copy>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

#[derive(Deserialize)]
struct Tag {
    #[serde(rename = "@odata.etag")]
    etag: String,
}

/// The method of a write, and the one status of its success (spec
/// section 5). Another 2xx is an unknown result.
enum Verb {
    /// `PATCH`, 200.
    Patch,
    /// `POST` that makes an item, 201.
    PostCreated,
    /// `POST /me/sendMail`, 202.
    PostAccepted,
}

impl Message {
    pub fn flagged(&self) -> bool {
        self.flag.flag_status == "flagged"
    }
}

/// One change of a delta page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// The message is in the folder now.
    Message(Message),
    /// The message left the folder: a delete or a move (spec section 3).
    Removed(String),
}

/// The link at the end of a delta page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// Call this `nextLink` for the next page of the round.
    More(String),
    /// The round is done. Store this `deltaLink` for the next sync.
    Done(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeltaPage {
    pub changes: Vec<Change>,
    pub next: Next,
}

#[derive(Deserialize)]
struct DeltaJson {
    value: Vec<serde_json::Value>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    delta_link: Option<String>,
}

#[derive(Deserialize)]
struct FolderPage {
    value: Vec<Folder>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

/// A count of the requests that run now.
struct Gate {
    running: Mutex<usize>,
    free: Condvar,
}

struct GateGuard<'a>(&'a Gate);

impl Gate {
    fn enter(&self) -> GateGuard<'_> {
        let mut n = self.running.lock().unwrap();
        while *n >= MAX_RUNNING {
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

/// A Graph API client. Share one client between threads.
pub struct Graph {
    agent: ureq::Agent,
    config: Config,
    gate: Gate,
}

impl Graph {
    pub fn new(config: Config) -> Graph {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            // A redirect could send the token to another host: a 3xx is an error.
            .max_redirects(0)
            .timeout_global(Some(config.timeout))
            .build()
            .into();
        Graph {
            agent,
            config,
            gate: Gate {
                running: Mutex::new(0),
                free: Condvar::new(),
            },
        }
    }

    /// `GET /me`.
    pub fn me(&self, token: &str) -> Result<Me> {
        let url = self.url("/me?$select=id,mail,userPrincipalName");
        self.json(token, &url, false)
    }

    /// Each mail folder of the mailbox, the child folders at each depth
    /// included, parents before children (spec section 2).
    pub fn folders(&self, token: &str) -> Result<Vec<Folder>> {
        let known = self.well_known(token)?;
        let mut all: Vec<Folder> = Vec::new();
        let mut parents = vec![None::<String>];
        while let Some(parent) = parents.pop() {
            let path = match &parent {
                None => "/me/mailFolders".to_string(),
                Some(id) => format!("/me/mailFolders/{}/childFolders", segment(id)),
            };
            let mut url = self.url(&format!(
                "{path}?includeHiddenFolders=true&$top={PAGE_SIZE}"
            ));
            loop {
                let page: FolderPage = self.json(token, &url, false)?;
                for mut f in page.value {
                    // A search folder is a view of messages in other folders.
                    if f.odata_type.as_deref() == Some("#microsoft.graph.mailSearchFolder") {
                        continue;
                    }
                    if f.child_folder_count > 0 {
                        parents.push(Some(f.id.clone()));
                    }
                    f.well_known = known.iter().find(|(id, _)| *id == f.id).map(|(_, n)| *n);
                    all.push(f);
                }
                match page.next_link {
                    Some(next) => url = self.checked(&next)?,
                    None => break,
                }
            }
        }
        Ok(all)
    }

    /// The ID of each well-known folder of [`WELL_KNOWN`] that the mailbox
    /// has: one call `GET /me/mailFolders/{name}` each.
    pub fn well_known(&self, token: &str) -> Result<Vec<(String, &'static str)>> {
        let mut known = Vec::new();
        for name in WELL_KNOWN {
            match self.json::<Folder>(token, &self.url(&format!("/me/mailFolders/{name}")), false) {
                Ok(f) => known.push((f.id, name)),
                // A mailbox can lack a well-known folder, for example `archive`.
                Err(Error::NotFound) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(known)
    }

    /// The name of each category of the master list of the mailbox:
    /// `GET /me/outlook/masterCategories`.
    pub fn categories(&self, token: &str) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Category {
            display_name: String,
        }
        #[derive(Deserialize)]
        struct Page {
            value: Vec<Category>,
            #[serde(rename = "@odata.nextLink")]
            next_link: Option<String>,
        }
        let mut url = self.url("/me/outlook/masterCategories");
        let mut names = Vec::new();
        loop {
            let page: Page = self.json(token, &url, false)?;
            names.extend(page.value.into_iter().map(|c| c.display_name));
            match page.next_link {
                Some(next) => url = self.checked(&next)?,
                None => return Ok(names),
            }
        }
    }

    /// One page of all mail of all folders, newest first:
    /// `GET /me/messages` with `$orderby=receivedDateTime desc`. With
    /// `link`, call that `nextLink`. Gives (ID, conversation ID) pairs and
    /// the next link.
    pub fn all_mail(&self, token: &str, link: Option<&str>) -> Result<AllMail> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Item {
            id: String,
            conversation_id: String,
        }
        #[derive(Deserialize)]
        struct Page {
            value: Vec<Item>,
            #[serde(rename = "@odata.nextLink")]
            next_link: Option<String>,
        }
        let url = match link {
            Some(l) => self.checked(l)?,
            None => self.url(&format!(
                "/me/messages?$select=id,conversationId&$orderby=receivedDateTime%20desc&$top={PAGE_SIZE}"
            )),
        };
        let page: Page = self.json(token, &url, true)?;
        let next = page.next_link.map(|n| self.checked(&n)).transpose()?;
        let items = page
            .value
            .into_iter()
            .map(|i| (i.id, i.conversation_id))
            .collect();
        Ok((items, next))
    }

    /// One page of the message delta of `folder`. With no `link`, start a new
    /// round. With a link, call the stored `nextLink` or `deltaLink`.
    /// [`Error::DeltaReset`] means: the provider gives `HistoryExpired`, and the sync does a full resync.
    pub fn delta(&self, token: &str, folder: &str, link: Option<&str>) -> Result<DeltaPage> {
        let (url, stored) = match link {
            Some(l) => (self.checked(l)?, true),
            None => (
                self.url(&format!(
                    "/me/mailFolders/{}/messages/delta?$select={SELECT}",
                    segment(folder)
                )),
                false,
            ),
        };
        let page: DeltaJson = match self.json(token, &url, true) {
            Err(Error::NotFound) if stored => return Err(Error::DeltaReset),
            Err(Error::Api { status, .. })
                if stored && (400..500).contains(&status) && status != 403 && status != 429 =>
            {
                return Err(Error::DeltaReset)
            }
            other => other?,
        };
        let changes = page
            .value
            .into_iter()
            .map(change)
            .collect::<Result<Vec<_>>>()?;
        let next = match (page.next_link, page.delta_link) {
            (Some(n), None) => Next::More(self.checked(&n)?),
            (None, Some(d)) => Next::Done(self.checked(&d)?),
            _ => {
                return Err(Error::Decode(
                    "a delta page needs exactly one of nextLink and deltaLink".into(),
                ))
            }
        };
        Ok(DeltaPage { changes, next })
    }

    /// `GET /me/messages/{id}` with the fields of a delta page: the read by
    /// ID of a removed message (spec section 3). [`Error::NotFound`] means
    /// gone.
    pub fn message(&self, token: &str, id: &str) -> Result<Message> {
        let url = self.url(&format!("/me/messages/{}?$select={SELECT}", segment(id)));
        self.json(token, &url, false)
    }

    /// `PATCH isRead`. A write: one request (spec section 5).
    pub fn set_read(&self, token: &str, id: &str, read: bool) -> Result<Message> {
        let m = self.patch(token, id, serde_json::json!({ "isRead": read }), None)?;
        sure(m.is_read == read, m)
    }

    /// `PATCH flag`. A write: one request.
    pub fn set_flag(&self, token: &str, id: &str, flagged: bool) -> Result<Message> {
        let status = if flagged { "flagged" } else { "notFlagged" };
        let body = serde_json::json!({ "flag": { "flagStatus": status } });
        let m = self.patch(token, id, body, None)?;
        sure(m.flag.flag_status == status, m)
    }

    /// `PATCH categories` with the full list and `If-Match: etag`. A write:
    /// one request. HTTP 412 (`Error::Api`) means the message changed after
    /// the read of `etag`, and nothing changed.
    pub fn set_categories(
        &self,
        token: &str,
        id: &str,
        categories: &[String],
        etag: &str,
    ) -> Result<Message> {
        let body = serde_json::json!({ "categories": categories });
        let m = self.patch(token, id, body, Some(etag))?;
        // Graph keeps no order of the categories: compare as sets (spec, A6).
        let set = |l: &[String]| l.iter().cloned().collect::<std::collections::BTreeSet<_>>();
        sure(set(&m.categories) == set(categories), m)
    }

    /// `POST /me/messages/{id}/move` to the folder with the ID `folder`. A
    /// write: one request. The ID of the message stays (immutable IDs).
    pub fn move_to(&self, token: &str, id: &str, folder: &str) -> Result<Message> {
        let url = self.url(&format!("/me/messages/{}/move", segment(id)));
        let body = serde_json::json!({ "destinationId": folder }).to_string();
        let m: Message = accepted(&self.write(
            token,
            Verb::PostCreated,
            &url,
            body.into(),
            "application/json",
            None,
        )?)?;
        sure(m.id == id && m.parent_folder_id == folder, m)
    }

    /// `POST /me/messages` with the MIME text: a new draft. A write: one
    /// request. Keep the `etag` of the answer for the send.
    pub fn create_draft(&self, token: &str, mime: &[u8]) -> Result<Message> {
        let url = self.url("/me/messages");
        let m: Message = accepted(&self.write(
            token,
            Verb::PostCreated,
            &url,
            base64(mime),
            "text/plain",
            None,
        )?)?;
        // The release needs the ID and the etag of the draft (section 5, step 1).
        let tagged = m.etag.as_deref().is_some_and(|e| !e.is_empty());
        sure(m.is_draft && !m.id.is_empty() && tagged, m)
    }

    /// `POST /me/sendMail` with the MIME text: Graph sends these bytes. A
    /// write: one request. The answer (202) has no message ID: find the
    /// sent copy with [`Graph::sent_by_message_id`].
    pub fn send_mime(&self, token: &str, mime: &[u8]) -> Result<()> {
        let url = self.url("/me/sendMail");
        self.write(
            token,
            Verb::PostAccepted,
            &url,
            base64(mime),
            "text/plain",
            None,
        )?;
        Ok(())
    }

    /// Each message of the mailbox (each folder) with the `Message-ID`
    /// `message_id` (angle brackets included), each page. A read. The
    /// caller picks the sent copy (spec section 5, step 5). Drafts are in
    /// the list too.
    pub fn sent_by_message_id(&self, token: &str, message_id: &str) -> Result<Vec<Copy>> {
        let filter = format!("internetMessageId eq '{}'", message_id.replace('\'', "''"));
        let mut url = self.url(&format!(
            "/me/messages?$filter={}&$select=id,isDraft,sentDateTime,parentFolderId",
            segment(&filter)
        ));
        let mut all = Vec::new();
        loop {
            let page: CopyPage = self.json(token, &url, true)?;
            all.extend(page.value);
            match page.next_link {
                Some(next) => url = self.checked(&next)?,
                None => return Ok(all),
            }
        }
    }

    /// The `@odata.etag` of a message now. A read.
    pub fn draft_etag(&self, token: &str, id: &str) -> Result<String> {
        let url = self.url(&format!("/me/messages/{}?$select=id", segment(id)));
        self.json::<Tag>(token, &url, false).map(|t| t.etag)
    }

    fn patch(
        &self,
        token: &str,
        id: &str,
        body: serde_json::Value,
        etag: Option<&str>,
    ) -> Result<Message> {
        let url = self.url(&format!("/me/messages/{}", segment(id)));
        let answer = self.write(
            token,
            Verb::Patch,
            &url,
            body.to_string().into(),
            "application/json",
            etag,
        )?;
        let m: Message = accepted(&answer)?;
        sure(m.id == id, m)
    }

    /// One write request: no second try, also not after 429 or 5xx (spec
    /// section 1). Returns the body of a 2xx answer.
    fn write(
        &self,
        token: &str,
        verb: Verb,
        url: &str,
        body: Vec<u8>,
        content_type: &str,
        if_match: Option<&str>,
    ) -> Result<Vec<u8>> {
        let _running = self.gate.enter();
        let (request, success) = match verb {
            Verb::Patch => (self.agent.patch(url), 200),
            Verb::PostCreated => (self.agent.post(url), 201),
            Verb::PostAccepted => (self.agent.post(url), 202),
        };
        let mut request = request
            .header("Authorization", &format!("Bearer {token}"))
            .header("Prefer", "IdType=\"ImmutableId\"")
            .header("Content-Type", content_type);
        if let Some(e) = if_match {
            request = request.header("If-Match", e);
        }
        let response = request
            .send(&body[..])
            .map_err(|e| Error::Transport(e.to_string()))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(status_error(status, response));
        }
        if status != success {
            return Err(Error::Transport(format!(
                "Graph answered {status} to a write, not {success}"
            )));
        }
        // Graph accepted the write, so a lost answer leaves the result unknown.
        response
            .into_body()
            .with_config()
            .limit(ANSWER_LIMIT)
            .read_to_vec()
            .map_err(|e| Error::Transport(format!("answer of an accepted write: {e}")))
    }

    /// `GET /me/messages/{id}/$value`: the MIME text of the message goes to
    /// `sink` through a buffer of 64 KiB. Returns the count of bytes. If the
    /// call fails after the first byte, the sink has a part: discard it.
    pub fn raw<W: Write>(&self, token: &str, id: &str, sink: &mut W) -> Result<u64> {
        let url = self.url(&format!("/me/messages/{}/$value", segment(id)));
        let _running = self.gate.enter();
        let response = self.call(token, &url, false)?;
        let limit = self.config.raw_limit;
        let mut body = response.into_body().into_reader().take(limit + 1);
        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            let n = match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::Transport(e.to_string())),
            };
            total += n as u64;
            if total > limit {
                return Err(Error::Decode(format!("a message above {limit} bytes")));
            }
            sink.write_all(&buf[..n]).map_err(Error::Sink)?;
        }
        Ok(total)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.config.base_url)
    }

    /// A link from an answer, only if its scheme and authority equal those
    /// of the base URL and its path is below the base path. A text prefix
    /// is not enough: `https://graph.microsoft.com@evil.example/` has one.
    /// A `.` or `..` segment (also as `%2e`) could leave the base path.
    fn checked(&self, link: &str) -> Result<String> {
        use ureq::http::Uri;
        let bad = || Error::Decode("a link to another host".into());
        let base: Uri = self.config.base_url.parse().map_err(|_| bad())?;
        let uri: Uri = link.parse().map_err(|_| bad())?;
        let (Some(a), Some(b)) = (uri.authority(), base.authority()) else {
            return Err(bad());
        };
        let below = uri.path().strip_prefix(base.path()).is_some_and(|rest| {
            rest.starts_with('/')
                && !rest.split('/').any(|seg| {
                    let seg = seg.to_ascii_lowercase().replace("%2e", ".");
                    seg == "." || seg == ".."
                })
        });
        if uri.scheme() == base.scheme() && a.as_str() == b.as_str() && below {
            Ok(link.to_string())
        } else {
            Err(bad())
        }
    }

    fn json<T: DeserializeOwned>(&self, token: &str, url: &str, page: bool) -> Result<T> {
        let _running = self.gate.enter();
        let response = self.call(token, url, page)?;
        let body = response
            .into_body()
            .with_config()
            .limit(ANSWER_LIMIT)
            .read_to_vec()
            .map_err(|e| match e {
                ureq::Error::BodyExceedsLimit(_) => Error::Decode("an answer above 16 MiB".into()),
                e => Error::Transport(e.to_string()),
            })?;
        serde_json::from_slice(&body).map_err(|e| Error::Decode(e.to_string()))
    }

    /// One GET, with the backoff. Returns only a 2xx answer. `page` asks
    /// for [`PAGE_SIZE`] items in a page.
    fn call(&self, token: &str, url: &str, page: bool) -> Result<ureq::http::Response<ureq::Body>> {
        let auth = format!("Bearer {token}");
        let prefer = if page {
            format!("IdType=\"ImmutableId\", odata.maxpagesize={PAGE_SIZE}")
        } else {
            "IdType=\"ImmutableId\"".to_string()
        };
        let deadline = Instant::now() + self.config.timeout;
        let late = || Error::Transport("no answer before the time limit of the call".into());
        let mut wait = self.config.backoff_start;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(late());
            }
            let response = self
                .agent
                .get(url)
                .header("Authorization", &auth)
                .header("Prefer", &prefer)
                .config()
                .timeout_global(Some(left))
                .build()
                .call()
                .map_err(|e| Error::Transport(e.to_string()))?;
            let status = response.status().as_u16();
            if (200..300).contains(&status) {
                return Ok(response);
            }
            if status == 429 || status >= 500 {
                // Graph's own Retry-After is kept; the cap is for our waits.
                let after = retry_after(&response)
                    .unwrap_or_else(|| jitter(wait).min(self.config.backoff_max));
                // No Instant addition: a huge Retry-After must not overflow.
                if after >= deadline.saturating_duration_since(Instant::now()) {
                    return Err(late());
                }
                std::thread::sleep(after);
                wait = (wait * 2).min(self.config.backoff_max);
                continue;
            }
            return Err(status_error(status, response));
        }
    }
}

/// The error of a status that is not 2xx and not retried.
fn status_error(status: u16, response: ureq::http::Response<ureq::Body>) -> Error {
    match status {
        401 => Error::Unauthorized,
        404 => Error::NotFound,
        _ => {
            let (code, message) = error_of(response);
            Error::Api {
                status,
                code,
                message,
            }
        }
    }
}

/// The answer of an accepted write. A bad answer leaves the result
/// unknown, so it is [`Error::Transport`], not [`Error::Decode`].
fn accepted<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    serde_json::from_slice(body)
        .map_err(|e| Error::Transport(format!("bad answer of an accepted write: {e}")))
}

/// The message if Graph answered with the asked state, else an unknown
/// result: the caller reads the message again (spec, "External writes").
fn sure(ok: bool, m: Message) -> Result<Message> {
    if ok {
        Ok(m)
    } else {
        Err(Error::Transport(format!(
            "Graph accepted the write of {} but answered with another state",
            m.id
        )))
    }
}

fn base64(raw: &[u8]) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .encode(raw)
        .into_bytes()
}

/// One item of a delta page. An item with `@removed` must have a reason
/// of Graph (`deleted` or `changed`) and an ID; any other shape is
/// [`Error::Decode`], so a wrong answer never removes a message.
fn change(mut item: serde_json::Value) -> Result<Change> {
    let bad = |e: String| Error::Decode(format!("a delta item: {e}"));
    let obj = item
        .as_object_mut()
        .ok_or_else(|| bad("not an object".into()))?;
    let Some(removed) = obj.remove("@removed") else {
        return serde_json::from_value(item)
            .map(Change::Message)
            .map_err(|e| bad(e.to_string()));
    };
    if !matches!(removed["reason"].as_str(), Some("deleted" | "changed")) {
        return Err(bad("@removed has no known reason".into()));
    }
    match obj.get("id").and_then(|v| v.as_str()) {
        Some(id) if !id.is_empty() => Ok(Change::Removed(id.to_string())),
        _ => Err(bad("a removed item has no id".into())),
    }
}

/// The seconds of a `Retry-After` header. Graph sends seconds (spec, A5).
fn retry_after(response: &ureq::http::Response<ureq::Body>) -> Option<Duration> {
    let v = response.headers().get("Retry-After")?.to_str().ok()?;
    v.trim().parse::<u64>().ok().map(Duration::from_secs)
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

/// The code and the message of a Graph error body (spec, A8).
fn error_of(response: ureq::http::Response<ureq::Body>) -> (String, String) {
    let body = response
        .into_body()
        .with_config()
        .limit(64 * 1024)
        .read_to_vec()
        .unwrap_or_default();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let e = &v["error"];
    (
        e["code"].as_str().unwrap_or("").to_string(),
        e["message"].as_str().unwrap_or("").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_must_start_with_the_base_url() {
        let g = Graph::new(Config {
            base_url: "http://127.0.0.1:9/v1.0".into(),
            ..Config::default()
        });
        assert!(g
            .checked("http://127.0.0.1:9/v1.0/me/x?$deltatoken=1")
            .is_ok());
        for bad in [
            "http://127.0.0.1:9/v1.0.evil.example/me",
            "http://127.0.0.1:99/v1.0/me",
            "https://evil.example/v1.0/me",
            "http://127.0.0.1:9/v1.0",
            "http://127.0.0.1:9@evil.example/v1.0/me",
            "http://user@127.0.0.1:9/v1.0/me",
            "https://127.0.0.1:9/v1.0/me",
            "http://127.0.0.1:9/v1.0x/me",
        ] {
            assert!(matches!(g.checked(bad), Err(Error::Decode(_))), "{bad}");
        }
    }

    /// A small random number source for the property test.
    pub(crate) struct Rng(pub(crate) u64);

    impl Rng {
        pub(crate) fn below(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) % n as u64) as usize
        }

        pub(crate) fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
            from[self.below(from.len())]
        }
    }

    /// Property: a link passes only if its scheme, user part, host, port
    /// and base path are those of the base URL, with no dot segment.
    #[test]
    fn only_links_of_the_base_origin_and_path_pass() {
        let g = Graph::new(Config {
            base_url: "http://127.0.0.1:9/v1.0".into(),
            ..Config::default()
        });
        let schemes = ["http", "https", "ftp", "HTTP", ""];
        let users = ["", "u@", "u:p@", "127.0.0.1:9@", "@"];
        let hosts = [
            "127.0.0.1",
            "evil.example",
            "127.0.0.1.evil",
            "127.0.0.2",
            "[::1]",
        ];
        let ports = [":9", ":99", ":09", "", ":"];
        let paths = [
            "/v1.0", "/v1.0/", "/v1.0x", "/v1", "/", "", "/V1.0", "/x/v1.0",
        ];
        let rests = [
            "",
            "me",
            "me/x?$deltatoken=1",
            "..",
            ".",
            "%2e%2E",
            "a/../b",
            "a/./b",
            "a@b",
            "#f",
            "?q=/v1.0",
            "%2F..",
            "a.b",
            "...",
        ];
        let mut rng = Rng(7);
        let mut passed = 0;
        for _ in 0..20_000 {
            let (scheme, user, host, port, path, rest) = (
                rng.pick(&schemes),
                rng.pick(&users),
                rng.pick(&hosts),
                rng.pick(&ports),
                rng.pick(&paths),
                rng.pick(&rests),
            );
            let link = format!("{scheme}://{user}{host}{port}{path}{rest}");
            let dots = rest
                .split(['/', '?', '#'])
                .any(|s| matches!(s.to_ascii_lowercase().as_str(), "." | ".." | "%2e%2e"));
            // The scheme has no case (RFC 3986 section 3.1).
            let expect = scheme.eq_ignore_ascii_case("http")
                && user.is_empty()
                && host == "127.0.0.1"
                && port == ":9"
                && path == "/v1.0/"
                && !dots;
            let got = g.checked(&link);
            assert_eq!(got.is_ok(), expect, "{link}");
            if let Ok(l) = got {
                assert_eq!(l, link);
                passed += 1;
            }
        }
        assert!(passed > 0, "no generated link passed");
    }

    /// Property: a delta item is a removal only with a known reason and an
    /// ID, a message only with each required field, and else `Decode`.
    #[test]
    fn delta_items_of_another_shape_are_refused() {
        use serde_json::{json, Value};
        let removed = [
            None,
            Some(Value::Null),
            Some(json!({})),
            Some(json!({"reason": "deleted"})),
            Some(json!({"reason": "changed"})),
            Some(json!({"reason": "x"})),
            Some(json!("deleted")),
            Some(json!(1)),
        ];
        let ids = [None, Some(json!("")), Some(json!("AAMk1")), Some(json!(5))];
        let fields = [
            ("conversationId", json!("c")),
            ("parentFolderId", json!("f")),
            ("categories", json!(["a"])),
            ("isRead", json!(true)),
            ("flag", json!({"flagStatus": "flagged"})),
            ("isDraft", json!(false)),
        ];
        let mut rng = Rng(11);
        for _ in 0..5_000 {
            let r = removed[rng.below(removed.len())].clone();
            let id = ids[rng.below(ids.len())].clone();
            // Leave out one field, or none (index == len).
            let gone = rng.below(fields.len() + 1);
            let mut item = serde_json::Map::new();
            if let Some(r) = &r {
                item.insert("@removed".into(), r.clone());
            }
            if let Some(id) = &id {
                item.insert("id".into(), id.clone());
            }
            for (i, (k, v)) in fields.iter().enumerate() {
                if i != gone {
                    item.insert((*k).into(), v.clone());
                }
            }
            let got = change(Value::Object(item.clone()));
            let id_str = id.as_ref().and_then(|v| v.as_str());
            match &r {
                None => {
                    let ok = id_str.is_some() && gone == fields.len();
                    assert_eq!(matches!(got, Ok(Change::Message(_))), ok, "{item:?}");
                }
                Some(r) => {
                    let reason = matches!(r["reason"].as_str(), Some("deleted" | "changed"));
                    let ok = reason && id_str.is_some_and(|s| !s.is_empty());
                    assert_eq!(matches!(got, Ok(Change::Removed(_))), ok, "{item:?}");
                }
            }
            if got.is_err() {
                assert!(matches!(got, Err(Error::Decode(_))));
            }
        }
        assert!(matches!(change(json!([])), Err(Error::Decode(_))));
    }

    #[test]
    fn segments_escape_path_characters() {
        assert_eq!(segment("AAMk-_."), "AAMk-_.");
        assert_eq!(segment("a/b=c+"), "a%2Fb%3Dc%2B");
    }

    #[test]
    fn the_address_falls_back_to_the_principal_name() {
        let me = |mail: Option<&str>| Me {
            id: "1".into(),
            mail: mail.map(String::from),
            user_principal_name: "u@example.com".into(),
        };
        assert_eq!(me(Some("m@example.com")).address(), "m@example.com");
        assert_eq!(me(Some("")).address(), "u@example.com");
        assert_eq!(me(None).address(), "u@example.com");
    }
}
