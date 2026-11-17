//! A fake IMAP server for tests (spec `docs/specs/2026-11-17-imap-client.md`).
//!
//! `FakeImap::start(caps)` serves plain IMAP on `127.0.0.1` at a random
//! port. Each connection runs on its own thread. It needs no network.
//!
//! Supported commands: `CAPABILITY`, `NOOP`, `LOGOUT`, `LOGIN`,
//! `AUTHENTICATE PLAIN` and `AUTHENTICATE XOAUTH2`, `ENABLE`, `LIST`,
//! `EXAMINE` (with `(CONDSTORE)`) and `UID FETCH` with the items `UID`,
//! `FLAGS`, `MODSEQ` and `BODY.PEEK[]`, and the modifiers `CHANGEDSINCE` and
//! `VANISHED`. Each other command gets `BAD`.
//!
//! The server has `CONDSTORE`, `QRESYNC` and `AUTH=PLAIN` only when [`Caps`]
//! says so, so that a test can run each sync mode. A test changes the
//! mailbox with the methods of [`FakeImap`]. Each change gets a new
//! mod-sequence, as RFC 7162 says.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

/// The user name that the fake accepts.
pub const USER: &str = "owner@example.test";
/// The password that the fake accepts.
pub const PASSWORD: &str = "app \"pass\" \\ word";
/// The `INTERNALDATE` of each message.
pub const DATE: &str = "17-Nov-2026 09:30:00 +0000";
/// The OAuth access token that the fake accepts.
pub const TOKEN: &str = "ya29.fake-token";

/// The capabilities of the server.
#[derive(Clone, Copy, Debug)]
pub struct Caps {
    pub condstore: bool,
    pub qresync: bool,
    /// `AUTH=PLAIN`. Without it, the client must use `LOGIN`.
    pub plain: bool,
}

impl Caps {
    /// All capabilities on.
    pub const ALL: Caps = Caps {
        condstore: true,
        qresync: true,
        plain: true,
    };
}

#[derive(Clone)]
struct Message {
    flags: Vec<String>,
    modseq: u64,
    raw: Arc<Vec<u8>>,
}

struct Folder {
    special: Option<String>,
    uidvalidity: u32,
    uidnext: u32,
    messages: BTreeMap<u32, Message>,
    /// Removed UIDs and the mod-sequence of the removal.
    gone: Vec<(u32, u64)>,
}

struct State {
    caps: Caps,
    folders: BTreeMap<String, Folder>,
    modseq: u64,
    validity: u32,
    commands: Vec<String>,
}

/// A running fake server. The thread lives until the process ends.
#[derive(Clone)]
pub struct FakeImap {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
}

impl FakeImap {
    /// Start a server with an empty `INBOX`.
    pub fn start(caps: Caps) -> FakeImap {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let state = Arc::new(Mutex::new(State {
            caps,
            folders: BTreeMap::new(),
            modseq: 1,
            validity: 1000,
            commands: Vec::new(),
        }));
        let fake = FakeImap { addr, state };
        fake.create("INBOX", None);
        let shared = fake.state.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = shared.clone();
                // No Nagle delay: each command waits for its answer.
                let _ = stream.set_nodelay(true);
                thread::spawn(move || {
                    let _ = serve(stream, &state);
                });
            }
        });
        fake
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Each command line that the server got, with the sign-in data
    /// replaced by `***`.
    pub fn commands(&self) -> Vec<String> {
        self.lock().commands.clone()
    }

    /// Make a folder. `special` is a special-use attribute, for example
    /// `\Sent`.
    pub fn create(&self, name: &str, special: Option<&str>) {
        let mut s = self.lock();
        s.validity += 1;
        let uidvalidity = s.validity;
        s.folders.insert(
            name.to_owned(),
            Folder {
                special: special.map(str::to_owned),
                uidvalidity,
                uidnext: 1,
                messages: BTreeMap::new(),
                gone: Vec::new(),
            },
        );
    }

    /// Add a message and give its UID.
    pub fn add(&self, folder: &str, raw: Vec<u8>, flags: &[&str]) -> u32 {
        let mut s = self.lock();
        let modseq = s.next_modseq();
        let f = s.folders.get_mut(folder).expect("folder");
        let uid = f.uidnext;
        f.uidnext += 1;
        f.messages.insert(
            uid,
            Message {
                flags: flags.iter().map(|x| x.to_string()).collect(),
                modseq,
                raw: Arc::new(raw),
            },
        );
        uid
    }

    /// Replace the flags of a message.
    pub fn set_flags(&self, folder: &str, uid: u32, flags: &[&str]) {
        let mut s = self.lock();
        let modseq = s.next_modseq();
        let m = s
            .folders
            .get_mut(folder)
            .expect("folder")
            .messages
            .get_mut(&uid)
            .expect("uid");
        m.flags = flags.iter().map(|x| x.to_string()).collect();
        m.modseq = modseq;
    }

    /// Remove a message (`EXPUNGE`).
    pub fn remove(&self, folder: &str, uid: u32) {
        let mut s = self.lock();
        let modseq = s.next_modseq();
        let f = s.folders.get_mut(folder).expect("folder");
        if f.messages.remove(&uid).is_some() {
            f.gone.push((uid, modseq));
        }
    }

    /// Delete a folder and its messages.
    pub fn drop_folder(&self, name: &str) {
        self.lock().folders.remove(name);
    }

    /// Give the folder a new `UIDVALIDITY`. Each message gets a new UID,
    /// from 1.
    pub fn renew(&self, folder: &str) {
        let mut s = self.lock();
        s.validity += 1;
        let validity = s.validity;
        let modseq = s.next_modseq();
        let f = s.folders.get_mut(folder).expect("folder");
        let old = std::mem::take(&mut f.messages);
        f.uidvalidity = validity;
        f.gone.clear();
        f.uidnext = 1;
        for (_, mut m) in old {
            m.modseq = modseq;
            f.messages.insert(f.uidnext, m);
            f.uidnext += 1;
        }
    }

    /// The state of a folder: `UIDVALIDITY` and each UID with its flags.
    pub fn snapshot(&self, folder: &str) -> (u32, BTreeMap<u32, Vec<String>>) {
        let s = self.lock();
        let f = &s.folders[folder];
        let map = f
            .messages
            .iter()
            .map(|(u, m)| (*u, m.flags.clone()))
            .collect();
        (f.uidvalidity, map)
    }
}

impl State {
    fn next_modseq(&mut self) -> u64 {
        self.modseq += 1;
        self.modseq
    }
}

/// The `Message-ID`, `In-Reply-To` and `References` lines of the header
/// of `raw`, with their folded lines, and an empty line at the end.
fn thread_fields(raw: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(raw);
    let head = text.split("\r\n\r\n").next().unwrap_or("");
    let mut out = String::new();
    let mut keep = false;
    for line in head.split("\r\n") {
        if !line.starts_with([' ', '\t']) {
            let name = line.split(':').next().unwrap_or("").to_ascii_lowercase();
            keep = ["message-id", "in-reply-to", "references"].contains(&name.as_str());
        }
        if keep {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }
    out.push_str("\r\n");
    out.into_bytes()
}

/// Split a command line into words. A quoted string is one word, without
/// the quotes. A parenthesized list is one word, with the parentheses.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == ' ' {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut w = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => w.extend(chars.next()),
                    '"' => break,
                    _ => w.push(c),
                }
            }
            out.push(w);
        } else if c == '(' {
            let mut w = String::new();
            let mut depth = 0;
            for c in chars.by_ref() {
                w.push(c);
                depth += (c == '(') as i32 - (c == ')') as i32;
                if depth == 0 {
                    break;
                }
            }
            out.push(w);
        } else {
            let mut w = String::new();
            while let Some(&c) = chars.peek() {
                if c == ' ' {
                    break;
                }
                w.push(c);
                chars.next();
            }
            out.push(w);
        }
    }
    out
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Parse a UID set. `*` is the largest UID. A range can be in either order.
fn uid_set(set: &str, max: u32) -> Vec<(u32, u32)> {
    let num = |x: &str| if x == "*" { Some(max) } else { x.parse().ok() };
    set.split(',')
        .filter_map(|part| {
            let (a, b) = part.split_once(':').unwrap_or((part, part));
            let (a, b) = (num(a)?, num(b)?);
            Some((a.min(b), a.max(b)))
        })
        .collect()
}

fn decode64(s: &str) -> Vec<u8> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for b in s.bytes().filter(|b| *b != b'=') {
        let Some(v) = A.iter().position(|a| *a == b) else {
            return Vec::new();
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

struct Conn {
    authed: bool,
    folder: Option<String>,
    condstore: bool,
    qresync: bool,
}

fn serve(stream: TcpStream, state: &Mutex<State>) -> std::io::Result<()> {
    let mut out = stream.try_clone()?;
    let mut input = BufReader::new(stream);
    let lock = || state.lock().unwrap_or_else(|e| e.into_inner());
    let caps = |c: Caps| {
        let mut s = String::from("IMAP4rev1 ENABLE AUTH=XOAUTH2");
        for (on, name) in [
            (c.plain, " AUTH=PLAIN"),
            (c.condstore, " CONDSTORE"),
            (c.qresync, " QRESYNC"),
        ] {
            if on {
                s.push_str(name);
            }
        }
        s
    };
    out.write_all(format!("* OK [CAPABILITY {}] fake ready\r\n", caps(lock().caps)).as_bytes())?;
    let mut conn = Conn {
        authed: false,
        folder: None,
        condstore: false,
        qresync: false,
    };
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let text = line.trim_end_matches(['\r', '\n']);
        let w = words(text);
        let (tag, cmd) = match (w.first(), w.get(1)) {
            (Some(t), Some(c)) => (t.clone(), c.to_ascii_uppercase()),
            _ => {
                out.write_all(b"* BAD no command\r\n")?;
                continue;
            }
        };
        let logged = match cmd.as_str() {
            "LOGIN" | "AUTHENTICATE" => format!("{tag} {cmd} ***"),
            _ => text.to_owned(),
        };
        lock().commands.push(logged);
        let mut reply = String::new();
        let done = match cmd.as_str() {
            "CAPABILITY" => {
                reply.push_str(&format!("* CAPABILITY {}\r\n", caps(lock().caps)));
                "OK done".to_owned()
            }
            "NOOP" => "OK done".to_owned(),
            "LOGOUT" => {
                out.write_all(format!("* BYE bye\r\n{tag} OK done\r\n").as_bytes())?;
                return Ok(());
            }
            "LOGIN" => {
                if w.get(2).map(String::as_str) == Some(USER)
                    && w.get(3).map(String::as_str) == Some(PASSWORD)
                {
                    conn.authed = true;
                    "OK signed in".to_owned()
                } else {
                    "NO [AUTHENTICATIONFAILED] wrong".to_owned()
                }
            }
            "AUTHENTICATE" => {
                let mech = w.get(2).map(|m| m.to_ascii_uppercase()).unwrap_or_default();
                if mech == "PLAIN" && !lock().caps.plain {
                    "NO no such mechanism".to_owned()
                } else {
                    out.write_all(b"+ \r\n")?;
                    let mut data = String::new();
                    input.read_line(&mut data)?;
                    let data = decode64(data.trim_end());
                    let good = match mech.as_str() {
                        "PLAIN" => {
                            data == [b"\0", USER.as_bytes(), b"\0", PASSWORD.as_bytes()].concat()
                        }
                        "XOAUTH2" => {
                            data == format!("user={USER}\x01auth=Bearer {TOKEN}\x01\x01")
                                .into_bytes()
                        }
                        _ => false,
                    };
                    if good {
                        conn.authed = true;
                        "OK signed in".to_owned()
                    } else {
                        if mech == "XOAUTH2" {
                            // The XOAUTH2 error challenge: the client must send an empty line.
                            out.write_all(b"+ eyJzdGF0dXMiOiI0MDAifQ==\r\n")?;
                            let mut empty = String::new();
                            input.read_line(&mut empty)?;
                        }
                        "NO [AUTHENTICATIONFAILED] wrong".to_owned()
                    }
                }
            }
            _ if !conn.authed => "BAD sign in first".to_owned(),
            "ENABLE" => {
                if lock().caps.qresync && w[2..].iter().any(|x| x.eq_ignore_ascii_case("QRESYNC")) {
                    conn.qresync = true;
                    conn.condstore = true;
                    reply.push_str("* ENABLED QRESYNC\r\n");
                }
                "OK done".to_owned()
            }
            "LIST" => {
                for (name, f) in &lock().folders {
                    let attrs = f.special.clone().unwrap_or_default();
                    reply.push_str(&format!(
                        "* LIST (\\HasNoChildren{}{attrs}) \"/\" {}\r\n",
                        if attrs.is_empty() { "" } else { " " },
                        quote(name)
                    ));
                }
                "OK done".to_owned()
            }
            "EXAMINE" => {
                let s = lock();
                match w.get(2).and_then(|n| s.folders.get(n).map(|f| (n, f))) {
                    None => "NO no such folder".to_owned(),
                    Some((name, f)) => {
                        conn.folder = Some(name.clone());
                        if s.caps.condstore
                            && w.get(3)
                                .is_some_and(|p| p.eq_ignore_ascii_case("(CONDSTORE)"))
                        {
                            conn.condstore = true;
                        }
                        let high = f
                            .messages
                            .values()
                            .map(|m| m.modseq)
                            .chain(f.gone.iter().map(|g| g.1))
                            .max()
                            .unwrap_or(1);
                        reply.push_str(&format!(
                            "* FLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft{})\r\n",
                            f.messages
                                .values()
                                .flat_map(|m| m.flags.iter())
                                .filter(|x| !x.starts_with('\\'))
                                .collect::<std::collections::BTreeSet<_>>()
                                .into_iter()
                                .map(|k| format!(" {k}"))
                                .collect::<String>()
                        ));
                        reply.push_str(&format!("* {} EXISTS\r\n", f.messages.len()));
                        reply.push_str(&format!("* OK [UIDVALIDITY {}] ok\r\n", f.uidvalidity));
                        reply.push_str(&format!("* OK [UIDNEXT {}] ok\r\n", f.uidnext));
                        if s.caps.condstore {
                            reply.push_str(&format!("* OK [HIGHESTMODSEQ {high}] ok\r\n"));
                        } else {
                            reply.push_str("* OK [NOMODSEQ] no mod-sequences\r\n");
                        }
                        "OK [READ-ONLY] done".to_owned()
                    }
                }
            }
            "UID" if w.get(2).is_some_and(|x| x.eq_ignore_ascii_case("SEARCH")) => {
                let s = lock();
                match conn.folder.as_ref().and_then(|n| s.folders.get(n)) {
                    None => "BAD no folder".to_owned(),
                    Some(f) => {
                        let key = w.get(3).map(|x| x.to_ascii_uppercase()).unwrap_or_default();
                        let arg = w.get(4).cloned().unwrap_or_default();
                        let has = |m: &Message, flag: &str| {
                            m.flags.iter().any(|x| x.eq_ignore_ascii_case(flag))
                        };
                        let hits: Vec<String> = f
                            .messages
                            .iter()
                            .filter(|(u, m)| match key.as_str() {
                                "UNSEEN" => !has(m, "\\Seen"),
                                "FLAGGED" => has(m, "\\Flagged"),
                                "KEYWORD" => has(m, &arg),
                                "UID" => uid_set(&arg, u32::MAX)
                                    .iter()
                                    .any(|(a, b)| (*a..=*b).contains(u)),
                                _ => false,
                            })
                            .map(|(u, _)| u.to_string())
                            .collect();
                        reply.push_str(
                            &format!("* SEARCH {}\r\n", hits.join(" ")).replace(" \r\n", "\r\n"),
                        );
                        "OK done".to_owned()
                    }
                }
            }
            // `UID FETCH`, and `FETCH` by sequence numbers.
            "UID" | "FETCH"
                if w[1] == "FETCH" || w.get(2).is_some_and(|x| x.eq_ignore_ascii_case("FETCH")) =>
            {
                let by_uid = w[1] == "UID";
                let at = if by_uid { 3 } else { 2 };
                let s = lock();
                match conn.folder.as_ref().and_then(|n| s.folders.get(n)) {
                    None => "BAD no folder".to_owned(),
                    Some(f) => {
                        let items = w
                            .get(at + 1)
                            .map(|x| x.to_ascii_uppercase())
                            .unwrap_or_default();
                        let modifiers = w
                            .get(at + 2)
                            .map(|x| x.to_ascii_uppercase())
                            .unwrap_or_default();
                        let mods = words(modifiers.trim_matches(['(', ')']));
                        let since = mods
                            .iter()
                            .position(|x| x == "CHANGEDSINCE")
                            .and_then(|i| mods.get(i + 1))
                            .and_then(|x| x.parse::<u64>().ok());
                        let vanished = mods.iter().any(|x| x == "VANISHED");
                        let max = if by_uid {
                            f.messages.keys().next_back().copied().unwrap_or(0)
                        } else {
                            f.messages.len() as u32
                        };
                        let ranges = uid_set(w.get(at).map(String::as_str).unwrap_or(""), max);
                        if (since.is_some() && !conn.condstore)
                            || (vanished && (!conn.qresync || since.is_none() || !by_uid))
                        {
                            "BAD modifier not enabled".to_owned()
                        } else if !by_uid && ranges.iter().any(|(a, b)| *a == 0 || *b > max) {
                            // RFC 3501: a sequence number above EXISTS is an error.
                            "BAD no such message".to_owned()
                        } else {
                            let inside =
                                |u: u32| ranges.iter().any(|(a, b)| (*a..=*b).contains(&u));
                            let since = since.unwrap_or(0);
                            if vanished {
                                let gone: Vec<String> = f
                                    .gone
                                    .iter()
                                    .filter(|(u, m)| *m > since && inside(*u))
                                    .map(|(u, _)| u.to_string())
                                    .collect();
                                if !gone.is_empty() {
                                    reply.push_str(&format!(
                                        "* VANISHED (EARLIER) {}\r\n",
                                        gone.join(",")
                                    ));
                                }
                            }
                            let mut bytes = reply.into_bytes();
                            for (seq, (uid, m)) in f.messages.iter().enumerate() {
                                let key = if by_uid { *uid } else { seq as u32 + 1 };
                                if !inside(key) || m.modseq <= since {
                                    continue;
                                }
                                let mut parts = vec![format!("UID {uid}")];
                                if items.contains("INTERNALDATE") {
                                    parts.push(format!("INTERNALDATE \"{DATE}\""));
                                }
                                if items.contains("RFC822.SIZE") {
                                    parts.push(format!("RFC822.SIZE {}", m.raw.len()));
                                }
                                if items.contains("FLAGS") {
                                    parts.push(format!("FLAGS ({})", m.flags.join(" ")));
                                }
                                if conn.condstore {
                                    parts.push(format!("MODSEQ ({})", m.modseq));
                                }
                                let head = format!("* {} FETCH ({}", seq + 1, parts.join(" "));
                                bytes.extend_from_slice(head.as_bytes());
                                if items.contains("HEADER.FIELDS") {
                                    let h = thread_fields(&m.raw);
                                    bytes.extend_from_slice(
                                        format!(" BODY[HEADER.FIELDS (MESSAGE-ID IN-REPLY-TO REFERENCES)] {{{}}}\r\n", h.len()).as_bytes(),
                                    );
                                    bytes.extend_from_slice(&h);
                                }
                                if items.contains("BODY.PEEK[]") {
                                    bytes.extend_from_slice(
                                        format!(" BODY[] {{{}}}\r\n", m.raw.len()).as_bytes(),
                                    );
                                    bytes.extend_from_slice(&m.raw);
                                }
                                bytes.extend_from_slice(b")\r\n");
                            }
                            out.write_all(&bytes)?;
                            reply = String::new();
                            "OK done".to_owned()
                        }
                    }
                }
            }
            _ => "BAD unknown command".to_owned(),
        };
        reply.push_str(&format!("{tag} {done}\r\n"));
        out.write_all(reply.as_bytes())?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_split_quotes_and_lists() {
        assert_eq!(
            words(r#"a1 EXAMINE "In \"x\" \\" (CONDSTORE)"#),
            ["a1", "EXAMINE", r#"In "x" \"#, "(CONDSTORE)"]
        );
    }

    #[test]
    fn uid_set_star_and_reverse() {
        assert_eq!(uid_set("5:*,2", 3), [(3, 5), (2, 2)]);
    }

    #[test]
    fn decode64_plain() {
        assert_eq!(decode64("AHUAcA=="), b"\0u\0p");
    }
}
