//! A synchronous IMAP client for the Iris mirror (spec
//! `docs/specs/2026-11-17-imap-client.md`).
//!
//! [`connect`] opens a TLS connection (port 993). [`Session::start`] signs
//! in on any stream, so that tests use a plain local stream. The client only
//! reads: it opens folders with `EXAMINE` and fetches with `BODY.PEEK[]`, so
//! no call changes the mailbox.
//!
//! Memory: [`Session::fetch_raw`] streams the message to a sink. Each other
//! response is at most [`RESPONSE_LIMIT`] bytes. [`Session::changes`] and
//! [`Session::list`] give each message to a callback and keep no list.
//!
//! The server is not trusted. Each response that does not parse is an
//! error. After any error, drop the session and connect again.

use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use imap_proto::{
    AttributeValue, Capability, MailboxDatum, NameAttribute, Response, ResponseCode, Status,
};

/// The largest response that the client reads into memory. The `BODY[]`
/// literal of [`Session::fetch_raw`] does not count: the client streams it.
pub const RESPONSE_LIMIT: usize = 1024 * 1024;
/// The most bytes of server text in an error.
const TEXT_LIMIT: usize = 200;

#[derive(Debug)]
pub enum Error {
    /// The connection failed, or a read or write timed out.
    Io(io::Error),
    /// The TLS setup failed (for example, a bad host name).
    Tls(String),
    /// A response did not parse, or did not agree with the command.
    Protocol(String),
    /// A response or a line is above [`RESPONSE_LIMIT`].
    TooLarge,
    /// The server refused the sign-in. The person must sign in again.
    SignIn(String),
    /// The server answered `NO` or `BAD` to a command.
    Refused(String),
    /// The server closed the connection (`BYE`).
    Closed,
    /// The server does not have a capability that the call needs.
    Missing(&'static str),
    /// A folder name or a user name has a byte that a quoted string cannot
    /// hold. Nothing was sent.
    Name,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "imap: {e}"),
            Error::Tls(e) => write!(f, "imap: tls: {e}"),
            Error::Protocol(e) => write!(f, "imap: bad response: {e}"),
            Error::TooLarge => write!(f, "imap: response above {RESPONSE_LIMIT} bytes"),
            Error::SignIn(e) => write!(f, "imap: sign-in refused: {e}"),
            Error::Refused(e) => write!(f, "imap: refused: {e}"),
            Error::Closed => write!(f, "imap: the server closed the connection"),
            Error::Missing(c) => write!(f, "imap: the server has no {c}"),
            Error::Name => write!(f, "imap: a name has a byte that IMAP cannot quote"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The sign-in data. The client stores none of it. `Debug` hides the secret.
pub enum Auth<'a> {
    /// An app password of the provider.
    Password { user: &'a str, password: &'a str },
    /// An OAuth access token (`XOAUTH2`).
    OAuth { user: &'a str, token: &'a str },
}

impl fmt::Debug for Auth<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Auth::Password { user, .. } => write!(f, "Password {{ user: {user:?}, .. }}"),
            Auth::OAuth { user, .. } => write!(f, "OAuth {{ user: {user:?}, .. }}"),
        }
    }
}

/// The TLS stream that [`connect`] makes.
pub type Tls = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

/// Open a TLS connection to `host:port`. `timeout` limits the connect and
/// each read and write.
pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<Tls> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no address");
    let mut tcp = None;
    for addr in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => {
                tcp = Some(s);
                break;
            }
            Err(e) => last = e,
        }
    }
    let tcp = tcp.ok_or(last)?;
    tcp.set_read_timeout(Some(timeout))?;
    tcp.set_write_timeout(Some(timeout))?;
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| Error::Tls(e.to_string()))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.to_owned())
        .map_err(|e| Error::Tls(e.to_string()))?;
    let conn = rustls::ClientConnection::new(Arc::new(config), name)
        .map_err(|e| Error::Tls(e.to_string()))?;
    let mut tls = rustls::StreamOwned::new(conn, tcp);
    // Finish the handshake now, so that a server with no TLS fails here.
    while tls.conn.is_handshaking() {
        tls.conn.complete_io(&mut tls.sock)?;
    }
    Ok(tls)
}

/// The special use of a folder (RFC 6154).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Special {
    All,
    Archive,
    Drafts,
    Flagged,
    Junk,
    Sent,
    Trash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// The name as the server sends it (modified UTF-7). Give it back to
    /// [`Session::examine`] as it is.
    pub name: String,
    pub delimiter: Option<String>,
    pub special: Option<Special>,
    /// `false` for `\Noselect` and `\NonExistent`.
    pub selectable: bool,
}

/// The state of a folder after [`Session::examine`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Examined {
    pub uidvalidity: u32,
    pub uidnext: Option<u32>,
    pub exists: u32,
    /// `None` when the server has no `CONDSTORE`, or no mod-sequences for
    /// this folder.
    pub highest_modseq: Option<u64>,
}

/// One message in a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub uid: u32,
    pub flags: Vec<String>,
    pub modseq: Option<u64>,
    /// `INTERNALDATE` as the server sends it, for example
    /// `17-Nov-2026 09:30:00 +0000`.
    pub internal_date: Option<String>,
    /// `RFC822.SIZE`.
    pub size: Option<u32>,
}

/// One event of [`Session::changes`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// A new message, or a message with changed flags.
    Entry(Entry),
    /// Removed UIDs (`VANISHED`, only with `QRESYNC`).
    Vanished(RangeInclusive<u32>),
}

/// A signed-in session.
pub struct Session<S: Read + Write> {
    io: BufReader<S>,
    tag: u32,
    caps: Vec<String>,
    qresync: bool,
}

/// Put `s` in a quoted string, or give [`Error::Name`].
fn quote(s: &str) -> Result<String> {
    if s.bytes()
        .any(|b| b == b'\r' || b == b'\n' || b == 0 || b > 127)
    {
        return Err(Error::Name);
    }
    Ok(format!(
        "\"{}\"",
        s.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// Server text for an error: cut, and only printable ASCII.
fn text(t: Option<&str>) -> String {
    t.unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(TEXT_LIMIT)
        .collect()
}

/// Remove the escapes of a quoted string.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        out.push(if c == '\\' {
            chars.next().unwrap_or(c)
        } else {
            c
        });
    }
    out
}

/// The first literal marker (`{n}` and CRLF) in a response.
fn literal_in(buf: &[u8]) -> Option<usize> {
    buf.windows(3).position(|w| w == b"}\r\n")
}

/// A status response (`OK`, `NO`, `BAD`, `BYE`, `PREAUTH`, or a `+`
/// continuation) with a byte above 127 just after the `]` of its code.
/// `imap-proto` 0.17 panics on it (it cuts the text at byte 1).
fn bad_outcome(buf: &[u8]) -> bool {
    let line = &buf[..buf.iter().position(|b| *b == b'\n').unwrap_or(buf.len())];
    let mut words = line.splitn(3, |b| *b == b' ');
    let first = words.next().unwrap_or_default();
    let rest = if first == b"+" {
        line.get(2..).unwrap_or_default()
    } else {
        let status = words.next().unwrap_or_default();
        if !["OK", "NO", "BAD", "BYE", "PREAUTH"]
            .iter()
            .any(|s| status.eq_ignore_ascii_case(s.as_bytes()))
        {
            return false;
        }
        words.next().unwrap_or_default()
    };
    if !rest.starts_with(b"[") {
        return false;
    }
    rest.iter()
        .position(|b| *b == b']')
        .and_then(|i| rest.get(i + 1))
        .is_some_and(|b| *b > 127)
}

/// The size of the literal at the end of a line, as `{n}\r\n`.
fn literal(line: &[u8]) -> Option<usize> {
    let body = line.strip_suffix(b"}\r\n")?;
    let open = body.iter().rposition(|b| *b == b'{')?;
    let n = &body[open + 1..];
    if n.is_empty() || !n.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(n).ok()?.parse().ok()
}

impl<S: Read + Write> Session<S> {
    /// Read the greeting, sign in, and turn on `QRESYNC` when the server has
    /// it.
    pub fn start(stream: S, auth: Auth<'_>) -> Result<Session<S>> {
        let mut s = Session {
            io: BufReader::new(stream),
            tag: 0,
            caps: Vec::new(),
            qresync: false,
        };
        let greeting = s.read_response(None)?;
        if bad_outcome(&greeting) {
            return Err(Error::Protocol("greeting".into()));
        }
        match Response::parse(&greeting) {
            Ok((
                _,
                Response::Data {
                    status: Status::Ok, ..
                },
            )) => {}
            Ok((
                _,
                Response::Data {
                    status: Status::Bye,
                    ..
                },
            )) => return Err(Error::Closed),
            _ => return Err(Error::Protocol("greeting".into())),
        }
        s.capability()?;
        match auth {
            Auth::Password { user, password } => {
                if s.has("AUTH=PLAIN") {
                    s.authenticate("PLAIN", format!("\0{user}\0{password}").as_bytes())?;
                } else if s.has("LOGINDISABLED") {
                    return Err(Error::Missing("AUTH=PLAIN or LOGIN"));
                } else {
                    let cmd = format!("LOGIN {} {}", quote(user)?, quote(password)?);
                    s.run(&cmd, None, |_, _| Ok(())).map_err(|e| match e {
                        Error::Refused(t) => Error::SignIn(t),
                        e => e,
                    })?;
                }
            }
            Auth::OAuth { user, token } => {
                if !s.has("AUTH=XOAUTH2") {
                    return Err(Error::Missing("AUTH=XOAUTH2"));
                }
                s.authenticate(
                    "XOAUTH2",
                    format!("user={user}\x01auth=Bearer {token}\x01\x01").as_bytes(),
                )?;
            }
        }
        // The capabilities can change after the sign-in.
        s.capability()?;
        if s.has("QRESYNC") {
            let mut on = false;
            s.run("ENABLE QRESYNC", None, |r, _| {
                if let Response::Capabilities(c) = r {
                    on |= c.iter().any(
                        |c| matches!(c, Capability::Atom(a) if a.eq_ignore_ascii_case("QRESYNC")),
                    );
                }
                Ok(())
            })?;
            s.qresync = on;
        }
        Ok(s)
    }

    /// The server has the capability `name` (no case).
    pub fn has(&self, name: &str) -> bool {
        self.caps.iter().any(|c| c.eq_ignore_ascii_case(name))
    }

    /// `CONDSTORE` is usable: [`Session::changes`] works.
    pub fn condstore(&self) -> bool {
        self.qresync || self.has("CONDSTORE")
    }

    /// `QRESYNC` is on: [`Session::changes`] also gives removed UIDs.
    pub fn qresync(&self) -> bool {
        self.qresync
    }

    fn capability(&mut self) -> Result<()> {
        let mut caps = Vec::new();
        self.run("CAPABILITY", None, |r, _| {
            if let Response::Capabilities(c) = r {
                caps = c
                    .iter()
                    .map(|c| match c {
                        Capability::Imap4rev1 => "IMAP4rev1".to_owned(),
                        Capability::Auth(a) => format!("AUTH={a}"),
                        Capability::Atom(a) => a.to_string(),
                    })
                    .collect();
            }
            Ok(())
        })?;
        self.caps = caps;
        Ok(())
    }

    fn authenticate(&mut self, mech: &str, payload: &[u8]) -> Result<()> {
        self.tag += 1;
        let tag = format!("a{}", self.tag);
        self.send(format!("{tag} AUTHENTICATE {mech}\r\n").as_bytes())?;
        let mut sent = false;
        loop {
            let buf = self.read_response(None)?;
            if bad_outcome(&buf) {
                return Err(Error::Protocol("byte above 127 after ]".into()));
            }
            let (_, r) = Response::parse(&buf).map_err(|_| Error::Protocol("sign-in".into()))?;
            match r {
                Response::Continue(_) if !sent => {
                    let mut line = base64::engine::general_purpose::STANDARD
                        .encode(payload)
                        .into_bytes();
                    line.extend_from_slice(b"\r\n");
                    self.send(&line)?;
                    sent = true;
                }
                // An error challenge (XOAUTH2): an empty line ends it, then the server says NO.
                Response::Continue(_) => self.send(b"\r\n")?,
                Response::Done {
                    tag: t,
                    status,
                    outcome,
                } if t.0 == tag => {
                    return match status {
                        Status::Ok => Ok(()),
                        _ => Err(Error::SignIn(text(outcome.information.as_deref()))),
                    };
                }
                Response::Data {
                    status: Status::Bye,
                    ..
                } => return Err(Error::Closed),
                _ => {}
            }
        }
    }

    /// The folders of the account.
    pub fn folders(&mut self) -> Result<Vec<Folder>> {
        let mut out = Vec::new();
        self.run("LIST \"\" \"*\"", None, |r, raw| {
            if let Response::MailboxData(MailboxDatum::List(l)) = r {
                let mut special = None;
                let mut selectable = true;
                for a in &l.name_attributes {
                    match a {
                        NameAttribute::All => special = Some(Special::All),
                        NameAttribute::Archive => special = Some(Special::Archive),
                        NameAttribute::Drafts => special = Some(Special::Drafts),
                        NameAttribute::Flagged => special = Some(Special::Flagged),
                        NameAttribute::Junk => special = Some(Special::Junk),
                        NameAttribute::Sent => special = Some(Special::Sent),
                        NameAttribute::Trash => special = Some(Special::Trash),
                        NameAttribute::NoSelect => selectable = false,
                        NameAttribute::Extension(e) if e.eq_ignore_ascii_case("\\NonExistent") => {
                            selectable = false
                        }
                        _ => {}
                    }
                }
                // The parser keeps the escapes of a quoted string. A name in a literal has none.
                let quoted = literal_in(raw).is_none();
                out.push(Folder {
                    name: if quoted {
                        unescape(&l.name)
                    } else {
                        l.name.to_string()
                    },
                    delimiter: l.delimiter.as_deref().map(str::to_owned),
                    special,
                    selectable,
                });
            }
            Ok(())
        })?;
        Ok(out)
    }

    /// Open a folder read-only.
    pub fn examine(&mut self, folder: &str) -> Result<Examined> {
        let mut cmd = format!("EXAMINE {}", quote(folder)?);
        if self.condstore() {
            cmd.push_str(" (CONDSTORE)");
        }
        let mut e = Examined::default();
        let mut validity = None;
        self.run(&cmd, None, |r, _| {
            match r {
                Response::MailboxData(MailboxDatum::Exists(n)) => e.exists = n,
                Response::Data { outcome, .. } => match outcome.code {
                    Some(ResponseCode::UidValidity(v)) => validity = Some(v),
                    Some(ResponseCode::UidNext(n)) => e.uidnext = Some(n),
                    Some(ResponseCode::HighestModSeq(m)) => e.highest_modseq = Some(m),
                    _ => {}
                },
                _ => {}
            }
            Ok(())
        })?;
        e.uidvalidity = validity.ok_or_else(|| Error::Protocol("no UIDVALIDITY".into()))?;
        if !self.condstore() {
            e.highest_modseq = None;
        }
        Ok(e)
    }

    /// Each message of the open folder with a mod-sequence above `since`,
    /// and with `QRESYNC` each UID removed after `since`.
    pub fn changes(&mut self, since: u64, mut each: impl FnMut(Change)) -> Result<()> {
        if !self.condstore() {
            return Err(Error::Missing("CONDSTORE"));
        }
        let vanished = if self.qresync { " VANISHED" } else { "" };
        let cmd = format!(
            "UID FETCH 1:* (UID FLAGS INTERNALDATE RFC822.SIZE) (CHANGEDSINCE {since}{vanished})"
        );
        self.run(&cmd, None, |r, _| {
            match r {
                Response::Vanished { uids, .. } => {
                    uids.into_iter().for_each(|u| each(Change::Vanished(u)))
                }
                Response::Fetch(_, attrs) => {
                    if let Some(e) = entry(&attrs) {
                        each(Change::Entry(e));
                    }
                }
                _ => {}
            }
            Ok(())
        })
    }

    /// Each message of the open folder with a UID in `from..=to` (`to`
    /// `None` is `*`).
    pub fn list(&mut self, from: u32, to: Option<u32>, mut each: impl FnMut(Entry)) -> Result<()> {
        let to_text = to.map_or("*".to_owned(), |t| t.to_string());
        let cmd = format!("UID FETCH {from}:{to_text} (UID FLAGS INTERNALDATE RFC822.SIZE)");
        let to = to.unwrap_or(u32::MAX);
        self.run(&cmd, None, |r, _| {
            if let Response::Fetch(_, attrs) = r {
                // `n:*` gives the last message also when its UID is below n.
                if let Some(e) = entry(&attrs).filter(|e| (from..=to).contains(&e.uid)) {
                    each(e);
                }
            }
            Ok(())
        })
    }

    /// Stream the RFC 5322 bytes of message `uid` of the open folder to
    /// `sink`, and give the byte count. `Ok(None)`: no message has the UID.
    /// After an error, the sink can hold part of the message: drop it.
    pub fn fetch_raw(&mut self, uid: u32, sink: &mut dyn Write) -> Result<Option<u64>> {
        let mut found = None;
        let mut counter = Counter { sink, n: 0 };
        let cmd = format!("UID FETCH {uid} (UID BODY.PEEK[])");
        self.run(&cmd, Some(&mut counter), |r, _| {
            if let Response::Fetch(_, attrs) = r {
                let has_body = attrs
                    .iter()
                    .any(|a| matches!(a, AttributeValue::BodySection { .. }));
                let this = attrs
                    .iter()
                    .any(|a| matches!(a, AttributeValue::Uid(u) if *u == uid));
                if has_body && !this {
                    return Err(Error::Protocol("BODY[] of another message".into()));
                }
                if has_body {
                    found = Some(());
                }
            }
            Ok(())
        })?;
        Ok(found.map(|_| counter.n))
    }

    /// End the session.
    pub fn logout(mut self) -> Result<()> {
        match self.run("LOGOUT", None, |_, _| Ok(())) {
            Ok(()) | Err(Error::Closed) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        let s = self.io.get_mut();
        s.write_all(bytes)?;
        s.flush()?;
        Ok(())
    }

    /// Send a command and give each untagged response to `on`, until the
    /// tagged answer.
    fn run(
        &mut self,
        cmd: &str,
        mut body: Option<&mut Counter<'_>>,
        mut on: impl FnMut(Response<'_>, &[u8]) -> Result<()>,
    ) -> Result<()> {
        self.tag += 1;
        let tag = format!("a{}", self.tag);
        self.send(format!("{tag} {cmd}\r\n").as_bytes())?;
        loop {
            let buf = self.read_response(body.as_deref_mut())?;
            if bad_outcome(&buf) {
                return Err(Error::Protocol("byte above 127 after ]".into()));
            }
            let r = match Response::parse(&buf) {
                Ok((b"", r)) => r,
                _ => return Err(Error::Protocol(text(Some(&String::from_utf8_lossy(&buf))))),
            };
            match r {
                Response::Done {
                    tag: t,
                    status,
                    outcome,
                } if t.0 == tag => {
                    return match status {
                        Status::Ok => Ok(()),
                        _ => Err(Error::Refused(text(outcome.information.as_deref()))),
                    };
                }
                Response::Done { .. } | Response::Continue(_) => {
                    return Err(Error::Protocol("unexpected answer".into()))
                }
                Response::Data {
                    status: Status::Bye,
                    ..
                } => return Err(Error::Closed),
                r => on(r, &buf)?,
            }
        }
    }

    /// Read one response with its literals. With `body`, the literal after
    /// `BODY[]` goes to the sink, and the buffer gets `{0}` in its place.
    fn read_response(&mut self, mut body: Option<&mut Counter<'_>>) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        loop {
            let start = buf.len();
            let room = RESPONSE_LIMIT.saturating_sub(start) as u64;
            let n = (&mut self.io).take(room + 1).read_until(b'\n', &mut buf)?;
            if buf.len() > RESPONSE_LIMIT {
                return Err(Error::TooLarge);
            }
            if n == 0 || !buf.ends_with(b"\r\n") {
                return Err(Error::Closed);
            }
            let Some(size) = literal(&buf[start..]) else {
                return Ok(buf);
            };
            let open = buf.iter().rposition(|b| *b == b'{').unwrap_or(start);
            let is_body =
                buf[..open].len() >= 7 && buf[open - 7..open].eq_ignore_ascii_case(b"BODY[] ");
            match body.as_deref_mut() {
                Some(sink) if is_body => {
                    buf.truncate(open);
                    buf.extend_from_slice(b"{0}\r\n");
                    let copied = io::copy(&mut (&mut self.io).take(size as u64), sink)?;
                    if copied != size as u64 {
                        return Err(Error::Closed);
                    }
                }
                _ => {
                    if buf.len() + size > RESPONSE_LIMIT {
                        return Err(Error::TooLarge);
                    }
                    let at = buf.len();
                    buf.resize(at + size, 0);
                    self.io.read_exact(&mut buf[at..])?;
                }
            }
        }
    }
}

/// A sink that counts the bytes.
struct Counter<'a> {
    sink: &'a mut dyn Write,
    n: u64,
}

impl Write for Counter<'_> {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let n = self.sink.write(b)?;
        self.n += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.sink.flush()
    }
}

fn entry(attrs: &[AttributeValue<'_>]) -> Option<Entry> {
    let mut uid = None;
    let mut flags = Vec::new();
    let mut modseq = None;
    let mut internal_date = None;
    let mut size = None;
    for a in attrs {
        match a {
            AttributeValue::Uid(u) => uid = Some(*u),
            AttributeValue::Flags(f) => flags = f.iter().map(|x| x.to_string()).collect(),
            AttributeValue::ModSeq(m) => modseq = Some(*m),
            AttributeValue::InternalDate(d) => internal_date = Some(d.to_string()),
            AttributeValue::Rfc822Size(n) => size = Some(*n),
            _ => {}
        }
    }
    // A FETCH with no UID (an unsolicited flag change) is skipped. The next sync finds the change.
    Some(Entry {
        uid: uid?,
        flags,
        modseq,
        internal_date,
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_escapes_and_refuses() {
        assert_eq!(quote(r#"a"b\c"#).unwrap(), r#""a\"b\\c""#);
        for bad in ["a\r", "a\nb", "a\0", "café"] {
            assert!(matches!(quote(bad), Err(Error::Name)), "{bad:?}");
        }
    }

    #[test]
    fn unescape_quoted() {
        assert_eq!(unescape(r#"Odd \"n\" \\x"#), r#"Odd "n" \x"#);
    }

    #[test]
    fn bad_outcome_finds_the_panic_input() {
        assert!(bad_outcome("* OK [UIDNEXT 4]é\r\n".as_bytes()));
        assert!(bad_outcome("a1 NO [ALERT]\u{80}x\r\n".as_bytes()));
        assert!(bad_outcome("+ [ALERT]é\r\n".as_bytes()));
        assert!(!bad_outcome("* OK [UIDNEXT 4] é\r\n".as_bytes()));
        assert!(!bad_outcome("* LIST () \"/\" \"a]é\"\r\n".as_bytes()));
    }

    #[test]
    fn literal_size() {
        assert_eq!(literal(b"* 1 FETCH (BODY[] {12}\r\n"), Some(12));
        assert_eq!(literal(b"* OK {x}\r\n"), None);
        assert_eq!(literal(b"* OK {}\r\n"), None);
        assert_eq!(literal(b"* OK done\r\n"), None);
    }

    #[test]
    fn auth_debug_hides_secret() {
        let a = Auth::Password {
            user: "u",
            password: "hunter2",
        };
        let b = Auth::OAuth {
            user: "u",
            token: "ya29.secret",
        };
        let shown = format!("{a:?} {b:?}");
        assert!(
            !shown.contains("hunter2") && !shown.contains("ya29"),
            "{shown}"
        );
    }
}
