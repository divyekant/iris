//! The mail provider of an Outlook account: [`Outlook`] maps Microsoft Graph
//! to the label model of the mirror (spec
//! `docs/specs/2026-11-17-outlook-provider.md`, sections 3 and 6, and spec
//! `docs/specs/2026-11-17-provider.md`).
//!
//! This part gives the calls of the sync, the verify and the state writes
//! (spec section 5.1). The draft calls and the send give a definite
//! rejection before any request until card R6-17 adds them.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::sync::Mutex;

use iris_provider::{
    Change as Record, Draft, DraftRaw, History, HistoryMessage, HistoryPage, Label, MessagePage,
    MessageRef, Metadata, Minimal, Profile, RawMessage, Sent,
};
use serde::{Deserialize, Serialize};
use unicase::UniCase;

use crate::{Change, Error, Folder, Graph, Message, Next};

/// The system label names of the mirror (spec 1 section 4.10). A category
/// with such a name gets the prefix `Categories/`.
const SYSTEM_NAMES: [&str; 9] = [
    "INBOX",
    "SPAM",
    "TRASH",
    "UNREAD",
    "STARRED",
    "IMPORTANT",
    "SENT",
    "DRAFT",
    "CHAT",
];

/// The cursor of the sync: the delta link of each folder whose round ended.
/// A folder with no link gets a new round, which lists it as it is now.
#[derive(Default, Serialize, Deserialize)]
struct Cursor {
    v: u8,
    links: BTreeMap<String, String>,
}

/// The page token of [`iris_provider::Provider::changes`] and of
/// [`iris_provider::Provider::messages`]: the folders of the call, the folder
/// of the next page, its `nextLink`, and the delta links of the folders whose
/// round ended.
#[derive(Default, Serialize, Deserialize)]
struct Page {
    ids: Vec<String>,
    i: usize,
    next: Option<String>,
    #[serde(default)]
    done: BTreeMap<String, String>,
}

/// The Outlook provider. One value serves one mailbox: it keeps the IDs of
/// the well-known folders after the first read.
pub struct Outlook {
    graph: Graph,
    known: Mutex<HashMap<String, &'static str>>,
}

impl Outlook {
    pub fn new(graph: Graph) -> Outlook {
        Outlook {
            graph,
            known: Mutex::new(HashMap::new()),
        }
    }

    /// The well-known name of each folder ID, from the cache or from Graph.
    fn known(&self, token: &str) -> Result<HashMap<String, &'static str>, Error> {
        let mut k = self.known.lock().unwrap();
        if k.is_empty() {
            *k = self.graph.well_known(token)?.into_iter().collect();
        }
        Ok(k.clone())
    }

    /// The folders of the mailbox. Also refreshes the cache of well-known IDs.
    fn folders(&self, token: &str) -> Result<Vec<Folder>, Error> {
        let folders = self.graph.folders(token)?;
        *self.known.lock().unwrap() = folders
            .iter()
            .filter_map(|f| Some((f.id.clone(), f.well_known?)))
            .collect();
        Ok(folders)
    }

    fn id_of(&self, token: &str, name: &str) -> Result<Option<String>, Error> {
        Ok(self
            .known(token)?
            .into_iter()
            .find(|(_, n)| *n == name)
            .map(|(id, _)| id))
    }

    /// The folders that a list of `label` reads: one folder for a folder
    /// label, else all folders.
    fn list_folders(&self, token: &str, label: Option<&str>) -> Result<Vec<String>, Error> {
        let one = match label {
            Some("INBOX") => self.id_of(token, "inbox")?,
            Some("TRASH") => self.id_of(token, "deleteditems")?,
            Some("SPAM") => self.id_of(token, "junkemail")?,
            Some("DRAFT") => self.id_of(token, "drafts")?,
            Some(l) => match l.strip_prefix("folder:") {
                Some(id) => Some(id.to_string()),
                None => return Ok(self.folders(token)?.into_iter().map(|f| f.id).collect()),
            },
            None => return Ok(self.folders(token)?.into_iter().map(|f| f.id).collect()),
        };
        Ok(one.into_iter().collect())
    }
}

impl Outlook {
    /// The writes `ops` of message `id`, in order. `etag` is the etag of
    /// the read of the plan. `wrote` becomes true at the first write that
    /// Graph confirms.
    fn write_ops(
        &self,
        token: &str,
        id: &str,
        mut etag: Option<String>,
        ops: Vec<Op>,
        wrote: &mut bool,
    ) -> Result<(), Error> {
        for op in ops {
            let tag = || {
                etag.clone()
                    .ok_or_else(|| Error::Decode(format!("message {id} has no etag")))
            };
            let m = match &op {
                Op::Categories(list) => {
                    tag().and_then(|e| self.graph.set_categories(token, id, list, &e))
                }
                Op::Read(read) => {
                    tag().and_then(|e| self.graph.set_read(token, id, *read, Some(&e)))
                }
                Op::Flag(flagged) => {
                    tag().and_then(|e| self.graph.set_flag(token, id, *flagged, Some(&e)))
                }
                Op::Move(folder) => tag().and_then(|e| self.graph.move_to(token, id, folder, &e)),
            }?;
            *wrote = true;
            etag = m.etag;
        }
        Ok(())
    }

    /// Move `id` to the well-known folder `to`, only from one of `from`,
    /// with `If-Match` on the etag of the check read. One write.
    fn move_between(
        &self,
        token: &str,
        id: &str,
        from: &[&str],
        to: &str,
    ) -> iris_provider::Result<Minimal> {
        let known = self.known(token).map_err(map)?;
        let m = self.graph.message(token, id).map_err(map)?;
        let here = known.get(&m.parent_folder_id).copied();
        if !here.is_some_and(|h| from.contains(&h)) {
            return refuse(format!(
                "Outlook moves this message to {to} only from {}",
                from.join(" or ")
            ));
        }
        let Some(to_id) = folder_of(&known, to) else {
            return refuse(format!("the mailbox has no {to} folder"));
        };
        let etag = m
            .etag
            .ok_or_else(|| iris_provider::Error::Decode(format!("message {id} has no etag")))?;
        let moved = self.graph.move_to(token, id, &to_id, &etag).map_err(map)?;
        Ok(Minimal {
            id: moved.id.clone(),
            label_ids: label_ids(&moved, &known),
        })
    }
}

/// The mirror labels of a message (spec section 6): one folder label, then
/// `UNREAD`, `STARRED` and one label for each category.
pub fn label_ids(m: &Message, known: &HashMap<String, &'static str>) -> Vec<String> {
    let folder = match known.get(&m.parent_folder_id).copied() {
        Some("inbox") => "INBOX".to_string(),
        Some("deleteditems") => "TRASH".to_string(),
        Some("junkemail") => "SPAM".to_string(),
        // A received message that the person moved into Drafts is not a draft.
        Some("drafts") if m.is_draft => "DRAFT".to_string(),
        _ => format!("folder:{}", m.parent_folder_id),
    };
    let mut ids = vec![folder];
    if !m.is_read {
        ids.push("UNREAD".into());
    }
    if m.flagged() {
        ids.push("STARRED".into());
    }
    for c in &m.categories {
        let id = format!("category:{c}");
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// Each label of the mailbox, with the names of spec section 6.
pub fn label_list(folders: &[Folder], categories: &[String]) -> Vec<Label> {
    let label = |id: String, name: String, system: bool| Label {
        id,
        name,
        kind: if system { "system" } else { "user" }.into(),
        messages_total: None,
        messages_unread: None,
        threads_total: None,
        threads_unread: None,
    };
    let mut out: Vec<Label> = ["INBOX", "UNREAD", "STARRED", "DRAFT", "SPAM", "TRASH"]
        .iter()
        .map(|n| label(n.to_string(), n.to_string(), true))
        .collect();

    let by_id: HashMap<&str, &Folder> = folders.iter().map(|f| (f.id.as_str(), f)).collect();
    let mut named: Vec<Label> = Vec::new();
    for f in folders {
        let fixed = match f.well_known {
            Some("inbox" | "deleteditems" | "junkemail") => continue,
            Some("drafts") => Some("Folders/Drafts"),
            Some("archive") => Some("Folders/Archive"),
            Some("outbox") => Some("Folders/Outbox"),
            Some("sentitems") => Some("Folders/Sent Items"),
            _ => None,
        };
        let name = match fixed {
            Some(n) => n.to_string(),
            None => {
                // The path from the top folder. The depth limit stops a loop
                // of parents in a bad answer.
                let mut parts = vec![escape(&f.display_name)];
                let mut at = f.parent_folder_id.as_deref();
                while let Some(p) = at.and_then(|id| by_id.get(id)) {
                    if parts.len() > folders.len() {
                        break;
                    }
                    parts.push(escape(&p.display_name));
                    at = p.parent_folder_id.as_deref();
                }
                parts.reverse();
                format!("Folders/{}", parts.join("/"))
            }
        };
        named.push(label(format!("folder:{}", f.id), name, fixed.is_some()));
    }
    // Folder labels equal under case folding get the folder ID at the end.
    let names: Vec<UniCase<String>> = named.iter().map(|l| UniCase::new(l.name.clone())).collect();
    for (l, folded) in named.iter_mut().zip(&names) {
        if names.iter().filter(|n| *n == folded).count() > 1 {
            l.name = format!("{} #{}", l.name, &l.id["folder:".len()..]);
        }
    }
    out.extend(named);

    let mut cats: Vec<&String> = categories.iter().collect();
    cats.sort();
    cats.dedup();
    for c in &cats {
        let folded = UniCase::new(c.as_str());
        let twin = cats
            .iter()
            .filter(|o| UniCase::new(o.as_str()) == folded)
            .count()
            > 1;
        let prefix = |p: &str| {
            c.get(..p.len())
                .is_some_and(|h| UniCase::new(h) == UniCase::new(p))
        };
        let system = SYSTEM_NAMES.iter().any(|s| UniCase::new(*s) == folded) || prefix("CATEGORY_");
        let name = if twin {
            let hex: String = c.bytes().map(|b| format!("{b:02x}")).collect();
            format!("Categories/{c} #{hex}")
        } else if system || prefix("Folders/") || prefix("Categories/") {
            format!("Categories/{c}")
        } else {
            c.to_string()
        };
        out.push(label(format!("category:{c}"), name, false));
    }
    out
}

/// A display name in a label path: `%`, `/` and `#` are escaped, so each
/// `/` of a label is a level and no name has a `#`.
fn escape(name: &str) -> String {
    name.replace('%', "%25")
        .replace('/', "%2F")
        .replace('#', "%23")
}

/// Milliseconds since 1970 of an RFC 3339 time, as Graph gives it.
fn epoch_ms(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.timestamp_millis())
}

/// The provider error of a Graph error (provider spec section 5).
fn map(e: Error) -> iris_provider::Error {
    use iris_provider::Error as P;
    match e {
        Error::Unauthorized => P::Unauthorized,
        Error::NotFound => P::NotFound,
        Error::DeltaReset => P::HistoryExpired,
        Error::Api {
            status,
            code,
            message,
        } => P::Api {
            status,
            reason: code,
            message,
        },
        Error::Transport(t) => P::Transport(t),
        Error::Decode(d) => P::Decode(d),
        Error::Sink(s) => P::Sink(s),
    }
}

/// One write of [`iris_provider::Provider::modify`] for one message.
#[derive(Debug, PartialEq, Eq)]
enum Op {
    /// The full new list of categories.
    Categories(Vec<String>),
    Read(bool),
    Flag(bool),
    /// The ID of the folder to move to.
    Move(String),
}

/// A definite rejection before any write (`Api` 400).
fn refuse<T>(why: String) -> iris_provider::Result<T> {
    Err(iris_provider::Error::Api {
        status: 400,
        reason: "refused".into(),
        message: why,
    })
}

/// The ID of the well-known folder `name`.
fn folder_of(known: &HashMap<String, &'static str>, name: &str) -> Option<String> {
    known
        .iter()
        .find(|(_, n)| **n == name)
        .map(|(id, _)| id.clone())
}

/// The writes that change the labels of `m` by `add` and `remove`, in the
/// order of the call. A label that Outlook cannot change this way, and a
/// move out of a folder other than the inbox or the archive, give a
/// definite rejection (spec section 6).
fn plan(
    m: &Message,
    known: &HashMap<String, &'static str>,
    add: &[&str],
    remove: &[&str],
) -> iris_provider::Result<Vec<Op>> {
    if let Some(l) = add.iter().find(|l| remove.contains(l)) {
        return refuse(format!("{l} is added and removed in one call"));
    }
    let mut categories = m.categories.clone();
    let (mut read, mut flag, mut target) = (None, None, None);
    for (label, adding) in add
        .iter()
        .map(|l| (*l, true))
        .chain(remove.iter().map(|l| (*l, false)))
    {
        match label {
            "UNREAD" => read = Some(!adding),
            "STARRED" => flag = Some(adding),
            "INBOX" => target = Some(if adding { "inbox" } else { "archive" }),
            l => match l.strip_prefix("category:") {
                Some(name) if !name.is_empty() => {
                    categories.retain(|c| c != name);
                    if adding {
                        categories.push(name.to_string());
                    }
                }
                _ => {
                    return refuse(format!(
                        "Outlook cannot change {l} as a label: a folder moves only with archive, inbox and trash"
                    ))
                }
            },
        }
    }
    let mut ops = Vec::new();
    let same = |a: &[String], b: &[String]| a.len() == b.len() && a.iter().all(|x| b.contains(x));
    if !same(&categories, &m.categories) {
        ops.push(Op::Categories(categories));
    }
    if let Some(r) = read.filter(|r| *r != m.is_read) {
        ops.push(Op::Read(r));
    }
    if let Some(f) = flag.filter(|f| *f != m.flagged()) {
        ops.push(Op::Flag(f));
    }
    if let Some(to) = target {
        let from = known.get(&m.parent_folder_id).copied();
        if !matches!(from, Some("inbox" | "archive")) {
            return refuse("Outlook moves a message only between the inbox and the archive".into());
        }
        let Some(to_id) = folder_of(known, to) else {
            return refuse(format!("the mailbox has no {to} folder"));
        };
        if to_id != m.parent_folder_id {
            ops.push(Op::Move(to_id));
        }
    }
    Ok(ops)
}

/// The definite rejection of a call that card R6-17 adds. No request goes
/// out.
fn later<T>() -> iris_provider::Result<T> {
    Err(iris_provider::Error::Api {
        status: 400,
        reason: "notSupported".into(),
        message: "the Outlook provider does not make this call yet".into(),
    })
}

fn history(m: &Message, known: &HashMap<String, &'static str>) -> HistoryMessage {
    HistoryMessage {
        id: m.id.clone(),
        thread_id: m.conversation_id.clone(),
        label_ids: label_ids(m, known),
    }
}

fn token_of<T: serde::de::DeserializeOwned>(text: &str, what: &str) -> iris_provider::Result<T> {
    serde_json::from_str(text).map_err(|e| iris_provider::Error::Decode(format!("{what}: {e}")))
}

fn text_of<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("the JSON of a cursor")
}

impl iris_provider::Provider for Outlook {
    /// The cursor of now is the empty cursor: Graph has no cheap cursor of
    /// now, so the next `changes` starts a new round of each folder.
    fn profile(&self, token: &str) -> iris_provider::Result<Profile> {
        let me = self.graph.me(token).map_err(map)?;
        Ok(Profile {
            email_address: me.address().to_string(),
            messages_total: 0,
            threads_total: 0,
            cursor: text_of(&Cursor {
                v: 1,
                links: BTreeMap::new(),
            }),
        })
    }

    fn labels(&self, token: &str) -> iris_provider::Result<Vec<Label>> {
        let folders = self.folders(token).map_err(map)?;
        let categories = self.graph.categories(token).map_err(map)?;
        Ok(label_list(&folders, &categories))
    }

    /// With no `label`: one page of all mail, newest first. With `label`:
    /// one delta page of one folder with no stored link (the folder as it
    /// is now), with only the messages that have the label.
    fn messages(
        &self,
        token: &str,
        label: Option<&str>,
        page: Option<&str>,
    ) -> iris_provider::Result<MessagePage> {
        if label.is_none() {
            // All mail: newest first across folders (spec 1 section 4.2).
            // The page token is the `nextLink`.
            let (items, next) = self.graph.all_mail(token, page).map_err(map)?;
            return Ok(MessagePage {
                messages: items
                    .into_iter()
                    .map(|(id, thread_id)| MessageRef { id, thread_id })
                    .collect(),
                next_page_token: next,
            });
        }
        let mut at: Page = match page {
            Some(p) => token_of(p, "page token")?,
            None => Page {
                ids: self.list_folders(token, label).map_err(map)?,
                ..Page::default()
            },
        };
        let Some(folder) = at.ids.get(at.i).cloned() else {
            return Ok(MessagePage {
                messages: Vec::new(),
                next_page_token: None,
            });
        };
        let known = self.known(token).map_err(map)?;
        let delta = self
            .graph
            .delta(token, &folder, at.next.as_deref())
            .map_err(map)?;
        let messages = delta
            .changes
            .iter()
            .filter_map(|c| match c {
                Change::Message(m) => Some(m),
                Change::Removed(_) => None,
            })
            .filter(|m| label.is_none_or(|l| label_ids(m, &known).iter().any(|x| x == l)))
            .map(|m| MessageRef {
                id: m.id.clone(),
                thread_id: m.conversation_id.clone(),
            })
            .collect();
        match delta.next {
            Next::More(l) => at.next = Some(l),
            Next::Done(_) => {
                at.next = None;
                at.i += 1;
            }
        }
        Ok(MessagePage {
            messages,
            next_page_token: (at.i < at.ids.len()).then(|| text_of(&at)),
        })
    }

    /// One delta page of one folder (spec section 3). A removed message is
    /// read by ID: a message gives its labels now (a move), HTTP 404 gives
    /// `Deleted`. A folder of the cursor that the mailbox lost, and a link
    /// that Graph does not accept, give `HistoryExpired`: the sync then does
    /// a full resync.
    fn changes(
        &self,
        token: &str,
        from: &str,
        page: Option<&str>,
    ) -> iris_provider::Result<HistoryPage> {
        let cursor: Cursor = token_of(from, "cursor")?;
        if cursor.v != 1 {
            return Err(iris_provider::Error::Decode(
                "an unknown cursor version".into(),
            ));
        }
        let mut at: Page = match page {
            Some(p) => token_of(p, "page token")?,
            None => {
                let ids: Vec<String> = self
                    .folders(token)
                    .map_err(map)?
                    .into_iter()
                    .map(|f| f.id)
                    .collect();
                if cursor.links.keys().any(|k| !ids.contains(k)) {
                    return Err(iris_provider::Error::HistoryExpired);
                }
                Page {
                    ids,
                    ..Page::default()
                }
            }
        };
        let known = self.known(token).map_err(map)?;
        let mut changes = Vec::new();
        if let Some(folder) = at.ids.get(at.i).cloned() {
            let link = at
                .next
                .clone()
                .or_else(|| cursor.links.get(&folder).cloned());
            let delta = match self.graph.delta(token, &folder, link.as_deref()) {
                Ok(d) => d,
                // The folder went away during the sync.
                Err(Error::NotFound) => return Err(iris_provider::Error::HistoryExpired),
                Err(e) => return Err(map(e)),
            };
            for c in delta.changes {
                changes.push(match c {
                    Change::Message(m) => Record::Added(history(&m, &known)),
                    Change::Removed(id) => match self.graph.message(token, &id) {
                        Ok(m) => Record::Added(history(&m, &known)),
                        Err(Error::NotFound) => Record::Deleted(HistoryMessage {
                            id,
                            thread_id: String::new(),
                            label_ids: Vec::new(),
                        }),
                        Err(e) => return Err(map(e)),
                    },
                });
            }
            match delta.next {
                Next::More(l) => at.next = Some(l),
                Next::Done(l) => {
                    at.done.insert(folder, l);
                    at.next = None;
                    at.i += 1;
                }
            }
        }
        let last = at.i >= at.ids.len();
        Ok(HistoryPage {
            history: vec![History { changes }],
            cursor: if last {
                text_of(&Cursor {
                    v: 1,
                    links: std::mem::take(&mut at.done),
                })
            } else {
                from.to_string()
            },
            next_page_token: (!last).then(|| text_of(&at)),
        })
    }

    fn minimal(&self, token: &str, id: &str) -> iris_provider::Result<Minimal> {
        let m = self.graph.message(token, id).map_err(map)?;
        let known = self.known(token).map_err(map)?;
        Ok(Minimal {
            id: m.id.clone(),
            label_ids: label_ids(&m, &known),
        })
    }

    fn metadata(&self, _: &str, _: &str, _: &[&str]) -> iris_provider::Result<Metadata> {
        later()
    }

    /// The fields first, then the MIME text into `sink`.
    fn raw(
        &self,
        token: &str,
        id: &str,
        sink: &mut dyn Write,
    ) -> iris_provider::Result<RawMessage> {
        let m = self.graph.message(token, id).map_err(map)?;
        let known = self.known(token).map_err(map)?;
        // A draft can have no received time: then the time it was made.
        let time = m
            .received_date_time
            .as_deref()
            .or(m.created_date_time.as_deref())
            .ok_or_else(|| iris_provider::Error::Decode(format!("message {id} has no time")))?;
        let internal_date = epoch_ms(time)
            .ok_or_else(|| iris_provider::Error::Decode(format!("bad message time {time}")))?;
        let size = self.graph.raw(token, id, &mut &mut *sink).map_err(map)?;
        Ok(RawMessage {
            id: m.id.clone(),
            thread_id: m.conversation_id.clone(),
            label_ids: label_ids(&m, &known),
            internal_date,
            size,
        })
    }

    fn messages_with_id(&self, _: &str, _: &str) -> iris_provider::Result<Vec<MessageRef>> {
        later()
    }

    /// Reads each message first and plans its writes; a refusal comes
    /// before any write (provider spec section 4). Then, for each message:
    /// the categories (`If-Match`), the read state, the flag, and the move
    /// last (`If-Match` on the newest etag). An error after a write of the
    /// call is an unknown result (provider spec section 6).
    fn modify(
        &self,
        token: &str,
        ids: &[&str],
        add: &[&str],
        remove: &[&str],
    ) -> iris_provider::Result<()> {
        if ids.len() > iris_provider::BATCH_LIMIT {
            return refuse(format!("more than {} messages", iris_provider::BATCH_LIMIT));
        }
        let known = self.known(token).map_err(map)?;
        let mut plans = Vec::with_capacity(ids.len());
        for id in ids {
            let m = self.graph.message(token, id).map_err(map)?;
            plans.push((m.etag.clone(), plan(&m, &known, add, remove)?));
        }
        let mut wrote = false;
        for (id, (etag, ops)) in ids.iter().zip(plans) {
            let result = match self.write_ops(token, id, etag, ops, &mut wrote) {
                // HTTP 412: another client changed the message after the
                // read. One new read and plan, so that its change stays
                // (spec section 5, A14 and A17).
                Err(Error::Api { status: 412, .. }) => self
                    .graph
                    .message(token, id)
                    .map_err(map)
                    .and_then(|m| Ok((m.etag.clone(), plan(&m, &known, add, remove)?)))
                    .and_then(|(etag, ops)| {
                        self.write_ops(token, id, etag, ops, &mut wrote)
                            .map_err(map)
                    }),
                other => other.map_err(map),
            };
            if let Err(e) = result {
                return Err(if wrote {
                    iris_provider::Error::Transport(format!("a part of the call ran, then: {e}"))
                } else {
                    e
                });
            }
        }
        Ok(())
    }

    /// A move to Deleted Items, only from the inbox or the archive.
    fn trash(&self, token: &str, id: &str) -> iris_provider::Result<Minimal> {
        self.move_between(token, id, &["inbox", "archive"], "deleteditems")
    }

    /// A move from Deleted Items to the inbox. The apply then archives a
    /// message that had no `INBOX` before the trash.
    fn untrash(&self, token: &str, id: &str) -> iris_provider::Result<Minimal> {
        self.move_between(token, id, &["deleteditems"], "inbox")
    }

    /// Iris makes no category: the master list needs one more permission
    /// (`MailboxSettings.ReadWrite`), and a category that is not on the
    /// list is not mirrored (#656). Make the category in Outlook first.
    fn create_label(&self, _: &str, name: &str) -> iris_provider::Result<Label> {
        refuse(format!(
            "Outlook has no category {name}: make it in Outlook first, then sync"
        ))
    }

    fn drafts_with_id(&self, _: &str, _: &str) -> iris_provider::Result<Vec<Draft>> {
        later()
    }

    fn draft_raw(&self, _: &str, _: &str) -> iris_provider::Result<DraftRaw> {
        later()
    }

    fn create_draft(&self, _: &str, _: &[u8], _: Option<&str>) -> iris_provider::Result<Draft> {
        later()
    }

    fn send_draft(
        &self,
        _: &str,
        _: &str,
        _: &[u8],
        _: Option<&str>,
    ) -> iris_provider::Result<Sent> {
        later()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Rng;
    use crate::Flag;

    fn folder(
        id: &str,
        name: &str,
        parent: Option<&str>,
        well_known: Option<&'static str>,
    ) -> Folder {
        Folder {
            odata_type: None,
            id: id.into(),
            display_name: name.into(),
            parent_folder_id: parent.map(str::to_string),
            child_folder_count: 0,
            total_item_count: 0,
            unread_item_count: 0,
            well_known,
        }
    }

    fn unescape(part: &str) -> String {
        part.replace("%2F", "/")
            .replace("%23", "#")
            .replace("%25", "%")
    }

    /// Property: for random folder trees and category lists (names from
    /// Graph, which the person sets), the label names are distinct under
    /// case folding, no category gets a system name or a `Folders/` name,
    /// and each folder name gives back the display names of its path.
    #[test]
    fn label_names_stay_distinct_for_any_folder_and_category_names() {
        let words = [
            "Inbox",
            "inbox",
            "INBOX",
            "a/b",
            "a%2Fb",
            "#1",
            "%",
            "Folders",
            "Categories/x",
            "folders/y",
            "Travel",
            "travel",
            "Σσ",
            "ς",
            "Drafts",
            "Sent Items",
            "Archive",
            "",
            " x ",
            "CATEGORY_A",
            "Spam",
            "Unread",
        ];
        let known = [
            "inbox",
            "drafts",
            "deleteditems",
            "junkemail",
            "archive",
            "outbox",
            "sentitems",
        ];
        let mut rng = Rng(7);
        for _ in 0..3_000 {
            let mut folders = Vec::new();
            for (i, wk) in known.iter().enumerate() {
                if rng.below(4) > 0 {
                    folders.push(folder(&format!("W{i}"), rng.pick(&words), None, Some(wk)));
                }
            }
            for i in 0..rng.below(8) {
                let parent = (i > 0 && rng.below(2) == 0).then(|| format!("U{}", rng.below(i)));
                let f = folder(&format!("U{i}"), rng.pick(&words), parent.as_deref(), None);
                folders.push(f);
            }
            let cats: Vec<String> = (0..rng.below(6))
                .map(|_| rng.pick(&words).to_string())
                .collect();
            let labels = label_list(&folders, &cats);

            let mut ids: Vec<&str> = labels.iter().map(|l| l.id.as_str()).collect();
            ids.sort();
            let n = ids.len();
            ids.dedup();
            assert_eq!(ids.len(), n, "distinct IDs");
            for (i, a) in labels.iter().enumerate() {
                for b in &labels[i + 1..] {
                    assert_ne!(UniCase::new(&a.name), UniCase::new(&b.name), "{a:?} {b:?}");
                }
                if a.id.starts_with("category:") {
                    let folded = UniCase::new(a.name.as_str());
                    assert!(!SYSTEM_NAMES.iter().any(|s| UniCase::new(*s) == folded));
                    assert!(!a.name.to_lowercase().starts_with("folders/"), "{a:?}");
                    assert_eq!(a.kind, "user");
                }
                if let Some(id) = a.id.strip_prefix("folder:") {
                    let f = folders.iter().find(|f| f.id == id).unwrap();
                    let name = a.name.split(" #").next().unwrap();
                    if f.well_known.is_none() {
                        let last = name.rsplit('/').next().unwrap();
                        assert_eq!(unescape(last), f.display_name, "{a:?}");
                        assert_eq!(a.kind, "user");
                    } else {
                        assert_eq!(a.kind, "system");
                    }
                    assert!(name.starts_with("Folders/"));
                }
            }
        }
    }

    /// Property: each message has exactly one folder label, and `DRAFT`
    /// only when it is in Drafts and Graph says that it is a draft.
    #[test]
    fn each_message_has_one_folder_label() {
        let known: HashMap<String, &'static str> = [
            ("I", "inbox"),
            ("D", "drafts"),
            ("T", "deleteditems"),
            ("J", "junkemail"),
            ("S", "sentitems"),
        ]
        .into_iter()
        .map(|(id, n)| (id.to_string(), n))
        .collect();
        let mut rng = Rng(11);
        for _ in 0..5_000 {
            let m = Message {
                id: "m".into(),
                conversation_id: "c".into(),
                parent_folder_id: rng.pick(&["I", "D", "T", "J", "S", "X", ""]).into(),
                categories: (0..rng.below(4))
                    .map(|_| rng.pick(&["a", "A", "INBOX", "a"]).to_string())
                    .collect(),
                is_read: rng.below(2) == 0,
                flag: Flag {
                    flag_status: rng.pick(&["flagged", "notFlagged", "complete"]).into(),
                },
                is_draft: rng.below(2) == 0,
                received_date_time: None,
                created_date_time: None,
                internet_message_id: None,
                etag: None,
            };
            let ids = label_ids(&m, &known);
            let folders: Vec<&String> = ids
                .iter()
                .filter(|l| {
                    l.starts_with("folder:")
                        || ["INBOX", "TRASH", "SPAM", "DRAFT"].contains(&l.as_str())
                })
                .collect();
            assert_eq!(folders.len(), 1, "{ids:?}");
            assert_eq!(
                ids.contains(&"DRAFT".to_string()),
                m.is_draft && m.parent_folder_id == "D"
            );
            assert!(!ids.contains(&"SENT".to_string()));
            assert_eq!(ids.contains(&"UNREAD".to_string()), !m.is_read);
            assert_eq!(ids.contains(&"STARRED".to_string()), m.flagged());
            let mut sorted = ids.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), ids.len(), "no label twice");
        }
    }

    /// Property: `epoch_ms` reads each time that `format` writes, and gives
    /// `None` for a changed text, never a wrong time or a panic.
    #[test]
    fn received_times_read_back_and_bad_text_is_refused() {
        fn format(ms: i64) -> String {
            let secs = ms.div_euclid(1000);
            let days = secs.div_euclid(86_400);
            let rest = secs.rem_euclid(86_400);
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
                "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
                rest / 3600,
                rest % 3600 / 60,
                rest % 60,
                ms.rem_euclid(1000)
            )
        }
        let mut rng = Rng(3);
        let chars = ["0", "9", "-", ":", "T", "Z", ".", "x", "é", " "];
        for _ in 0..20_000 {
            let ms = (rng.below(1 << 30) as i64) * 4096 + rng.below(4096) as i64;
            let text = format(ms);
            assert_eq!(epoch_ms(&text), Some(ms), "{text}");
            let whole = format!("{}Z", &text[..19]);
            assert_eq!(epoch_ms(&whole), Some(ms - ms % 1000), "{whole}");
            let mut bad = text.clone();
            let at = rng.below(bad.len());
            if bad.is_char_boundary(at) && bad.is_char_boundary(at + 1) {
                bad.replace_range(at..at + 1, rng.pick(&chars));
            }
            if let Some(t) = epoch_ms(&bad) {
                // A changed digit can give another valid time, never a panic.
                assert!(bad != text || t == ms);
            }
        }
        assert_eq!(epoch_ms("2026-11-17T10:00:00Z"), Some(1_794_909_600_000));
        assert_eq!(epoch_ms("2026-13-17T10:00:00Z"), None);
        assert_eq!(epoch_ms("2026-11-17T10:00:00"), None);
        assert_eq!(epoch_ms("2026-11-17T10:00:00.Z"), None);
        assert_eq!(epoch_ms(""), None);
    }

    /// Property: for random messages and random label changes, `plan`
    /// refuses each change that Outlook cannot make, before any write, and
    /// else its writes give exactly the labels of the change.
    #[test]
    fn a_plan_gives_the_labels_of_the_change_or_a_refusal() {
        let known: HashMap<String, &'static str> = [
            ("I", "inbox"),
            ("A", "archive"),
            ("D", "drafts"),
            ("T", "deleteditems"),
            ("J", "junkemail"),
        ]
        .into_iter()
        .map(|(id, n)| (id.to_string(), n))
        .collect();
        let labels = [
            "INBOX",
            "UNREAD",
            "STARRED",
            "category:a",
            "category:B",
            "category:",
            "TRASH",
            "SPAM",
            "DRAFT",
            "SENT",
            "folder:X",
            "folder:A",
            "IMPORTANT",
        ];
        let mut rng = Rng(5);
        for _ in 0..20_000 {
            let m = Message {
                id: "m".into(),
                conversation_id: "c".into(),
                parent_folder_id: rng.pick(&["I", "A", "D", "T", "J", "X"]).into(),
                categories: (0..rng.below(3))
                    .map(|_| rng.pick(&["a", "B", "c"]).to_string())
                    .fold(Vec::new(), |mut v, c| {
                        if !v.contains(&c) {
                            v.push(c);
                        }
                        v
                    }),
                is_read: rng.below(2) == 0,
                flag: Flag {
                    flag_status: rng.pick(&["flagged", "notFlagged"]).into(),
                },
                is_draft: false,
                received_date_time: None,
                created_date_time: None,
                internet_message_id: None,
                etag: Some("e".into()),
            };
            let add: Vec<&str> = (0..rng.below(3)).map(|_| rng.pick(&labels)).collect();
            let remove: Vec<&str> = (0..rng.below(3)).map(|_| rng.pick(&labels)).collect();
            let ok_label = |l: &&str| {
                ["INBOX", "UNREAD", "STARRED"].contains(l)
                    || l.strip_prefix("category:").is_some_and(|n| !n.is_empty())
            };
            let moves = add.contains(&"INBOX") || remove.contains(&"INBOX");
            let from_ok = matches!(m.parent_folder_id.as_str(), "I" | "A");
            let clash = add.iter().any(|l| remove.contains(l));
            let allowed = !clash && add.iter().chain(&remove).all(ok_label) && (!moves || from_ok);
            let plan = plan(&m, &known, &add, &remove);
            assert_eq!(
                plan.is_ok(),
                allowed,
                "{add:?} {remove:?} {:?}",
                m.parent_folder_id
            );
            let Ok(ops) = plan else { continue };
            // Apply the writes to a copy, as Graph does.
            let mut after = m.clone();
            for op in &ops {
                match op {
                    Op::Categories(c) => after.categories = c.clone(),
                    Op::Read(r) => after.is_read = *r,
                    Op::Flag(f) => {
                        after.flag.flag_status = if *f { "flagged" } else { "notFlagged" }.into()
                    }
                    Op::Move(to) => after.parent_folder_id = to.clone(),
                }
            }
            let mut want: Vec<String> = label_ids(&m, &known)
                .into_iter()
                .filter(|l| !remove.contains(&l.as_str()))
                .collect();
            for l in &add {
                if !want.iter().any(|w| w == l) {
                    want.push(l.to_string());
                }
            }
            // A move changes the folder label: INBOX or the archive folder.
            if moves {
                want.retain(|l| l != "INBOX" && l != "folder:A");
                want.push(
                    if add.contains(&"INBOX") {
                        "INBOX"
                    } else {
                        "folder:A"
                    }
                    .into(),
                );
            }
            let mut got = label_ids(&after, &known);
            got.sort();
            want.sort();
            assert_eq!(got, want, "{add:?} {remove:?} {ops:?}");
            // No write that changes nothing.
            let mut seen = Vec::new();
            for op in &ops {
                let kind = std::mem::discriminant(op);
                assert!(!seen.contains(&kind), "one write of each kind: {ops:?}");
                seen.push(kind);
            }
            if ops.is_empty() {
                assert_eq!(label_ids(&m, &known).len(), label_ids(&after, &known).len());
            }
        }
    }
}
