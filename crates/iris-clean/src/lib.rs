//! The clean text of one mail message (specification 1, section 7.2).
//!
//! `clean(raw)` reads the raw RFC 5322 bytes of one message and returns the
//! text that a person reads: MIME and charsets decoded, HTML as Markdown,
//! without quoted history, signatures, hidden HTML and tracking.
//!
//! The functions are pure: no network, no files, no model. Mail text is data.
//! This crate never runs or follows text from a message.
//!
//! The contract (what goes out, what stays, the limits and the known
//! limits) is in `docs/specs/2026-11-16-clean-text.md`.
//!
//! The work has two steps. Step 1 walks the MIME tree and makes the clean
//! text of each body part, until the text has [`TEXT_LIMIT`] characters. It
//! lists the attachments without a decode. Step 2 reads the attachments in
//! order, with one text budget of [`ATTACHMENT_TEXT_LIMIT`] bytes.
//!
//! Bounds for one message, whatever its size:
//! - Each body text part decodes at most [`PART_LIMIT`] bytes.
//! - An attached message gives at most [`INNER_LIMIT`] decoded bytes to the parser.
//! - Only a PDF of at most [`PDF_LIMIT`] bytes is decoded for its text.
//! - The parser reads at most [`MAX_PARTS`] MIME parts, [`MAX_DEPTH`] levels deep.
//! - The body of an attachment is not decoded to measure its size.
//! - A message with MIME fields past [`MIME_FIELD_LIMIT`] is not parsed.
//!
//! So the memory use does not grow with the size of the message, and the
//! time is linear in the size of the message. The largest use is a PDF of
//! 10 MiB with one decompressed stream of at most 4 MiB.

mod html;
mod mime;
mod pdf;
mod quote;
mod url;

use std::borrow::Cow;

use mail_parser::{
    Encoding, HeaderForm, HeaderName, MessageParser, MessagePart, MessageRef, PartFlags,
};

pub use quote::remove_repeated_end;

/// The most characters of clean text for each message.
pub const TEXT_LIMIT: usize = 32_000;
/// The most bytes of attachment text for each message.
pub const ATTACHMENT_TEXT_LIMIT: usize = 1_000_000;
/// The most decoded bytes of one body text part (plain text or HTML).
pub const PART_LIMIT: usize = 2 * 1024 * 1024;
/// The most MIME parts that the parser reads in one message.
pub const MAX_PARTS: usize = 500;
/// The most levels of nested MIME parts.
pub const MAX_DEPTH: usize = 8;
/// The most decoded bytes of an attached message that the parser reads.
pub const INNER_LIMIT: usize = 2 * 1024 * 1024;
/// The largest PDF attachment (decoded bytes) that gives text (spec 1 section 9).
pub const PDF_LIMIT: u64 = 10 * 1024 * 1024;
/// The least bytes of PDF text that the quality check reads, whatever the
/// budget that is left.
const PDF_CHECK_TEXT: usize = 4096;
/// The most lines of one body part that the quote and signature steps read.
/// More lines are cut. 2 MiB of lines of 20 bytes is about 100,000 lines.
pub const MAX_LINES: usize = 100_000;
/// The most characters of an attachment name.
const NAME_LIMIT: usize = 255;

/// The mark at the end of a text that was cut.
pub const CUT_MARK: &str = "\n\n[Iris cut the text here.]";

/// The clean text of one message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clean {
    /// The decoded `Subject` header.
    pub subject: String,
    /// The clean body text, at most [`TEXT_LIMIT`] characters plus [`CUT_MARK`].
    pub text: String,
    /// True if text went out because of a limit. Then `text` ends with [`CUT_MARK`].
    pub cut: bool,
    pub attachments: Vec<Attachment>,
    /// True if a `text/calendar` part of the top message is a request
    /// (spec 1 section 7.3): its `method` parameter is `REQUEST`, or its
    /// decoded body has a `METHOD:REQUEST` line.
    pub invitation: bool,
    /// Each HTML link of the body whose text is not its target, in order
    /// (the link check of spec 1 section 6.7). A version of the body that
    /// the text does not use adds its links too: a mail client can show it.
    pub links: Vec<Link>,
    /// True if links went out: past [`LINK_BYTES`], or past the output
    /// limit of an HTML part.
    pub links_cut: bool,
}

/// One HTML link whose text is not its target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// The link text as the clean text shows it, trimmed. It can have lines.
    pub text: String,
    /// The `href` as the HTML gives it, with its entities decoded, after
    /// [`browser_href`].
    pub href: String,
}

/// True when `rest`, the part of a message past a cut, has a MIME part
/// header with the type `text/html`: a mail client can show an HTML part
/// there. `before` is the message from its start up to the cut: the scan
/// reads it first, for the boundaries that the headers declare and to know
/// if the cut is in a header or in a body. Its HTML parts do not count.
///
/// The message header starts the scan. A part header starts after a
/// delimiter line of a declared boundary (`--` and the boundary) and ends
/// at an empty line. `text/html` in a part body (an attachment) does not
/// count, also after a line that starts with `--` but is not a delimiter.
/// Folded header lines count. Case and white space in the type do not
/// matter, and boundaries match with no case, so the scan finds a part
/// header in more places, not fewer. One pass that keeps at most
/// `LINE_BYTES` of each line and `MAX_BOUNDARIES` boundaries, so the
/// memory does not grow with the input.
pub fn html_part_in(before: &[u8], mut rest: impl std::io::Read) -> std::io::Result<bool> {
    let mut scan = PartScan {
        header: true,
        value: None,
        line: Vec::with_capacity(PartScan::LINE_BYTES),
        bounds: Vec::new(),
    };
    for &b in before {
        scan.byte(b);
    }
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = rest.read(&mut buf)?;
        if n == 0 {
            // The end of the input also ends the open header field.
            let last = scan.end_line();
            return Ok(last || scan.end_field());
        }
        if buf[..n].iter().any(|&b| scan.byte(b)) {
            return Ok(true);
        }
    }
}

/// The state of [`html_part_in`].
struct PartScan {
    /// True in a header: the message header, or a part header.
    header: bool,
    /// The `Content-Type` value of the open header field, lowercase, with
    /// its folded lines, at most `LINE_BYTES`.
    value: Option<Vec<u8>>,
    /// The first bytes of the open line, lowercase.
    line: Vec<u8>,
    /// The declared boundaries, lowercase.
    bounds: Vec<Vec<u8>>,
}

impl PartScan {
    /// The longest header line of RFC 5322, with its line end.
    const LINE_BYTES: usize = 1000;
    const MAX_BOUNDARIES: usize = 64;

    /// Reads one byte. True when a header field ends at it with the type
    /// `text/html`.
    fn byte(&mut self, b: u8) -> bool {
        if b == b'\n' {
            return self.end_line();
        }
        if self.line.len() < Self::LINE_BYTES {
            self.line.push(b.to_ascii_lowercase());
        }
        false
    }

    /// Ends the open line. True when it ends a header field with the type
    /// `text/html`.
    fn end_line(&mut self) -> bool {
        let line = std::mem::take(&mut self.line);
        let text = line.strip_suffix(b"\r").unwrap_or(&line);
        let mut html = false;
        if self.header && matches!(text.first(), Some(b' ' | b'\t')) {
            if let Some(v) = &mut self.value {
                v.extend_from_slice(text);
                v.truncate(Self::LINE_BYTES);
            }
        } else {
            html = self.end_field();
            if self.delimiter(text) {
                self.header = true;
            } else if text.is_empty() {
                self.header = false;
            } else if self.header {
                if let Some(v) = text.strip_prefix(b"content-type:") {
                    self.value = Some(v.to_vec());
                }
            }
        }
        self.line = line;
        self.line.clear();
        html
    }

    /// Ends the open `Content-Type` field: keeps its boundary, and is true
    /// for the type `text/html`.
    fn end_field(&mut self) -> bool {
        let Some(v) = self.value.take() else {
            return false;
        };
        let kind: Vec<u8> = v
            .iter()
            .copied()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
        if kind.starts_with(b"multipart/") && self.bounds.len() < Self::MAX_BOUNDARIES {
            if let Some(b) = boundary(&v) {
                self.bounds.push(b);
            }
        }
        kind.starts_with(b"text/html")
    }

    /// True for a delimiter line of a declared boundary: `--`, the
    /// boundary, then `--` or nothing, then only white space.
    fn delimiter(&self, text: &[u8]) -> bool {
        let Some(t) = text.strip_prefix(b"--") else {
            return false;
        };
        self.bounds.iter().any(|b| {
            t.strip_prefix(b.as_slice()).is_some_and(|r| {
                let r = r.strip_prefix(b"--").unwrap_or(r);
                r.iter().all(u8::is_ascii_whitespace)
            })
        })
    }
}

/// The `boundary` parameter of a lowercase `Content-Type` value: quoted or
/// a token. `None` if there is none, or if it is empty.
fn boundary(value: &[u8]) -> Option<Vec<u8>> {
    let at = value.windows(9).position(|w| w == b"boundary=")? + 9;
    let rest = &value[at..];
    let b = match rest.strip_prefix(b"\"") {
        Some(q) => q.split(|&c| c == b'"').next()?,
        None => rest
            .split(|c| *c == b';' || c.is_ascii_whitespace())
            .next()?,
    };
    (!b.is_empty()).then(|| b.to_vec())
}

/// An href as the URL standard reads it before the parse: no control
/// characters or spaces at the ends, and no tab or line end inside.
pub fn browser_href(href: &str) -> String {
    href.trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect()
}

/// The most bytes of link texts and hrefs in [`Clean::links`].
pub const LINK_BYTES: usize = 1024 * 1024;

/// One attachment: a name, a type and a size, and the text if there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    /// The file name. Empty if the message gives none.
    pub name: String,
    /// The lowercase MIME type, for example `application/pdf`.
    pub mime_type: String,
    /// The decoded size in bytes.
    pub size: u64,
    /// The text of a plain-text attachment, an attached message or a simple PDF.
    pub text: Option<String>,
}

/// Make the clean text of one raw RFC 5322 message.
pub fn clean(raw: &[u8]) -> Clean {
    clean_cut(raw, false)
}

/// [`clean`] for the first bytes of a message: `raw_cut` is true when
/// `raw` is a prefix of a longer message. Then the link list is cut
/// (`links_cut`) when an HTML body part reaches the end of `raw`. An HTML
/// part that starts past the cut is the work of the caller
/// ([`html_part_in`]).
pub fn clean_cut(raw: &[u8], raw_cut: bool) -> Clean {
    let mut parts = 0;
    let mut attach_left = ATTACHMENT_TEXT_LIMIT;
    if !mime_fields_fit(raw) {
        return Clean {
            subject: String::new(),
            text: CUT_MARK.to_string(),
            cut: true,
            attachments: Vec::new(),
            invitation: false,
            // Iris could not read the message, so it cannot check its links.
            links: Vec::new(),
            links_cut: true,
        };
    }
    let Some(parsed) = parser().parse(raw) else {
        return Clean {
            subject: String::new(),
            text: String::new(),
            cut: false,
            attachments: Vec::new(),
            invitation: false,
            // Iris could not read the message, so it cannot check its links.
            links: Vec::new(),
            links_cut: true,
        };
    };
    let invitation = calendar_request(parsed.root().root_part(), 0, &mut 0);
    let at_end = raw_cut.then_some(raw.len());
    let (subject, text, cut, files, links) = message(parsed.root(), 0, &mut parts, at_end);
    let mut attachments = Vec::new();
    for f in files {
        attachment(f, &mut parts, &mut attach_left, &mut attachments);
    }
    Clean {
        subject,
        text,
        cut,
        attachments,
        invitation,
        links_cut: links.cut,
        links: links.list,
    }
}

/// The links of one message, inside [`LINK_BYTES`].
#[derive(Default)]
struct Links {
    list: Vec<Link>,
    bytes: usize,
    cut: bool,
}

impl Links {
    fn add(&mut self, found: Vec<(String, String)>) {
        for (text, href) in found {
            let size = text.len() + href.len();
            if self.cut || self.bytes + size > LINK_BYTES {
                self.cut = true;
                return;
            }
            self.bytes += size;
            self.list.push(Link { text, href });
        }
    }
}

/// The most MIME parts that `mail-parser` reads in one parse. It parses an
/// attached message that is not transfer-encoded in the same parse, so its
/// parts count here too. The walker applies [`MAX_PARTS`] by itself and does
/// not count the parts of an attached message in step 1, so an attached
/// message of up to this many parts leaves the outer text.
pub const PARSE_PARTS: usize = 10_000;

/// The most bytes of all MIME fields (`Content-Type`, `Content-Disposition`
/// and `Content-Transfer-Encoding`) of one message, with their folded lines.
pub const MIME_FIELD_LIMIT: usize = 1024 * 1024;

/// True if the MIME fields of a message fit the limits: each field at most
/// [`VALUE_LIMIT`] bytes, all of them at most [`MIME_FIELD_LIMIT`] bytes.
/// `mail-parser` always parses these fields in full, and their decoded
/// parameters cost about 2.3 bytes for each byte. So a message that does not
/// fit is not parsed at all (fail closed), and the parse memory has a fixed
/// bound. The scan reads lines only: a body line that looks like a MIME field
/// counts too, so the check can only refuse more, never less.
fn mime_fields_fit(raw: &[u8]) -> bool {
    const NAMES: [&[u8]; 3] = [
        b"content-type:",
        b"content-disposition:",
        b"content-transfer-encoding:",
    ];
    let (mut field, mut total, mut inside) = (0usize, 0usize, false);
    for line in raw.split(|&b| b == b'\n') {
        let len = line.len() + 1;
        if inside && line.first().is_some_and(|&b| b == b' ' || b == b'\t') {
            field += len;
        } else {
            inside = NAMES
                .iter()
                .any(|n| line.len() >= n.len() && line[..n.len()].eq_ignore_ascii_case(n));
            field = if inside { len } else { 0 };
        }
        if inside {
            total += len;
            if field > VALUE_LIMIT || total > MIME_FIELD_LIMIT {
                return false;
            }
        }
    }
    true
}

/// The parser of the MIME structure. Transfer-encoded attached messages are
/// not parsed here: step 2 decodes at most [`INNER_LIMIT`] bytes of each one.
fn parser() -> MessageParser {
    let p = MessageParser::new()
        .max_parts(PARSE_PARTS)
        .max_depth(MAX_DEPTH + 2)
        .max_encoded_nesting(0);
    // Iris parses these fields itself, from at most VALUE_LIMIT bytes (see
    // `field`). A parse of the whole value costs memory: 8.5 MB for a 5 MB subject.
    RAW_FIELDS
        .into_iter()
        .fold(p, |p, name| p.header(name, HeaderForm::Raw))
}

/// The fields that the parser keeps raw. It always parses the MIME fields.
const RAW_FIELDS: [HeaderName<'static>; 33] = [
    HeaderName::Subject,
    HeaderName::From,
    HeaderName::To,
    HeaderName::Cc,
    HeaderName::Bcc,
    HeaderName::Date,
    HeaderName::ReplyTo,
    HeaderName::Sender,
    HeaderName::Comments,
    HeaderName::InReplyTo,
    HeaderName::Keywords,
    HeaderName::Received,
    HeaderName::MessageId,
    HeaderName::References,
    HeaderName::ReturnPath,
    HeaderName::ContentDescription,
    HeaderName::ContentId,
    HeaderName::ContentLanguage,
    HeaderName::ContentLocation,
    HeaderName::ListArchive,
    HeaderName::ListHelp,
    HeaderName::ListId,
    HeaderName::ListOwner,
    HeaderName::ListPost,
    HeaderName::ListSubscribe,
    HeaderName::ListUnsubscribe,
    HeaderName::ResentBcc,
    HeaderName::ResentCc,
    HeaderName::ResentDate,
    HeaderName::ResentFrom,
    HeaderName::ResentMessageId,
    HeaderName::ResentSender,
    HeaderName::ResentTo,
];

/// The most header fields of one entity that Iris reads.
const MAX_HEADERS: usize = 1000;
/// The most bytes of the header block of one entity that Iris reads.
const HEAD_LIMIT: usize = 256 * 1024;
/// The most bytes of one header value that Iris reads.
const VALUE_LIMIT: usize = 16 * 1024;

/// The raw value of the first field `name` of a part, cut at [`VALUE_LIMIT`]
/// bytes. Only the first [`MAX_HEADERS`] fields that start in the first
/// [`HEAD_LIMIT`] bytes of the header block count. Each field of a part that
/// Iris reads goes through this function.
fn field<'m>(part: MessagePart<'m>, name: HeaderName) -> Option<&'m [u8]> {
    let start = part.offset_header();
    part.headers()
        .iter()
        .take(MAX_HEADERS)
        .take_while(|h| ((h.offset_field() - start) as usize) < HEAD_LIMIT)
        .find(|h| h.name() == name)
        .map(|h| {
            let v = h.raw_value();
            &v[..v.len().min(VALUE_LIMIT)]
        })
}

/// Step 1 for one message: the top message, or an attached
/// `message/rfc822` part. Return the subject, the text (with [`CUT_MARK`]
/// if it was cut), the cut flag and the attachments, not read yet. `parts`
/// counts the MIME parts of the whole top message.
fn message<'a>(
    m: MessageRef<'a>,
    depth: usize,
    parts: &mut usize,
    at_end: Option<usize>,
) -> (String, String, bool, Vec<File<'a>>, Links) {
    let root = m.root_part();
    let subject = field(root, HeaderName::Subject).map_or(String::new(), |raw| {
        let parsed = HeaderForm::Text.parse(raw);
        let text = parsed.value().as_text().unwrap_or("").trim().to_string();
        text
    });
    // Some clients send a reply without reply headers: the subject shows it.
    let re = subject
        .split_once(':')
        .is_some_and(|(p, _)| REPLY_PREFIXES.contains(&p.trim().to_lowercase().as_str()));
    let is_reply = re
        || field(root, HeaderName::InReplyTo).is_some()
        || field(root, HeaderName::References).is_some();
    // Step 1: the body text, and the list of the attachments (not decoded).
    let mut w = Walker {
        parts: *parts,
        texts: Vec::new(),
        chars: 0,
        cut: false,
        dropped: 0,
        files: Vec::new(),
        is_reply,
        links: Links::default(),
        at_end,
        links_only: false,
    };
    w.entity(root, depth);
    *parts = w.parts;
    let mut text = w.texts.join("\n\n");
    if w.cut {
        text.push_str(CUT_MARK);
    }
    (subject, text, w.cut, w.files, w.links)
}

/// True if a `text/calendar` part in the MIME tree of `part` is a request.
/// The check visits every part in the tree, in any order and apart from the
/// choice of the text, inside the limits of the walker ([`MAX_PARTS`],
/// [`MAX_DEPTH`]). It does not go into an attached message: a calendar in
/// an attached message is not an invitation to the user.
fn calendar_request(part: MessagePart<'_>, depth: usize, seen: &mut usize) -> bool {
    *seen += 1;
    if *seen > MAX_PARTS || depth > MAX_DEPTH {
        return false;
    }
    let ct_field = field(part, HeaderName::ContentType).map(|v| HeaderForm::ContentType.parse(v));
    let Some(ct) = ct_field.as_ref().and_then(|f| f.value().as_content_type()) else {
        return false;
    };
    let (t, sub) = (ct.ctype(), ct.subtype().unwrap_or(""));
    if t.eq_ignore_ascii_case("multipart") {
        return part.is_multipart()
            && part
                .children()
                .any(|child| calendar_request(child, depth + 1, seen));
    }
    if !(t.eq_ignore_ascii_case("text") && sub.eq_ignore_ascii_case("calendar")) {
        return false;
    }
    if ct
        .attribute("method")
        .is_some_and(|m| m.trim().eq_ignore_ascii_case("request"))
    {
        return true;
    }
    let cte = field(part, HeaderName::ContentTransferEncoding)
        .map(|v| String::from_utf8_lossy(v).trim().to_ascii_lowercase());
    let enc = match cte.as_deref() {
        Some("base64") => Encoding::Base64,
        Some("quoted-printable") => Encoding::QuotedPrintable,
        _ => Encoding::None,
    };
    // The whole body, decoded in chunks of about 64 KiB, through a matcher
    // of constant memory.
    let mut m = MethodLine::default();
    let body = part.raw_body();
    match enc {
        Encoding::Base64 => base64_chunks(body, |b| m.feed(b)),
        Encoding::QuotedPrintable => qp_chunks(body, |b| {
            m.feed(b);
            !m.found
        }),
        Encoding::None => m.feed(body),
    }
    m.feed(b"\n");
    m.found
}

/// Finds a line that is `METHOD:REQUEST` (any case, white space around it)
/// in bytes that come in parts. It keeps at most 14 bytes of a line.
#[derive(Default)]
struct MethodLine {
    /// The bytes of the line after its leading white space.
    line: Vec<u8>,
    /// The line has white space after its first word.
    space: bool,
    /// The line can no longer match.
    over: bool,
    found: bool,
}

impl MethodLine {
    const WORD: &'static [u8] = b"METHOD:REQUEST";

    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.found {
                return;
            }
            if b == b'\n' {
                self.found = !self.over && self.line.eq_ignore_ascii_case(Self::WORD);
                (self.space, self.over) = (false, false);
                self.line.clear();
            } else if b.is_ascii_whitespace() {
                self.space |= !self.line.is_empty();
            } else if self.space || self.line.len() == Self::WORD.len() {
                self.over = true;
            } else {
                self.line.push(b);
            }
        }
    }
}

/// An attachment that step 1 found. Step 2 reads it.
struct File<'a> {
    mime_type: String,
    name: Option<String>,
    charset: Option<String>,
    part: MessagePart<'a>,
    /// The transfer encoding, from the fields that [`field`] reads.
    enc: Encoding,
    /// The MIME depth of the part. An attached message goes one level deeper.
    depth: usize,
}

/// Step 1 of one message: walk the MIME tree, make the text of each body
/// part, and list the attachments.
struct Walker<'a> {
    parts: usize,
    /// The clean text of each body part that has text, in order.
    texts: Vec<String>,
    /// The characters in `texts`. At most [`TEXT_LIMIT`].
    chars: usize,
    /// True if body text went out because of a limit.
    cut: bool,
    /// The count of parts with clean text that did not fit in [`TEXT_LIMIT`].
    dropped: usize,
    files: Vec<File<'a>>,
    /// True if the message is a reply. Quoted history then goes out.
    is_reply: bool,
    /// The links of each HTML part that the walker reads.
    links: Links,
    /// The length of the raw message when it is a prefix of a longer
    /// message ([`clean_cut`]).
    at_end: Option<usize>,
    /// True while the walker reads a version of `multipart/alternative`
    /// that the text does not use: only its links count.
    links_only: bool,
}

impl<'a> Walker<'a> {
    fn entity(&mut self, part: MessagePart<'a>, depth: usize) {
        self.parts += 1;
        // Each limit that skips bytes of the message can skip a link too.
        if self.parts > MAX_PARTS || depth > MAX_DEPTH {
            self.cut = true;
            self.links.cut = true;
            return;
        }
        if part.flags().contains(PartFlags::LIMIT_REACHED) {
            self.cut = true;
            self.links.cut = true;
        }
        let ct_field =
            field(part, HeaderName::ContentType).map(|v| HeaderForm::ContentType.parse(v));
        let ct = ct_field.as_ref().and_then(|f| f.value().as_content_type());
        let disp_field =
            field(part, HeaderName::ContentDisposition).map(|v| HeaderForm::ContentType.parse(v));
        let disp = disp_field
            .as_ref()
            .and_then(|f| f.value().as_content_type());
        let mime_type = match ct.as_ref().and_then(|c| Some((c.ctype(), c.subtype()?))) {
            Some((t, s)) => format!("{t}/{s}").to_ascii_lowercase(),
            None => "text/plain".to_string(),
        };
        let attachment = disp.as_ref().is_some_and(|d| d.is_attachment());
        let name = disp
            .as_ref()
            .and_then(|d| d.attribute("filename"))
            .or_else(|| ct.as_ref()?.attribute("name"))
            .map(str::to_string);
        let charset = ct
            .as_ref()
            .and_then(|c| c.attribute("charset"))
            .map(str::to_string);
        let cte = field(part, HeaderName::ContentTransferEncoding)
            .map(|v| String::from_utf8_lossy(v).trim().to_ascii_lowercase());
        let enc = match cte.as_deref() {
            Some("base64") => Encoding::Base64,
            Some("quoted-printable") => Encoding::QuotedPrintable,
            _ => Encoding::None,
        };
        if let Some(sub) = mime_type
            .strip_prefix("multipart/")
            // A named multipart (for example a saved web page) is a file.
            .filter(|_| !attachment && name.is_none())
        {
            if !part.is_multipart() {
                return;
            }
            // One part more than the limit, so that the walker sees the overflow.
            // Parts past the limit are not read. A last alternative can be one of them.
            let max = MAX_PARTS.saturating_sub(self.parts) + 1;
            let list: Vec<MessagePart<'a>> = part.children().take(max).collect();
            self.cut |= part.children().len() > max;
            self.links.cut |= part.children().len() > max;
            if sub == "alternative" {
                self.alternative(list, depth);
            } else {
                for child in list {
                    self.entity(child, depth + 1);
                }
            }
            return;
        }
        let is_text = mime_type == "text/plain" || mime_type == "text/html";
        if is_text && !attachment && name.is_none() {
            self.body_part(part, enc, mime_type == "text/html", charset.as_deref());
            return;
        }
        if self.links_only {
            return;
        }
        self.files.push(File {
            mime_type,
            name,
            charset,
            part,
            enc,
            depth,
        });
    }

    /// RFC 2046: the last version is the richest. A mail client shows it, so
    /// the clean text uses it too. The walker reads the versions from the
    /// end and uses the first one with text after the quote and signature
    /// steps. A version that is not used adds no text, no attachment and no cut.
    ///
    /// Each version adds its links, also a version that the text does not
    /// use: a mail client can show any version.
    fn alternative(&mut self, list: Vec<MessagePart<'a>>, depth: usize) {
        let mut versions = list.into_iter().rev();
        while let Some(part) = versions.next() {
            let before = (self.texts.len(), self.chars, self.cut, self.files.len());
            let dropped = self.dropped;
            self.entity(part, depth + 1);
            // A version with text is used, also when the text is full and
            // its text did not fit: its cut stays.
            if self.texts.len() > before.0 || self.dropped > dropped {
                let (cut, only) = (self.cut, self.links_only);
                self.links_only = true;
                for rest in versions {
                    self.entity(rest, depth + 1);
                }
                (self.cut, self.links_only) = (cut, only);
                return;
            }
            let texts;
            (texts, self.chars, self.cut, _) = before;
            self.texts.truncate(texts);
            self.files.truncate(before.3);
        }
    }

    /// Make the clean text of one body part. Each part has its own quotes
    /// and signature, so a cut in one part cannot remove a later part.
    fn body_part(
        &mut self,
        part: MessagePart<'a>,
        enc: Encoding,
        html: bool,
        charset: Option<&str>,
    ) {
        // Also when the text is full, the part goes through the steps: only
        // a later part with clean text makes a cut.
        let (bytes, truncated) = decode_capped(part.raw_body(), enc, PART_LIMIT);
        self.cut |= truncated;
        let reaches_end = self
            .at_end
            .is_some_and(|end| part.offset_end() as usize >= end);
        if html && (truncated || reaches_end) {
            self.links.cut = true;
        }
        if self.links_only {
            if html {
                let (_, full, links) = html::convert(&mime::decode(&bytes, charset), self.is_reply);
                self.links.add(links);
                self.links.cut |= full;
            }
            return;
        }
        let text = mime::decode(&bytes, charset);
        drop(bytes);
        let text = if html {
            let (md, full, links) = html::convert(&text, self.is_reply);
            self.cut |= full;
            self.links.add(links);
            // The links after the output limit are not in the list.
            self.links.cut |= full;
            md
        } else {
            text
        };
        let mut text = text.replace("\r\n", "\n").replace('\r', "\n");
        // The quote and signature steps keep a list of the lines. Bound it.
        // The end of line MAX_LINES: a cut only if text follows it.
        if let Some((i, _)) = text
            .match_indices('\n')
            .nth(MAX_LINES - 1)
            .filter(|&(i, _)| i + 1 < text.len())
        {
            text.truncate(i);
            self.cut = true;
        }
        let text = quote::strip_signature(&quote::strip_quotes(&text, self.is_reply));
        let mut text = tidy(&url::clean_urls(&text));
        if text.is_empty() {
            return;
        }
        // The texts join with an empty line: two characters.
        let sep = if self.texts.is_empty() { 0 } else { 2 };
        let Some(left) = TEXT_LIMIT.checked_sub(self.chars + sep).filter(|&l| l > 0) else {
            self.chars = TEXT_LIMIT;
            self.cut = true;
            self.dropped += 1;
            return;
        };
        if let Some((i, _)) = text.char_indices().nth(left) {
            text.truncate(i);
            self.cut = true;
        }
        self.chars += sep + text.chars().count();
        self.texts.push(text);
    }
}

/// Step 2 for one attachment: its name, type, size and text. The text uses
/// `attach_left`, the one budget of the top message, in the order of the
/// message. So the budget never has to go back. An attached message adds its
/// own attachments after it: its text uses the budget first.
fn attachment(f: File, parts: &mut usize, attach_left: &mut usize, out: &mut Vec<Attachment>) {
    let body = f.part.raw_body();
    let enc = f.enc;
    let size = decoded_len(body, enc);
    let mut name = f.name.unwrap_or_default();
    let mut text = None;
    // True if the attachment text is not complete.
    let mut cut = false;
    let mut inner_attachments = Vec::new();
    if f.mime_type == "message/rfc822" {
        // Also with no budget left, the walk lists the attachments of the
        // attached message.
        let had_budget = *attach_left > 0;
        let (inner, clipped) = decode_capped(body, enc, INNER_LIMIT);
        // The first parse saw an attached message that is not encoded, but
        // its bytes past INNER_LIMIT must not count. So parse the capped bytes again.
        let fits = mime_fields_fit(&inner);
        let Some(parsed) = fits.then(|| parser().parse(&inner[..])).flatten() else {
            out.push(Attachment {
                name: String::new(),
                mime_type: f.mime_type,
                size,
                // A message that Iris does not parse because of a limit is cut.
                text: had_budget.then(|| {
                    if fits {
                        String::new()
                    } else {
                        CUT_MARK.to_string()
                    }
                }),
            });
            return;
        };
        // shortcut: the links of an attached message do not count; count
        // them if phishing comes as an attached message.
        let (subject, mut t, inner_cut, files, _) =
            message(parsed.root(), f.depth + 1, parts, None);
        // The cut mark is not part of the budget: take it off, and put one
        // mark on below.
        if inner_cut {
            t.truncate(t.len() - CUT_MARK.len());
        }
        cut = inner_cut || clipped;
        if t.len() > *attach_left {
            t.truncate(floor_char(&t, *attach_left));
            cut = true;
        }
        *attach_left -= t.len();
        // The end of a clipped message is not read, so the sizes of its own
        // attachments are not known. Do not list them.
        if !clipped {
            for inner_file in files {
                attachment(inner_file, parts, attach_left, &mut inner_attachments);
            }
        }
        if name.is_empty() {
            name = subject;
        }
        // A spent budget gives no text, as for the other types.
        text = had_budget.then_some(t);
    } else if f.mime_type == "text/plain" && *attach_left > 0 {
        // UTF-16 gives one byte of text for two source bytes of ASCII.
        let wide = mime::bytes_per_byte(f.charset.as_deref());
        let (bytes, truncated) = decode_capped(body, enc, *attach_left * wide);
        let mut t = mime::decode(&bytes, f.charset.as_deref());
        cut = truncated;
        // A charset can make the text longer than its bytes.
        if t.len() > *attach_left {
            t.truncate(floor_char(&t, *attach_left));
            cut = true;
        }
        *attach_left -= t.len();
        text = Some(t);
    } else if f.mime_type == "application/pdf" && *attach_left > 0 && size <= PDF_LIMIT {
        // Check the marker first, so that a file that is not a PDF costs no memory.
        if pdf::is_pdf(&decode_capped(body, enc, 1024).0) {
            let (bytes, _) = decode_capped(body, enc, PDF_LIMIT as usize);
            // The quality check needs some text, also when little budget is
            // left. One byte more than the budget shows a cut.
            let (found, stopped) = pdf::text_cut(&bytes, (*attach_left + 1).max(PDF_CHECK_TEXT));
            if let Some(mut t) = found {
                cut = stopped;
                if t.len() > *attach_left {
                    t.truncate(floor_char(&t, *attach_left));
                    cut = true;
                }
                *attach_left -= t.len();
                text = Some(t);
            }
        }
    }
    if cut {
        if let Some(t) = text.as_mut() {
            t.push_str(CUT_MARK);
        }
    }
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(NAME_LIMIT)
        .collect();
    out.push(Attachment {
        name: name.trim().to_string(),
        mime_type: f.mime_type,
        size,
        text,
    });
    out.extend(inner_attachments);
}

/// The decoded size of a body. It uses the same chunks as the decode (see
/// [`base64_chunks`] and [`qp_chunks`]), so the size and the decode agree.
fn decoded_len(body: &[u8], enc: Encoding) -> u64 {
    match enc {
        Encoding::Base64 => {
            let mut n = 0u64;
            base64_chunks(body, |b| n += b.len() as u64);
            n
        }
        // `mail-parser` counts a final `=4` as 1 byte but decodes 2 bytes.
        Encoding::QuotedPrintable => {
            let mut n = 0u64;
            qp_chunks(body, |b| {
                n += b.len() as u64;
                true
            });
            n
        }
        Encoding::None => body.len() as u64,
    }
}

/// Decode quoted-printable with the `mail-parser` decoder, in chunks of at
/// most [`QP_CHUNK`] + 2 bytes, and give each decoded chunk to `f` until it
/// returns false. Each chunk ends at a boundary where the decode keeps no
/// state (see [`qp_chunk_end`]), so the chunks decode as the whole body does.
fn qp_chunks(body: &[u8], mut f: impl FnMut(&[u8]) -> bool) {
    let mut start = 0;
    while start < body.len() {
        let end = qp_chunk_end(body, start);
        if !f(&Encoding::QuotedPrintable.decode(&body[start..end])) {
            return;
        }
        start = end;
    }
}

/// The encoded bytes of one quoted-printable chunk, before its boundary.
const QP_CHUNK: usize = 64 * 1024;

/// The end of the chunk that starts at `start`, a boundary.
///
/// A boundary is after a line end, or else after the last token in the
/// first [`QP_CHUNK`] bytes that leaves no state: an escape `=XY` (two hex
/// digits), a pair `=X` that the decoder keeps as it is (`X` is not white
/// space, CR or LF), or a byte that is not `=`, white space or CR. After
/// white space, a CR or `=` with white space, the decode depends on the
/// next bytes (a line end deletes white space and makes a soft break).
fn qp_chunk_end(body: &[u8], start: usize) -> usize {
    let limit = body.len().min(start + QP_CHUNK);
    if let Some(n) = body[start..limit].iter().position(|&b| b == b'\n') {
        return start + n + 1;
    }
    if limit == body.len() {
        return limit;
    }
    let hex = |i: usize| body.get(i).is_some_and(u8::is_ascii_hexdigit);
    let (mut i, mut safe) = (start, None);
    while i < limit {
        let (len, keeps_no_state) = match body[i] {
            b'=' if hex(i + 1) && hex(i + 2) => (3, true),
            b'=' => (
                2,
                !matches!(body.get(i + 1), Some(b' ' | b'\t' | b'\r' | b'\n') | None),
            ),
            b' ' | b'\t' | b'\r' => (1, false),
            _ => (1, true),
        };
        i += len;
        if keeps_no_state {
            safe = Some(i.min(body.len()));
        }
    }
    // shortcut: a line of more than QP_CHUNK bytes of only white space, CR
    // and `=` with white space splits at the limit, so its white space can
    // differ from the whole decode; upgrade if real mail has such lines (#90).
    safe.unwrap_or(limit)
}

/// Decode base64 and give the bytes to `f`, in parts. The `mail-parser`
/// decoder allocates 3/4 of its input, white space too. So it gets only the
/// alphabet and the padding, in chunks of 64 KiB (a multiple of 4).
fn base64_chunks(input: &[u8], mut f: impl FnMut(&[u8])) {
    const CHUNK: usize = 64 * 1024;
    let mut chunk = Vec::with_capacity(input.len().min(CHUNK));
    for &b in input.iter().filter(|&&b| mime::is_b64(b) || b == b'=') {
        chunk.push(b);
        if chunk.len() == CHUNK {
            f(&Encoding::Base64.decode(&chunk));
            chunk.clear();
        }
    }
    f(&Encoding::Base64.decode(&chunk));
}

/// Decode at most `cap` bytes of a body. Return the bytes and true if bytes
/// went out. The input is cut before the decode, so the decode never
/// allocates more than about `cap` bytes.
fn decode_capped(body: &[u8], enc: Encoding, cap: usize) -> (Cow<'_, [u8]>, bool) {
    if enc == Encoding::QuotedPrintable {
        // Decode chunks until the bytes pass `cap`, then look for one more
        // decoded byte: the cut flag.
        let mut out = Vec::new();
        let mut more = false;
        qp_chunks(body, |b| {
            if out.len() <= cap {
                out.extend_from_slice(&b[..b.len().min(cap + 1 - out.len())]);
                true
            } else {
                more = !b.is_empty();
                !more
            }
        });
        let truncated = more || out.len() > cap;
        out.truncate(cap);
        return (Cow::Owned(out), truncated);
    }
    let input_len = match enc {
        // Line ends between base64 characters do not count.
        Encoding::Base64 => mime::base64_prefix(body, cap / 3 * 4 + 4),
        _ => body.len().min(cap),
    };
    let (input, rest) = body.split_at(input_len);
    // The base64 decoder of `mail-parser` allocates 3/4 of its input, white
    // space too. Give it only the alphabet and the padding.
    let mut bytes: Cow<'_, [u8]> = if enc == Encoding::Base64 {
        let mut out = Vec::with_capacity(mime::base64_chars(input) * 3 / 4 + 3);
        base64_chunks(input, |b| out.extend_from_slice(b));
        Cow::Owned(out)
    } else {
        enc.decode(input)
    };
    // Only data after the input is a cut: base64 padding and line ends are not.
    let truncated = decoded_len(rest, enc) > 0 || bytes.len() > cap;
    if bytes.len() > cap {
        bytes.to_mut().truncate(cap);
    }
    (bytes, truncated)
}

use mime::floor_char;

/// The subject prefixes of a reply in common clients and languages. A
/// forward (`Fwd:`, `WG:`) is not a reply.
const REPLY_PREFIXES: [&str; 6] = ["re", "aw", "sv", "vs", "antw", "réf"];

/// Characters that a person does not see. Mail uses them to pad a preview.
const INVISIBLE: [char; 8] = [
    '\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{feff}', '\u{034f}', '\u{00ad}', '\u{180e}',
];

/// Remove invisible characters and the spaces at line ends. Keep at most one
/// empty line between two lines of text.
fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank = false;
    for line in s.split('\n') {
        let line: String = line
            .chars()
            .filter(|c| !INVISIBLE.contains(c))
            .map(|c| if c == '\u{a0}' { ' ' } else { c })
            .collect();
        let line = line.trim_end();
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank { "\n\n" } else { "\n" });
        }
        blank = false;
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// For generated bodies of lines that match, almost match or do not
    /// match, fed in parts split at random points, `MethodLine` agrees with
    /// a whole-body line matcher.
    #[test]
    fn method_line_agrees_with_a_whole_body_matcher() {
        const LINES: [&[u8]; 14] = [
            b"METHOD:REQUEST",
            b"method:request",
            b"  METHOD:REQUEST \t",
            b"\tMethod:Request\r",
            b"METHOD:REQUESTS",
            b"METHOD:REQUES",
            b"METHOD: REQUEST",
            b"X-METHOD:REQUEST",
            b"METHOD:REQUEST X",
            b"METHOD:CANCEL",
            b"",
            b" ",
            b"BEGIN:VCALENDAR",
            b"DESCRIPTION:METHOD:REQUEST",
        ];
        let whole = |body: &[u8]| {
            body.split(|&c| c == b'\n')
                .any(|l| l.trim_ascii().eq_ignore_ascii_case(b"METHOD:REQUEST"))
        };
        let mut x: u64 = 0x5851_f42d_4c95_7f2d;
        let mut next = |n: usize| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x % n as u64) as usize
        };
        for _ in 0..5000 {
            let mut body = Vec::new();
            for _ in 0..next(6) {
                body.extend_from_slice(LINES[next(LINES.len())]);
                if next(4) > 0 {
                    body.push(b'\n');
                }
            }
            let mut m = MethodLine::default();
            let mut at = 0;
            while at < body.len() {
                let end = (at + 1 + next(8)).min(body.len());
                m.feed(&body[at..end]);
                at = end;
            }
            m.feed(b"\n");
            assert_eq!(
                m.found,
                whole(&body),
                "{:?}",
                String::from_utf8_lossy(&body)
            );
        }
    }

    /// For any quoted-printable body: each chunk has at most QP_CHUNK + 2
    /// bytes, and the chunks decode to the decode of the whole body.
    #[test]
    fn qp_chunks_stay_bounded_and_decode_as_the_whole() {
        const PIECES: [&[u8]; 12] = [
            b"=", b"A", b"3", b"=4", b"1", b"\r\n", b" ", b"=\r\n", b"\t", b"\r", b"G", b"= ",
        ];
        let mut x: u64 = 0x2545_f491_4f6c_dd1d;
        for round in 0..40 {
            let mut body = Vec::new();
            // Long lines with no line end, so the token rule decides.
            while body.len() < 3 * QP_CHUNK {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let p = PIECES[(x % PIECES.len() as u64) as usize];
                if round % 2 == 0 && p.contains(&b'\n') {
                    continue;
                }
                body.extend_from_slice(p);
            }
            let (mut out, mut max, mut at) = (Vec::new(), 0, 0);
            while at < body.len() {
                let end = qp_chunk_end(&body, at);
                max = max.max(end - at);
                at = end;
            }
            qp_chunks(&body, |b| {
                out.extend_from_slice(b);
                true
            });
            assert!(max <= QP_CHUNK + 2, "chunk of {max} bytes");
            assert_eq!(
                out,
                Encoding::QuotedPrintable.decode(&body),
                "round {round}"
            );
        }
        for b in *b"= " {
            let run = vec![b; 4 * 1024 * 1024];
            assert!(qp_chunk_end(&run, 0) <= QP_CHUNK + 2);
        }
        let eq = vec![b'='; 4 * 1024 * 1024 + 1];
        let mut out = Vec::new();
        qp_chunks(&eq, |b| {
            out.extend_from_slice(b);
            true
        });
        assert_eq!(out, Encoding::QuotedPrintable.decode(&eq));
    }

    /// The invariant of `decode_capped`, for each cap: the bytes are the first
    /// `min(cap, full)` bytes of the full decode, and the cut flag is true
    /// only if bytes of the full decode went out.
    fn check_every_cap(body: &[u8], enc: Encoding) {
        let (full, _) = decode_capped(body, enc, usize::MAX / 4);
        assert_eq!(
            &full[..],
            &enc.decode(body)[..],
            "{enc:?} full decode of {body:?}"
        );
        assert_eq!(
            decoded_len(body, enc),
            full.len() as u64,
            "{enc:?} size of {body:?}"
        );
        for cap in 0..=full.len() + 2 {
            let (bytes, cut) = decode_capped(body, enc, cap);
            let n = cap.min(full.len());
            assert_eq!(
                &bytes[..],
                &full[..n],
                "{enc:?} cap {cap} of {:?}",
                String::from_utf8_lossy(body)
            );
            assert_eq!(
                cut,
                full.len() > cap,
                "{enc:?} cap {cap} of {:?}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn quoted_printable_caps_keep_the_prefix_of_the_full_decode() {
        for body in [
            &b"=zz"[..],
            b"a=41b",
            b"ab=\r\ncd",
            b"ab=\ncd=",
            b"==41=4",
            b"x=4=41=",
            b"=\n=\n=\nX",
            b"a= \r\nb=3D=3d",
            b"=\r",
            b"=",
        ] {
            check_every_cap(body, Encoding::QuotedPrintable);
        }
    }

    #[test]
    fn base64_caps_keep_the_prefix_of_the_full_decode() {
        for body in [
            &b"QQ==QQ=="[..],
            b"QUJD\r\nREVG R0g=",
            b"QUJDRA==\r\n  ",
            b"Q",
            b"QUI",
        ] {
            check_every_cap(body, Encoding::Base64);
        }
    }

    #[test]
    fn base64_at_the_cap_with_white_space_after_is_not_cut() {
        // 12 characters of data, then padding and line ends.
        let body = b"QUJDREVGR0hJ\r\n\r\n  \r\n";
        for cap in [9, 10, 12] {
            let (bytes, cut) = decode_capped(body, Encoding::Base64, cap);
            assert_eq!((&bytes[..], cut), (&b"ABCDEFGHI"[..], false), "cap {cap}");
        }
        let (bytes, cut) = decode_capped(body, Encoding::Base64, 8);
        assert_eq!((&bytes[..], cut), (&b"ABCDEFGH"[..], true));
        let pad = b"QUJDRA==\r\n";
        assert!(!decode_capped(pad, Encoding::Base64, 4).1);
    }

    #[test]
    fn tidy_lines() {
        assert_eq!(tidy("\n\na \u{200c}\u{a0}\n\n\n\nb\u{feff}\n \n"), "a\n\nb");
    }

    #[test]
    fn floor_char_finds_a_boundary() {
        assert_eq!(floor_char("aé", 2), 1);
        assert_eq!(floor_char("aé", 9), 3);
    }
}
