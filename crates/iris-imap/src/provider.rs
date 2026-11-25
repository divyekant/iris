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

/// The label of a draft that is not proof of authorship (spec section 4):
/// a message in a `\Drafts` folder or with the `\Draft` flag. The
/// understanding skips it as it skips `DRAFT`.
pub const UNSENT: &str = "UNSENT";

/// The label of a message in a `\Sent` folder (spec section 4): not proof
/// of authorship. The understanding skips it as it skips `DRAFT`.
pub const SENT_COPY: &str = "SENT_COPY";

/// The messages of one page of [`Provider::messages`], and of one page of
/// [`Provider::changes`] for a new folder.
const WINDOW: u32 = 2000;
/// The sequence numbers of one page of [`Provider::changes`] for a known
/// folder: only the changed messages of them come back.
const CHANGED_WINDOW: u32 = 20_000;
/// The most deletes of one folder in one [`Provider::changes`] sequence.
/// More give a full resync.
const DELETES_MAX: u32 = 20_000;
/// The longest folder name, in bytes. Each ID and label of a page holds the
/// name, so this bound keeps a page small (spec section 6).
const NAME_MAX: usize = 255;
/// The most bytes of records in one page of `changes` (spec section 5).
const PAGE_BYTES: usize = 16 << 20;
/// The longest thread ID from a header: RFC 5322 allows no longer line.
const THREAD_MAX: usize = 998;

/// An IMAP account as a mail provider. The `token` of each call is the
/// password. The calls run one at a time on one session.
pub struct Imap<S: Read + Write> {
    address: String,
    user: String,
    connect: Box<dyn Fn() -> crate::Result<S> + Send + Sync>,
    session: Mutex<Option<Session<S>>>,
    page_bytes: usize,
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
            page_bytes: PAGE_BYTES,
        }
    }

    /// A smaller page budget, for the tests of the budget.
    #[doc(hidden)]
    pub fn with_page_bytes(mut self, bytes: usize) -> Imap<S> {
        self.page_bytes = bytes;
        self
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

/// The error of a read in `changes`: an answer above the response limit
/// (a long `VANISHED` list) starts a full resync, which reads in bounded
/// windows (spec section 1).
fn change_error(e: crate::Error) -> Error {
    match e {
        crate::Error::TooLarge => Error::HistoryExpired,
        e => read_error(e),
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

/// The flags to set (`plus`) and to clear (`minus`) for a `modify` of
/// labels (spec section 7). Only `UNREAD`, `STARRED` and keyword labels;
/// any other label is `notSupported`, before any command.
fn flag_change(add: &[&str], remove: &[&str]) -> Result<(Vec<String>, Vec<String>)> {
    let (mut plus, mut minus) = (Vec::new(), Vec::new());
    for (labels, adding) in [(add, true), (remove, false)] {
        for l in labels {
            let (flag, set) = match *l {
                // UNREAD is the absence of `\Seen`.
                "UNREAD" => ("\\Seen".to_owned(), !adding),
                "STARRED" => ("\\Flagged".to_owned(), adding),
                l => match l.strip_prefix("kw:") {
                    // The lowercase ID (section 4) is the keyword to send.
                    Some(k) if crate::is_atom(k) && !k.is_empty() => {
                        (k.to_ascii_lowercase(), adding)
                    }
                    Some(_) => return Err(api("badName", "not an IMAP keyword")),
                    None => return Err(not_supported()),
                },
            };
            let (to, other) = if set {
                (&mut plus, &minus)
            } else {
                (&mut minus, &plus)
            };
            if other.contains(&flag) {
                return Err(api("badRequest", "a label is added and removed"));
            }
            if !to.contains(&flag) {
                to.push(flag);
            }
        }
    }
    Ok((plus, minus))
}

/// Each flag of `flags` is permanent in the folder of `e` (RFC 3501
/// section 7.1): in `PERMANENTFLAGS`, or a keyword with `\*` there.
fn permanent(e: &crate::Examined, flags: &[&str]) -> bool {
    let Some(p) = &e.permanent else {
        return false;
    };
    flags.iter().all(|f| {
        p.iter().any(|x| x.eq_ignore_ascii_case(f)) || (!f.starts_with('\\') && e.any_keyword)
    })
}

/// The folders that hold mail: selectable, and not a view of other
/// folders. In the page order of spec section 2.
fn mail_folders<S: Read + Write>(s: &mut Session<S>) -> Result<Vec<Folder>> {
    // Gmail over IMAP keeps archived mail only in `[Gmail]/All Mail`, a `\All`
    // folder, so the mirror would miss it. A Gmail account uses the Gmail sign-in.
    if s.has("X-GM-EXT-1") {
        return Err(api(
            "gmailImap",
            "use the Gmail sign-in for a Gmail account",
        ));
    }
    let mut v: Vec<Folder> = s
        .folders()
        .map_err(read_error)?
        .into_iter()
        // A `\All` folder is a view of the mail of the other folders.
        .filter(|f| f.selectable && !matches!(f.special, Some(Special::All | Special::Flagged)))
        .collect();
    // shortcut: a longer folder name refuses the account, issue #650.
    if v.iter().any(|f| f.name.len() > NAME_MAX) {
        return Err(api("longFolderName", "an IMAP folder name is too long"));
    }
    v.sort_by(|a, b| order(&a.name).cmp(&order(&b.name)));
    by_name(&mut v);
    Ok(v)
}

/// The common names of the four special folders (spec section 4), in
/// lowercase, compared with the lowercase text of the last name part. English,
/// German, French, Spanish, Italian, Dutch, Portuguese and the Scandinavian
/// names of the common mail clients and servers.
const NAMES: [(Special, &[&str]); 4] = [
    (
        Special::Sent,
        &[
            "sent",
            "sent items",
            "sent messages",
            "sent mail",
            "gesendet",
            "gesendete objekte",
            "gesendete elemente",
            "envoyés",
            "éléments envoyés",
            "messages envoyés",
            "enviados",
            "elementos enviados",
            "inviati",
            "posta inviata",
            "elementi inviati",
            "verzonden",
            "verzonden items",
            "itens enviados",
            "skickat",
            "skickade objekt",
            "sendt",
            "sendte elementer",
        ],
    ),
    (
        Special::Drafts,
        &[
            "drafts",
            "draft",
            "entwürfe",
            "brouillons",
            "borradores",
            "bozze",
            "concepten",
            "rascunhos",
            "utkast",
            "kladder",
        ],
    ),
    (
        Special::Junk,
        &[
            "junk",
            "spam",
            "junk e-mail",
            "junk email",
            "bulk mail",
            "junk-e-mail",
            "courrier indésirable",
            "indésirables",
            "correo no deseado",
            "posta indesiderata",
            "ongewenste e-mail",
            "lixo eletrônico",
            "skräppost",
            "uønsket post",
        ],
    ),
    (
        Special::Trash,
        &[
            "trash",
            "deleted items",
            "deleted messages",
            "bin",
            "papierkorb",
            "gelöschte elemente",
            "gelöschte objekte",
            "corbeille",
            "éléments supprimés",
            "papelera",
            "elementos eliminados",
            "cestino",
            "posta eliminata",
            "elementi eliminati",
            "prullenbak",
            "verwijderde items",
            "lixeira",
            "itens excluídos",
            "papperskorgen",
            "borttagna objekt",
            "papirkurv",
            "slettede elementer",
        ],
    ),
];

/// A server with no folder of a special-use attribute: each folder with no
/// attribute whose last name part is a common name of it gets it. So sent
/// mail and drafts stay out of the understanding, and spam keeps SPAM.
fn by_name(folders: &mut [Folder]) {
    for (special, names) in NAMES {
        if folders.iter().any(|f| f.special == Some(special)) {
            continue;
        }
        for f in folders.iter_mut().filter(|f| f.special.is_none()) {
            let last = match f.delimiter.as_deref() {
                Some(d) if !d.is_empty() => f.name.rsplit(d).next().unwrap_or(&f.name),
                _ => f.name.as_str(),
            };
            if names.contains(&folder_text(last).to_lowercase().as_str()) {
                f.special = Some(special);
            }
        }
    }
}

/// The text of a folder name in modified UTF-7 (RFC 3501 section 5.1.3),
/// for display and for the name match. A name that is not valid modified
/// UTF-7 stays as the server sent it.
fn folder_text(name: &str) -> String {
    utf7(name).unwrap_or_else(|| name.to_owned())
}

fn utf7(name: &str) -> Option<String> {
    use base64::Engine;
    let mut out = String::new();
    let mut rest = name;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let end = after.find('-')?;
        if end == 0 {
            out.push('&');
        } else {
            let b = base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(after[..end].replace(',', "/"))
                .ok()?;
            if b.len() % 2 != 0 {
                return None;
            }
            let units: Vec<u16> = b
                .chunks(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            out.push_str(&String::from_utf16(&units).ok()?);
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// The page order of a folder (spec section 2): `INBOX` first, then by name.
fn order(name: &str) -> (bool, &[u8]) {
    (!name.eq_ignore_ascii_case("INBOX"), name.as_bytes())
}

/// The first folder after `name` in the page order that `keep` takes.
fn after<'a>(
    folders: &'a [Folder],
    name: &str,
    keep: impl Fn(&Folder) -> bool,
) -> Option<&'a Folder> {
    folders
        .iter()
        .find(|f| order(&f.name) > order(name) && keep(f))
}

/// The label of a folder: (ID, name, kind).
fn folder_label(f: &Folder) -> (String, String, &'static str) {
    let fixed = |id: &str| (id.to_owned(), id.to_owned(), "system");
    if f.name.eq_ignore_ascii_case("INBOX") {
        return fixed("INBOX");
    }
    match f.special {
        Some(Special::Junk) => fixed("SPAM"),
        Some(Special::Trash) => fixed("TRASH"),
        // `\Sent` and `\Drafts` too: no IMAP message gets SENT or DRAFT. Each
        // folder is system: the provider cannot change a folder (spec section 4).
        _ => (format!("folder:{}", f.name), folder_text(&f.name), "system"),
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
    if has("\\Draft") || f.special == Some(Special::Drafts) {
        out.push(UNSENT.into());
    }
    if f.special == Some(Special::Sent) {
        out.push(SENT_COPY.into());
    }
    let kws: std::collections::BTreeSet<String> = flags
        .iter()
        .filter(|g| !g.starts_with('\\'))
        .map(|g| keyword_id(g))
        .collect();
    out.extend(kws);
    out
}

/// The names of fixed labels of any provider: no folder or keyword label
/// takes one of them (spec section 4).
const FIXED: [&str; 9] = [
    "INBOX", "UNREAD", "STARRED", "SPAM", "TRASH", "SENT", "DRAFT", UNSENT, SENT_COPY,
];

/// One name for each label, with no case (spec section 4). In ID order
/// (fixed labels, then folder labels, then keyword labels), a name that a
/// fixed name or an earlier label has gets " (folder)" or " (keyword)",
/// then " (folder 2)", " (folder 3)" and so on. A lookup by name finds one
/// label. The fixed names are taken first, so no label takes one. Folder
/// names decode to Unicode, so the compare folds Unicode case, not only ASCII.
fn unique_names(mut labels: Vec<Label>) -> Vec<Label> {
    labels.sort_by(|a, b| a.id.cmp(&b.id));
    let mut taken: BTreeSet<String> = FIXED.iter().map(|x| x.to_lowercase()).collect();
    for l in labels.iter_mut() {
        let tag = if l.id.starts_with("folder:") {
            "folder"
        } else if l.id.starts_with("kw:") {
            "keyword"
        } else {
            continue;
        };
        let base = l.name.clone();
        let mut n = 1;
        while taken.contains(&l.name.to_lowercase()) {
            l.name = if n == 1 {
                format!("{base} ({tag})")
            } else {
                format!("{base} ({tag} {n})")
            };
            n += 1;
        }
        taken.insert(l.name.to_lowercase());
    }
    labels
}

/// The label ID of a keyword. IMAP compares keywords with no case (RFC 3501
/// section 9, note 1), so `$Todo` and `$todo` are one label.
fn keyword_id(k: &str) -> String {
    format!("kw:{}", k.to_ascii_lowercase())
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
        .filter(|t| !t.is_empty() && t.len() <= THREAD_MAX)
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
    /// The special use of the folder, from its attribute or its name
    /// (section 4): a change of it changes the labels of each message.
    role: char,
}

fn role_of(f: &Folder) -> char {
    match f.special {
        Some(Special::Sent) => 'S',
        Some(Special::Drafts) => 'D',
        Some(Special::Junk) => 'J',
        Some(Special::Trash) => 'T',
        _ => '-',
    }
}

fn mark_line(name: &str, m: Mark) -> String {
    format!(
        "{} {} {} {} {} {name}\n",
        m.validity, m.next, m.modseq, m.count, m.role
    )
}

/// The folders of a cursor. `None`: the text does not parse.
fn parse_cursor(text: &str) -> Option<BTreeMap<String, Mark>> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let mut p = line.splitn(6, ' ');
        let mut num = || p.next()?.parse::<u64>().ok();
        let (validity, next, modseq, count) = (num()?, num()?, num()?, num()?);
        let role = p.next()?;
        let role = match role {
            "S" | "D" | "J" | "T" | "-" => role.chars().next()?,
            _ => return None,
        };
        let name = p.next()?;
        let m = Mark {
            validity: u32::try_from(validity).ok()?,
            next: u32::try_from(next).ok()?,
            modseq,
            count: u32::try_from(count).ok()?,
            role,
        };
        // Two lines for one folder: not a cursor of ours.
        if out.insert(name.to_owned(), m).is_some() {
            return None;
        }
    }
    // A profile always has INBOX, so an empty cursor is not one of ours.
    (!out.is_empty()).then_some(out)
}

fn mark_of(f: &Folder, e: &crate::Examined) -> Mark {
    Mark {
        role: role_of(f),
        validity: e.uidvalidity,
        next: e.uidnext.unwrap_or(0),
        modseq: e.highest_modseq.unwrap_or(0),
        count: e.exists,
    }
}

/// Where a [`Provider::changes`] sequence is (spec section 5).
enum At {
    /// The start of the folder `name`: no window read yet.
    Folder(String),
    /// The window of `name` with the highest sequence number `high`.
    /// `mark` is the `EXAMINE` of the folder before its first window.
    Window { name: String, mark: Mark, high: u32 },
}

/// A page token of [`Provider::changes`]: the place, then the cursor
/// lines of the folders done.
fn changes_token(at: &At, done: &str) -> String {
    match at {
        At::Folder(name) => format!("F\n{name}\n{done}"),
        At::Window { name, mark, high } => format!("W {high}\n{}{done}", mark_line(name, *mark)),
    }
}

fn parse_changes_token(p: &str) -> Option<(At, String)> {
    let (head, rest) = p.split_once('\n')?;
    if head == "F" {
        let (name, done) = rest.split_once('\n')?;
        return Some((At::Folder(name.to_owned()), done.to_owned()));
    }
    let high = head.strip_prefix("W ")?.parse().ok()?;
    let (line, done) = rest.split_once('\n')?;
    let (name, mark) = parse_cursor(line)?.pop_first()?;
    Some((At::Window { name, mark, high }, done.to_owned()))
}

/// The bytes of one change record that a page keeps (spec section 6).
fn record_bytes(c: &Change) -> usize {
    let (m, extra) = match c {
        Change::Added(m) | Change::Deleted(m) => (m, &[][..]),
        Change::LabelsAdded(m, l) | Change::LabelsRemoved(m, l) => (m, &l[..]),
    };
    let labels = |l: &[String]| l.iter().map(String::len).sum::<usize>();
    m.id.len() + m.thread_id.len() + labels(&m.label_ids) + labels(extra)
}

fn deleted(validity: u32, uid: u32, folder: &str) -> Change {
    let id = message_id(validity, uid, folder);
    Change::Deleted(HistoryMessage {
        thread_id: id.clone(),
        id,
        label_ids: Vec::new(),
    })
}

/// The removals of folder `f` (open) between `old` and `mark`, read after
/// its last window. `None`: a full resync is needed.
fn removals<S: Read + Write>(
    s: &mut Session<S>,
    f: &Folder,
    old: Mark,
    mark: Mark,
) -> Result<Option<Vec<Change>>> {
    let mut out = Vec::new();
    if s.qresync() {
        let mut n = 0u64;
        s.vanished(old.modseq, |r| {
            n += u64::from(r.end() - r.start()) + 1;
            if n <= u64::from(DELETES_MAX) {
                out.extend(r.map(|uid| deleted(mark.validity, uid, &f.name)));
            }
        })
        .map_err(change_error)?;
        return Ok((n <= u64::from(DELETES_MAX)).then_some(out));
    }
    // `CONDSTORE` only (spec section 5).
    if old.next == 0 || mark.next < old.next || mark.next - old.next > DELETES_MAX {
        return Ok(None);
    }
    let present: BTreeSet<u32> = if mark.next > old.next {
        s.search(None, Search::Uids(old.next, mark.next - 1))
            .map_err(change_error)?
            .into_iter()
            .filter(|u| (old.next..mark.next).contains(u))
            .collect()
    } else {
        BTreeSet::new()
    };
    // Removed = count before - (count now - new UIDs).
    if i64::from(mark.count) - present.len() as i64 != i64::from(old.count) {
        return Ok(None);
    }
    // A UID from the `UIDNEXT` of `from` that is not here now came and went
    // after `from`: the first sync can have listed it. Delete it.
    out.extend(
        (old.next..mark.next)
            .filter(|u| !present.contains(u))
            .map(|uid| deleted(mark.validity, uid, &f.name)),
    );
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
                // A `NOMODSEQ` folder has no change record: no sync (issue #629).
                if e.highest_modseq.is_none() {
                    return Err(api("noCondstore", "an IMAP folder has no mod-sequences"));
                }
                // The removal rule of `CONDSTORE` only needs a `UIDNEXT` that only grows.
                if e.uidnext.is_none() && !s.qresync() {
                    return Err(api("noUidnext", "an IMAP folder has no UIDNEXT"));
                }
                cursor.push_str(&mark_line(&f.name, mark_of(&f, &e)));
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
            for id in ["UNREAD", "STARRED", UNSENT, SENT_COPY] {
                out.insert(id.into(), label(id, id, "system"));
            }
            for f in mail_folders(s)? {
                let (id, name, kind) = folder_label(&f);
                out.insert(id.clone(), label(&id, &name, kind));
                // SELECT, not EXAMINE: a server sends the keywords of
                // PERMANENTFLAGS only to a read-write open (RFC 3501 6.3.2).
                for k in s.select(&f.name).map_err(read_error)?.flags {
                    if !k.starts_with('\\') {
                        // The first spelling in folder order names the label.
                        let id = keyword_id(&k);
                        out.entry(id.clone())
                            .or_insert_with(|| label(&id, &k, "user"));
                    }
                }
            }
            Ok(unique_names(out.into_values().collect()))
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
            // The page token: `<UID bound>:<folder>`, the bound not included.
            let (name, bound) = match page {
                Some(p) => {
                    let (b, n) = p
                        .split_once(':')
                        .ok_or(Error::Decode("page token".into()))?;
                    let b = b
                        .parse::<u64>()
                        .map_err(|_| Error::Decode("page token".into()))?;
                    (Some(n), b)
                }
                None => (None, u64::MAX),
            };
            let search = match label {
                Some(UNSENT) => Some(Search::Draft),
                Some("UNREAD") => Some(Search::Unseen),
                Some("STARRED") => Some(Search::Flagged),
                Some(l) => l.strip_prefix("kw:").map(Search::Keyword),
                None => None,
            };
            let folder_match = |f: &Folder| match label {
                None => true,
                Some(SENT_COPY) => f.special == Some(Special::Sent),
                Some(_) if search.is_some() => true,
                Some(l) => folder_label(f).0 == l,
            };
            let mut messages = Vec::new();
            // The folder of the page. A folder deleted during the sequence
            // gives the next one: its mail is gone too.
            let found = match name {
                Some(n) => folders
                    .iter()
                    .find(|f| f.name == n && folder_match(f))
                    .map(|f| (f, bound))
                    .or_else(|| after(&folders, n, folder_match).map(|f| (f, u64::MAX))),
                None => folders
                    .iter()
                    .find(|f| folder_match(f))
                    .map(|f| (f, u64::MAX)),
            };
            if let Some((f, bound)) = found {
                let e = s.examine(&f.name).map_err(read_error)?;
                // (UID, reference): each entry becomes compact in the callback,
                // so no header block stays for the page (spec section 6).
                let mut got: Vec<(u32, MessageRef)> = Vec::new();
                let validity = e.uidvalidity;
                let mut keep = |en: Entry| {
                    let m = history_message(f, validity, &en);
                    got.push((
                        en.uid,
                        MessageRef {
                            id: m.id,
                            thread_id: m.thread_id,
                        },
                    ));
                };
                // A window of sequence numbers: for a search too, so that
                // each answer holds at most WINDOW UIDs.
                let high = u64::from(e.exists).min(bound.saturating_sub(1)) as u32;
                let low = high.saturating_sub(WINDOW - 1).max(1);
                if high > 0 {
                    match search {
                        // Each message of a Drafts folder is UNSENT.
                        Some(Search::Draft) if f.special == Some(Special::Drafts) => {
                            s.window(low, high, None, &mut keep).map_err(read_error)?
                        }
                        Some(key) => {
                            let uids = s.search(Some((low, high)), key).map_err(read_error)?;
                            s.fetch_uids(&uids, &mut keep).map_err(read_error)?;
                        }
                        None => s.window(low, high, None, &mut keep).map_err(read_error)?,
                    }
                }
                let more = (low > 1).then_some(low);
                got.sort_by_key(|(uid, _)| std::cmp::Reverse(*uid));
                messages.extend(got.into_iter().map(|(_, m)| m));
                let next = if let Some(lo) = more {
                    Some(format!("{lo}:{}", f.name))
                } else {
                    after(&folders, &f.name, folder_match)
                        .map(|g| format!("{}:{}", u64::MAX, g.name))
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
        let budget = self.page_bytes;
        let start = match page {
            Some(p) => Some(parse_changes_token(p).ok_or(Error::Decode("page token".into()))?),
            None => None,
        };
        self.with(token, |s| {
            let folders = mail_folders(s)?;
            if old.keys().any(|n| !folders.iter().any(|f| &f.name == n)) {
                return Err(Error::HistoryExpired);
            }
            let (at, mut done) = match start {
                Some(x) => x,
                None => match folders.first() {
                    Some(f) => (At::Folder(f.name.clone()), String::new()),
                    None => return Err(Error::HistoryExpired),
                },
            };
            let name = match &at {
                At::Folder(n) | At::Window { name: n, .. } => n.clone(),
            };
            // A folder deleted during the sequence: start again.
            let f = folders
                .iter()
                .find(|f| f.name == name)
                .ok_or(Error::HistoryExpired)?;
            let now = s.examine(&f.name).map_err(change_error)?;
            let (mark, high) = match at {
                At::Folder(_) => (mark_of(f, &now), now.exists),
                // Removals since the token lower `EXISTS`.
                At::Window { mark, high, .. } => (mark, high.min(now.exists)),
            };
            // A folder that is not in `from`: the first sync can have listed
            // mail of it that is gone now, and `from` cannot show which. Only
            // a full resync removes it (spec section 5).
            let before = old.get(&f.name).copied().ok_or(Error::HistoryExpired)?;
            // A new role of the folder (an attribute or a name) changes the
            // labels of each message with no other trace (spec section 5).
            if now.uidvalidity != mark.validity
                || role_of(f) != mark.role
                || before.validity != mark.validity
                || before.role != mark.role
            {
                return Err(Error::HistoryExpired);
            }
            let unchanged = before.modseq == mark.modseq
                && before.count == mark.count
                && before.next == mark.next;
            // `NOMODSEQ`: a flag change leaves no trace (spec section 5).
            if mark.modseq == 0 || before.modseq == 0 {
                return Err(Error::HistoryExpired);
            }
            let mut changes = Vec::new();
            if !unchanged {
                // Newest first, in windows of sequence numbers: dense, so the
                // page count follows the message count (spec section 5).
                let low = high.saturating_sub(CHANGED_WINDOW - 1).max(1);
                if high > 0 {
                    let mut bytes = 0usize;
                    s.window(low, high, Some(before.modseq), |e| {
                        let c = Change::Added(history_message(f, mark.validity, &e));
                        bytes += record_bytes(&c);
                        if bytes <= budget {
                            changes.push(c);
                        }
                    })
                    .map_err(change_error)?;
                    // Too large for one page: the full resync reads pages of
                    // `messages` and one message at a time (spec section 5).
                    if bytes > budget {
                        return Err(Error::HistoryExpired);
                    }
                }
                if low > 1 {
                    let at = At::Window {
                        name: f.name.clone(),
                        mark,
                        high: low - 1,
                    };
                    return Ok(HistoryPage {
                        history: vec![History { changes }],
                        cursor: from.to_owned(),
                        next_page_token: Some(changes_token(&at, &done)),
                    });
                }
                changes.extend(removals(s, f, before, mark)?.ok_or(Error::HistoryExpired)?);
                // The budget holds for all records of the page, removals too.
                if changes.iter().map(record_bytes).sum::<usize>() > budget {
                    return Err(Error::HistoryExpired);
                }
            }
            done.push_str(&mark_line(&f.name, mark));
            let next = after(&folders, &f.name, |_| true);
            // The cursor of this page: the new lines so far, the old lines of the rest.
            let mut cursor = done.clone();
            for g in folders.iter().filter(|g| order(&g.name) > order(&f.name)) {
                if let Some(m) = old.get(&g.name) {
                    cursor.push_str(&mark_line(&g.name, *m));
                }
            }
            Ok(HistoryPage {
                history: vec![History { changes }],
                cursor,
                next_page_token: next.map(|g| changes_token(&At::Folder(g.name.clone()), &done)),
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

    fn modify(&self, token: &str, ids: &[&str], add: &[&str], remove: &[&str]) -> Result<()> {
        let (plus, minus) = flag_change(add, remove)?;
        // The targets of each folder: (UIDVALIDITY, UIDs). All checks come
        // before the first write (spec section 7).
        let mut targets: BTreeMap<&str, (u32, Vec<u32>)> = BTreeMap::new();
        for id in ids {
            let (validity, uid, folder) = parse_id(id).ok_or(Error::NotFound)?;
            let t = targets.entry(folder).or_insert((validity, Vec::new()));
            // Two UIDVALIDITY values for one folder: one ID is stale.
            if t.0 != validity {
                return Err(Error::NotFound);
            }
            t.1.push(uid);
        }
        if targets.is_empty() || (plus.is_empty() && minus.is_empty()) {
            return Ok(());
        }
        let all: Vec<&str> = plus.iter().chain(&minus).map(String::as_str).collect();
        self.with(token, |s| {
            let folders = mail_folders(s)?;
            // The check pass: no STORE.
            for (folder, (validity, _)) in &targets {
                if !folders.iter().any(|f| f.name == *folder) {
                    return Err(Error::NotFound);
                }
                let e = s.select(folder).map_err(read_error)?;
                if e.uidvalidity != *validity {
                    return Err(Error::NotFound);
                }
                if !permanent(&e, &all) {
                    return Err(api("notPermanent", "the server does not keep this flag"));
                }
            }
            // The write pass. After the first STORE, each error is unknown.
            let mut sent = false;
            let unknown = |e: Error| Error::Transport(format!("unknown result: {e}"));
            for (folder, (validity, uids)) in &targets {
                let e = s.select(folder).map_err(|e| {
                    let e = read_error(e);
                    if sent {
                        unknown(e)
                    } else {
                        e
                    }
                })?;
                if e.uidvalidity != *validity {
                    return Err(if sent {
                        unknown(Error::NotFound)
                    } else {
                        Error::NotFound
                    });
                }
                for (flags, add) in [(&plus, true), (&minus, false)] {
                    if flags.is_empty() {
                        continue;
                    }
                    let names: Vec<&str> = flags.iter().map(String::as_str).collect();
                    let mut seen: BTreeMap<u32, Vec<String>> = BTreeMap::new();
                    sent = true;
                    s.store(uids, add, &names, |uid, f| {
                        seen.insert(uid, f);
                    })
                    .map_err(|e| unknown(read_error(e)))?;
                    // Each target must come back with the asked flags.
                    for uid in uids {
                        let got = seen.get(uid).ok_or_else(|| {
                            Error::Transport("unknown result: a UID had no answer".into())
                        })?;
                        let has = |x: &str| got.iter().any(|g| g.eq_ignore_ascii_case(x));
                        if names.iter().any(|n| has(n) != add) {
                            return Err(Error::Transport(
                                "unknown result: other flags in the answer".into(),
                            ));
                        }
                    }
                }
            }
            Ok(())
        })
    }

    fn trash(&self, _: &str, _: &str) -> Result<Minimal> {
        Err(not_supported())
    }

    fn untrash(&self, _: &str, _: &str) -> Result<Minimal> {
        Err(not_supported())
    }

    fn create_label(&self, _: &str, _: &str) -> Result<Label> {
        // shortcut: a keyword exists only on a message, so a label made
        // before its first STORE can be a lie, issue #652.
        Err(not_supported())
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

    /// Modified UTF-7 of `s` (RFC 3501 section 5.1.3), the encoder of the test.
    fn encode(s: &str) -> String {
        use base64::Engine;
        let mut out = String::new();
        let mut wide: Vec<u16> = Vec::new();
        let flush = |out: &mut String, wide: &mut Vec<u16>| {
            if !wide.is_empty() {
                let b: Vec<u8> = wide.iter().flat_map(|u| u.to_be_bytes()).collect();
                let t = base64::engine::general_purpose::STANDARD_NO_PAD.encode(b);
                out.push_str(&format!("&{}-", t.replace('/', ",")));
                wide.clear();
            }
        };
        for c in s.chars() {
            if (' '..='~').contains(&c) {
                flush(&mut out, &mut wide);
                if c == '&' {
                    out.push_str("&-");
                } else {
                    out.push(c);
                }
            } else {
                wide.extend(c.encode_utf16(&mut [0; 2]).iter());
            }
        }
        flush(&mut out, &mut wide);
        out
    }

    #[test]
    fn folder_text_examples() {
        assert_eq!(folder_text("Entw&APw-rfe"), "Entwürfe");
        assert_eq!(folder_text("A&-B"), "A&B");
        assert_eq!(
            folder_text("~peter/mail/&U,BTFw-/&ZeVnLIqe-"),
            "~peter/mail/台北/日本語"
        );
        // Not valid: the name stays as the server sent it.
        for bad in ["a&b", "&AP-", "&2D0-", "&AP,-"] {
            assert_eq!(folder_text(bad), bad);
        }
    }

    /// Property: each text round-trips through modified UTF-7, and no input
    /// of random bytes from the alphabet panics.
    #[test]
    fn folder_text_round_trip_and_no_panic() {
        let chars = [
            'a', 'Z', ' ', '&', '-', ',', '/', '+', 'ü', 'é', '台', '😀', 'A',
        ];
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..5000 {
            let n = (next() % 12) as usize;
            let s: String = (0..n)
                .map(|_| chars[(next() % chars.len() as u64) as usize])
                .collect();
            assert_eq!(folder_text(&encode(&s)), s, "{s:?}");
            // Raw input that a server can send: no panic.
            let _ = folder_text(&s);
        }
    }

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
    fn changes_too_large() {
        assert!(matches!(
            change_error(crate::Error::TooLarge),
            Error::HistoryExpired
        ));
        assert!(matches!(
            read_error(crate::Error::TooLarge),
            Error::Decode(_)
        ));
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
    fn label_names_are_unique() {
        let l = |id: &str, name: &str| Label {
            id: id.into(),
            name: name.into(),
            kind: "user".into(),
            messages_total: None,
            messages_unread: None,
            threads_total: None,
            threads_unread: None,
        };
        let out = unique_names(vec![
            l("INBOX", "INBOX"),
            l("folder:Spam", "Spam"),
            l("folder:Work", "Work"),
            l("kw:inbox", "Inbox"),
            l("kw:work", "work"),
            l("kw:late", "Late"),
            l("folder:work", "work"),
            l("folder:Work (folder)", "Work (folder)"),
        ]);
        let names: Vec<&str> = out.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "INBOX",
                "Spam (folder)",
                "Work",
                "Work (folder)",
                "work (folder 2)",
                "Inbox (keyword)",
                "Late",
                "work (keyword)",
            ]
        );
        // Decoded names that differ only in Unicode case get one name each.
        let out = unique_names(vec![
            l("folder:&ANw-ber", "Über"),
            l("folder:&APw-ber", "über"),
        ]);
        let names: Vec<&str> = out.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Über", "über (folder)"]);
    }

    #[test]
    fn long_thread_header_is_ignored() {
        let long = format!("Message-ID: <{}@x>\r\n\r\n", "a".repeat(THREAD_MAX));
        assert_eq!(thread_of(Some(long.as_bytes()), "1:2:INBOX"), "1:2:INBOX");
        let short = b"Message-ID: <a@x>\r\n\r\n";
        assert_eq!(thread_of(Some(short), "1:2:INBOX"), "a@x");
    }

    /// A small xorshift generator: the tests stay deterministic.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn render(c: &BTreeMap<String, Mark>) -> String {
        c.iter().map(|(n, m)| mark_line(n, *m)).collect()
    }

    /// Property: a cursor parses back to the same folders; any change to
    /// its text never panics and gives a cursor that renders the same way
    /// again or `None`; a second line for one folder is `None`. The token
    /// parser never panics either.
    #[test]
    fn cursor_parsers_property() {
        let mut rng = Rng(0x2545f4914f6cdd1d);
        let names = ["INBOX", "A b", "x:y", "Spam", "a/b c"];
        for _ in 0..5_000 {
            let mut c = BTreeMap::new();
            for n in names.iter().take(1 + rng.below(5) as usize) {
                c.insert(
                    n.to_string(),
                    Mark {
                        validity: rng.next() as u32,
                        next: rng.next() as u32,
                        modseq: rng.next(),
                        count: rng.next() as u32,
                        role: ['S', 'D', 'J', 'T', '-'][rng.below(5) as usize],
                    },
                );
            }
            let text = render(&c);
            let back = parse_cursor(&text).unwrap();
            assert_eq!(render(&back), text);
            // A duplicate folder line.
            let first = text.lines().next().unwrap();
            assert!(parse_cursor(&format!("{text}{first}\n")).is_none());
            // Random edits.
            let mut b = text.into_bytes();
            for _ in 0..1 + rng.below(4) {
                if b.is_empty() {
                    break;
                }
                let at = rng.below(b.len() as u64) as usize;
                match rng.below(3) {
                    0 => b[at] = b" \n0123456789-SJx:"[rng.below(16) as usize],
                    1 => {
                        b.remove(at);
                    }
                    _ => b.truncate(at),
                }
            }
            let t = String::from_utf8_lossy(&b);
            if let Some(p) = parse_cursor(&t) {
                assert_eq!(
                    parse_cursor(&render(&p)).map(|q| render(&q)),
                    Some(render(&p))
                );
            }
            let _ = parse_changes_token(&t);
            let _ = parse_changes_token(&format!("W 5\n{t}"));
            let _ = parse_changes_token(&format!("F\n{t}"));
        }
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
            role: 'J',
        };
        let c = parse_cursor(&(mark_line("A b", m) + &mark_line("INBOX", m))).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c["A b"].modseq, 3);
        assert_eq!(c["A b"].role, 'J');
        assert!(parse_cursor("1 2 x 4 - INBOX").is_none());
        assert!(parse_cursor("1 2 3 4 X INBOX").is_none());
    }
}
