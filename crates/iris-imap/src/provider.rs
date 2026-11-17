//! The IMAP provider (spec `docs/specs/2026-11-17-imap-provider.md`): the
//! client of this crate as an [`iris_provider::Provider`].

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::sync::Mutex;

use iris_provider::{
    Change, Draft, DraftRaw, Error, History, HistoryMessage, HistoryPage, Label, MessagePage,
    MessageRef, Metadata, Minimal, Profile, Provider, RawMessage, Result, Sent,
};

use crate::{Auth, Entry, Folder, Search, Session, Special};

/// The UID width of one page of [`Provider::messages`].
const WINDOW: u32 = 2000;
/// The most UIDs of a search in one page.
const SEARCH_PAGE: usize = 500;
/// The largest `VANISHED` range that the provider turns into deletes. A
/// larger range gives a full resync.
const VANISHED_MAX: u64 = 100_000;

/// An IMAP account as a mail provider. The `token` of each call is the
/// password. The calls run one at a time on one session.
pub struct Imap<S: Read + Write> {
    address: String,
    user: String,
    connect: Box<dyn Fn() -> crate::Result<S> + Send + Sync>,
    session: Mutex<Option<Session<S>>>,
}

impl<S: Read + Write + Send> Imap<S> {
    /// `address` is the mail address of the account, `user` the IMAP user
    /// name, and `connect` makes a new stream.
    pub fn new(
        address: &str,
        user: &str,
        connect: impl Fn() -> crate::Result<S> + Send + Sync + 'static,
    ) -> Imap<S> {
        Imap {
            address: address.to_owned(),
            user: user.to_owned(),
            connect: Box::new(connect),
            session: Mutex::new(None),
        }
    }

    /// Run `f` on the session. A new session signs in with `password`. Any
    /// error drops the session.
    fn with<T>(&self, password: &str, f: impl FnOnce(&mut Session<S>) -> Result<T>) -> Result<T> {
        let mut guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            let stream = (self.connect)().map_err(read_error)?;
            let auth = Auth::Password {
                user: &self.user,
                password,
            };
            *guard = Some(Session::start(stream, auth).map_err(read_error)?);
        }
        let s = guard.as_mut().expect("session");
        let r = f(s);
        if r.is_err() {
            *guard = None;
        }
        r
    }
}

/// The error of a read (spec section 1).
fn read_error(e: crate::Error) -> Error {
    use crate::Error as E;
    match e {
        E::SignIn => Error::Unauthorized,
        E::Io(e) => Error::Transport(e.to_string()),
        E::Closed | E::Tls(_) => Error::Transport(e.to_string()),
        E::Protocol(_) | E::TooLarge => Error::Decode(e.to_string()),
        E::Refused(_) => api("refused", &e.to_string()),
        E::Missing(_) => api("missing", &e.to_string()),
        E::Name => api("badName", &e.to_string()),
    }
}

fn api(reason: &str, message: &str) -> Error {
    Error::Api {
        status: 400,
        reason: reason.to_owned(),
        message: message.to_owned(),
    }
}

fn not_supported() -> Error {
    api("notSupported", "IMAP accounts cannot do this yet")
}

/// The folders that hold mail: selectable, and not a view of other folders.
fn mail_folders<S: Read + Write>(s: &mut Session<S>) -> Result<Vec<Folder>> {
    Ok(s.folders()
        .map_err(read_error)?
        .into_iter()
        .filter(|f| f.selectable && !matches!(f.special, Some(Special::All | Special::Flagged)))
        .collect())
}

/// The label of a folder: (ID, name, kind).
fn folder_label(f: &Folder) -> (String, String, &'static str) {
    let fixed = |id: &str| (id.to_owned(), id.to_owned(), "system");
    if f.name.eq_ignore_ascii_case("INBOX") {
        return fixed("INBOX");
    }
    match f.special {
        Some(Special::Drafts) => fixed("DRAFT"),
        Some(Special::Junk) => fixed("SPAM"),
        Some(Special::Trash) => fixed("TRASH"),
        // `\Sent` too: no IMAP message gets SENT (spec section 4).
        Some(_) => (format!("folder:{}", f.name), f.name.clone(), "system"),
        None => (format!("folder:{}", f.name), f.name.clone(), "user"),
    }
}

/// The labels of a message of folder `f` with `flags`.
fn labels_of(f: &Folder, flags: &[String]) -> Vec<String> {
    let has = |x: &str| flags.iter().any(|g| g.eq_ignore_ascii_case(x));
    let mut out = vec![folder_label(f).0];
    if !has("\\Seen") {
        out.push("UNREAD".into());
    }
    if has("\\Flagged") {
        out.push("STARRED".into());
    }
    out.extend(
        flags
            .iter()
            .filter(|g| !g.starts_with('\\'))
            .map(|g| format!("kw:{g}")),
    );
    out
}

fn message_id(validity: u32, uid: u32, folder: &str) -> String {
    format!("{validity}:{uid}:{folder}")
}

/// (UIDVALIDITY, UID, folder) of a message ID.
fn parse_id(id: &str) -> Option<(u32, u32, &str)> {
    let mut p = id.splitn(3, ':');
    Some((p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?))
}

/// The thread of a message (spec section 3): the first `References` ID,
/// else `In-Reply-To`, else the own `Message-ID`, else `id`.
pub(crate) fn thread_of(headers: Option<&[u8]>, id: &str) -> String {
    let parsed = headers.and_then(|h| mail_parser::MessageParser::default().parse_headers(h));
    parsed
        .and_then(|m| {
            m.references()
                .and_then(|r| r.first().map(str::to_owned))
                .or_else(|| m.in_reply_to().and_then(|r| r.first().map(str::to_owned)))
                .or_else(|| m.message_id().map(str::to_owned))
        })
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| id.to_owned())
}

/// `INTERNALDATE` (`17-Nov-2026 09:30:00 +0000`) in milliseconds.
pub(crate) fn internal_ms(date: &str) -> Option<i64> {
    let text = date.trim().replacen('-', " ", 2);
    let d = mail_parser::DateTime::parse_rfc822(&text)?;
    d.is_valid().then(|| d.to_timestamp() * 1000)
}

fn history_message(f: &Folder, validity: u32, e: &Entry) -> HistoryMessage {
    let id = message_id(validity, e.uid, &f.name);
    HistoryMessage {
        thread_id: thread_of(e.headers.as_deref(), &id),
        label_ids: labels_of(f, &e.flags),
        id,
    }
}

/// One folder of a cursor.
#[derive(Clone, Copy)]
struct Mark {
    validity: u32,
    next: u32,
    modseq: u64,
    count: u32,
}

fn mark_line(name: &str, m: Mark) -> String {
    format!(
        "{} {} {} {} {name}\n",
        m.validity, m.next, m.modseq, m.count
    )
}

/// The folders of a cursor. `None`: the text does not parse.
fn parse_cursor(text: &str) -> Option<BTreeMap<String, Mark>> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let mut p = line.splitn(5, ' ');
        let mut num = || p.next()?.parse::<u64>().ok();
        let (validity, next, modseq, count) = (num()?, num()?, num()?, num()?);
        let name = p.next()?;
        let m = Mark {
            validity: u32::try_from(validity).ok()?,
            next: u32::try_from(next).ok()?,
            modseq,
            count: u32::try_from(count).ok()?,
        };
        out.insert(name.to_owned(), m);
    }
    // A profile always has INBOX, so an empty cursor is not one of ours.
    (!out.is_empty()).then_some(out)
}

fn mark_of(e: &crate::Examined) -> Mark {
    Mark {
        validity: e.uidvalidity,
        next: e.uidnext.unwrap_or(0),
        modseq: e.highest_modseq.unwrap_or(0),
        count: e.exists,
    }
}

/// The changes of one folder after `old`. `None`: a full resync is needed.
fn folder_changes<S: Read + Write>(
    s: &mut Session<S>,
    f: &Folder,
    old: Option<Mark>,
    now: &crate::Examined,
) -> Result<Option<Vec<Change>>> {
    let mut out = Vec::new();
    let Some(old) = old else {
        s.list(1, None, |e| {
            out.push(Change::Added(history_message(f, now.uidvalidity, &e)))
        })
        .map_err(read_error)?;
        return Ok(Some(out));
    };
    if old.validity != now.uidvalidity {
        return Ok(None);
    }
    let modseq = now.highest_modseq.unwrap_or(0);
    if modseq == old.modseq && now.exists == old.count {
        return Ok(Some(out));
    }
    if old.modseq == 0 || modseq == 0 {
        return Ok(None);
    }
    let mut new_uids = BTreeSet::new();
    let mut too_wide = false;
    s.changes(old.modseq, |c| match c {
        crate::Change::Entry(e) => {
            if old.next > 0 && e.uid >= old.next {
                new_uids.insert(e.uid);
            }
            out.push(Change::Added(history_message(f, now.uidvalidity, &e)));
        }
        crate::Change::Vanished(r) => {
            if (*r.end() as u64) - (*r.start() as u64) >= VANISHED_MAX {
                too_wide = true;
                return;
            }
            for uid in r {
                let id = message_id(now.uidvalidity, uid, &f.name);
                out.push(Change::Deleted(HistoryMessage {
                    thread_id: id.clone(),
                    id,
                    label_ids: Vec::new(),
                }));
            }
        }
    })
    .map_err(read_error)?;
    if too_wide {
        return Ok(None);
    }
    if !s.qresync() {
        // Removed = count before - (count now - new messages now) (spec section 5).
        let kept = now.exists as i64 - new_uids.len() as i64;
        if old.next == 0 || kept != old.count as i64 {
            return Ok(None);
        }
    }
    Ok(Some(out))
}

impl<S: Read + Write + Send> Provider for Imap<S> {
    fn profile(&self, token: &str) -> Result<Profile> {
        self.with(token, |s| {
            if !s.condstore() {
                return Err(api("noCondstore", "the IMAP server has no CONDSTORE"));
            }
            let mut cursor = String::new();
            for f in mail_folders(s)? {
                let e = s.examine(&f.name).map_err(read_error)?;
                cursor.push_str(&mark_line(&f.name, mark_of(&e)));
            }
            Ok(Profile {
                email_address: self.address.clone(),
                messages_total: 0,
                threads_total: 0,
                cursor,
            })
        })
    }

    fn labels(&self, token: &str) -> Result<Vec<Label>> {
        self.with(token, |s| {
            let label = |id: &str, name: &str, kind: &str| Label {
                id: id.to_owned(),
                name: name.to_owned(),
                kind: kind.to_owned(),
                messages_total: None,
                messages_unread: None,
                threads_total: None,
                threads_unread: None,
            };
            let mut out: BTreeMap<String, Label> = BTreeMap::new();
            for id in ["UNREAD", "STARRED"] {
                out.insert(id.into(), label(id, id, "system"));
            }
            for f in mail_folders(s)? {
                let (id, name, kind) = folder_label(&f);
                out.insert(id.clone(), label(&id, &name, kind));
                for k in s.examine(&f.name).map_err(read_error)?.flags {
                    if !k.starts_with('\\') {
                        let id = format!("kw:{k}");
                        out.insert(id.clone(), label(&id, &k, "user"));
                    }
                }
            }
            Ok(out.into_values().collect())
        })
    }

    fn messages(
        &self,
        token: &str,
        label: Option<&str>,
        page: Option<&str>,
    ) -> Result<MessagePage> {
        self.with(token, |s| {
            let folders: Vec<Folder> = mail_folders(s)?;
            // The page token: `<folder index>:<UID bound>`, the bound not included.
            let (mut i, mut bound) = match page.and_then(|p| p.split_once(':')) {
                Some((a, b)) => (
                    a.parse::<usize>()
                        .map_err(|_| Error::Decode("page token".into()))?,
                    b.parse::<u64>()
                        .map_err(|_| Error::Decode("page token".into()))?,
                ),
                None => (0, u64::MAX),
            };
            let search = match label {
                Some("UNREAD") => Some(Search::Unseen),
                Some("STARRED") => Some(Search::Flagged),
                Some(l) => l.strip_prefix("kw:").map(Search::Keyword),
                None => None,
            };
            let folder_match = |f: &Folder| match label {
                None => true,
                Some(_) if search.is_some() => true,
                Some(l) => folder_label(f).0 == l,
            };
            let mut messages = Vec::new();
            while let Some(f) = folders.get(i) {
                if !folder_match(f) {
                    i += 1;
                    bound = u64::MAX;
                    continue;
                }
                let e = s.examine(&f.name).map_err(read_error)?;
                let top = bound.min(e.uidnext.map_or(u64::MAX, u64::from));
                let (lo, wanted) = match search {
                    Some(key) => {
                        let mut uids: Vec<u32> = s.search(key).map_err(read_error)?;
                        uids.retain(|u| (*u as u64) < top);
                        uids.sort_unstable_by(|a, b| b.cmp(a));
                        uids.truncate(SEARCH_PAGE);
                        let lo = uids.last().map_or(1, |u| *u as u64);
                        (lo, Some(uids.into_iter().collect::<BTreeSet<u32>>()))
                    }
                    None => (top.saturating_sub(WINDOW as u64).max(1), None),
                };
                let mut got: Vec<Entry> = Vec::new();
                if top > 1 && wanted.as_ref().is_none_or(|w| !w.is_empty()) {
                    let hi = if top == u64::MAX {
                        None
                    } else {
                        Some((top - 1) as u32)
                    };
                    s.list(lo as u32, hi, |en| {
                        if wanted.as_ref().is_none_or(|w| w.contains(&en.uid)) {
                            got.push(en)
                        }
                    })
                    .map_err(read_error)?;
                }
                got.sort_by_key(|e| std::cmp::Reverse(e.uid));
                messages.extend(got.iter().map(|en| {
                    let m = history_message(f, e.uidvalidity, en);
                    MessageRef {
                        id: m.id,
                        thread_id: m.thread_id,
                    }
                }));
                let more = lo > 1
                    && top != u64::MAX
                    && wanted.as_ref().is_none_or(|w| w.len() == SEARCH_PAGE);
                let next = if more {
                    Some(format!("{i}:{lo}"))
                } else {
                    folders[i + 1..]
                        .iter()
                        .position(folder_match)
                        .map(|k| format!("{}:{}", i + 1 + k, u64::MAX))
                };
                return Ok(MessagePage {
                    messages,
                    next_page_token: next,
                });
            }
            Ok(MessagePage {
                messages,
                next_page_token: None,
            })
        })
    }

    fn changes(&self, token: &str, from: &str, page: Option<&str>) -> Result<HistoryPage> {
        let old = parse_cursor(from).ok_or(Error::HistoryExpired)?;
        // The page token: the folder index, then the cursor lines of the pages before.
        let (i, done) = match page {
            Some(p) => {
                let (i, rest) = p.split_once('\n').unwrap_or((p, ""));
                (
                    i.parse::<usize>()
                        .map_err(|_| Error::Decode("page token".into()))?,
                    rest.to_owned(),
                )
            }
            None => (0, String::new()),
        };
        self.with(token, |s| {
            let folders = mail_folders(s)?;
            if old.keys().any(|n| !folders.iter().any(|f| &f.name == n)) {
                return Err(Error::HistoryExpired);
            }
            let mut done = done;
            let mut changes = Vec::new();
            if let Some(f) = folders.get(i) {
                let now = s.examine(&f.name).map_err(read_error)?;
                done.push_str(&mark_line(&f.name, mark_of(&now)));
                changes = folder_changes(s, f, old.get(&f.name).copied(), &now)?
                    .ok_or(Error::HistoryExpired)?;
            }
            let last = i + 1 >= folders.len();
            // The cursor of this page: the new lines so far, the old lines of the rest.
            let mut cursor = done.clone();
            for f in folders.iter().skip(i + 1) {
                if let Some(m) = old.get(&f.name) {
                    cursor.push_str(&mark_line(&f.name, *m));
                }
            }
            Ok(HistoryPage {
                history: vec![History { changes }],
                cursor,
                next_page_token: (!last).then(|| format!("{}\n{done}", i + 1)),
            })
        })
    }

    fn minimal(&self, token: &str, id: &str) -> Result<Minimal> {
        let (validity, uid, folder) = parse_id(id).ok_or(Error::NotFound)?;
        self.with(token, |s| {
            let f = mail_folders(s)?
                .into_iter()
                .find(|f| f.name == folder)
                .ok_or(Error::NotFound)?;
            if s.examine(folder).map_err(read_error)?.uidvalidity != validity {
                return Err(Error::NotFound);
            }
            let mut got = None;
            s.list(uid, Some(uid), |e| got = Some(e))
                .map_err(read_error)?;
            let e = got.ok_or(Error::NotFound)?;
            Ok(Minimal {
                id: id.to_owned(),
                label_ids: labels_of(&f, &e.flags),
            })
        })
    }

    fn metadata(&self, _: &str, _: &str, _: &[&str]) -> Result<Metadata> {
        // Only the send reads it (spec section 6).
        Err(not_supported())
    }

    fn raw(&self, token: &str, id: &str, sink: &mut dyn Write) -> Result<RawMessage> {
        let (validity, uid, folder) = parse_id(id).ok_or(Error::NotFound)?;
        self.with(token, |s| {
            let f = mail_folders(s)?
                .into_iter()
                .find(|f| f.name == folder)
                .ok_or(Error::NotFound)?;
            if s.examine(folder).map_err(read_error)?.uidvalidity != validity {
                return Err(Error::NotFound);
            }
            let mut w = SinkGuard { sink, failed: None };
            let got = s.fetch_raw(uid, &mut w);
            if let Some(e) = w.failed {
                return Err(Error::Sink(e));
            }
            let (size, e) = got.map_err(read_error)?.ok_or(Error::NotFound)?;
            Ok(RawMessage {
                id: id.to_owned(),
                thread_id: thread_of(e.headers.as_deref(), id),
                label_ids: labels_of(&f, &e.flags),
                internal_date: e
                    .internal_date
                    .as_deref()
                    .and_then(internal_ms)
                    .ok_or_else(|| Error::Decode("INTERNALDATE".into()))?,
                size,
            })
        })
    }

    fn messages_with_id(&self, _: &str, _: &str) -> Result<Vec<MessageRef>> {
        // Only the send recovery reads it (spec section 6).
        Err(not_supported())
    }

    fn modify(&self, _: &str, _: &[&str], _: &[&str], _: &[&str]) -> Result<()> {
        // shortcut: no writes yet, part B code 2 adds the flag writes.
        Err(not_supported())
    }

    fn trash(&self, _: &str, _: &str) -> Result<Minimal> {
        Err(not_supported())
    }

    fn untrash(&self, _: &str, _: &str) -> Result<Minimal> {
        Err(not_supported())
    }

    fn create_label(&self, _: &str, name: &str) -> Result<Label> {
        if !crate::is_atom(name) {
            return Err(api("badName", "a keyword must be an IMAP atom"));
        }
        Ok(Label {
            id: format!("kw:{name}"),
            name: name.to_owned(),
            kind: "user".into(),
            messages_total: None,
            messages_unread: None,
            threads_total: None,
            threads_unread: None,
        })
    }

    fn drafts_with_id(&self, _: &str, _: &str) -> Result<Vec<Draft>> {
        Err(not_supported())
    }

    fn draft_raw(&self, _: &str, _: &str) -> Result<DraftRaw> {
        Err(not_supported())
    }

    fn create_draft(&self, _: &str, _: &[u8], _: Option<&str>) -> Result<Draft> {
        Err(not_supported())
    }

    fn send_draft(&self, _: &str, _: &str, _: &[u8], _: Option<&str>) -> Result<Sent> {
        Err(not_supported())
    }
}

/// A sink that keeps its own error apart from the errors of the network.
struct SinkGuard<'a> {
    sink: &'a mut dyn Write,
    failed: Option<std::io::Error>,
}

impl Write for SinkGuard<'_> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.sink
            .write(b)
            .inspect_err(|e| self.failed = Some(std::io::Error::new(e.kind(), e.to_string())))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.sink
            .flush()
            .inspect_err(|e| self.failed = Some(std::io::Error::new(e.kind(), e.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_rules() {
        let h = |s: &str| thread_of(Some(s.as_bytes()), "id");
        assert_eq!(
            h("Message-ID: <a@x>\r\nReferences: <r1@x> <r2@x>\r\nIn-Reply-To: <r2@x>\r\n\r\n"),
            "r1@x"
        );
        assert_eq!(h("Message-ID: <a@x>\r\nIn-Reply-To: <p@x>\r\n\r\n"), "p@x");
        assert_eq!(h("Message-ID: <a@x>\r\n\r\n"), "a@x");
        assert_eq!(h("\r\n"), "id");
        assert_eq!(thread_of(None, "id"), "id");
        assert_eq!(h("References: \r\n\r\n"), "id");
    }

    #[test]
    fn internal_date() {
        assert_eq!(
            internal_ms("17-Nov-2026 09:30:00 +0000"),
            Some(1_794_907_800_000)
        );
        assert_eq!(
            internal_ms(" 7-Nov-2026 09:30:00 +0100"),
            Some(1_794_040_200_000)
        );
        assert_eq!(internal_ms("bad"), None);
    }

    #[test]
    fn ids_and_cursor_round_trip() {
        assert_eq!(parse_id(&message_id(7, 9, "a:b c")), Some((7, 9, "a:b c")));
        assert_eq!(parse_id("x:1:INBOX"), None);
        let m = Mark {
            validity: 1,
            next: 2,
            modseq: 3,
            count: 4,
        };
        let c = parse_cursor(&(mark_line("A b", m) + &mark_line("INBOX", m))).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c["A b"].modseq, 3);
        assert!(parse_cursor("1 2 x 4 INBOX").is_none());
    }
}
