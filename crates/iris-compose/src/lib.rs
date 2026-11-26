//! The mail that Iris writes (spec `docs/specs/2026-11-16-send.md`,
//! sections 1 to 3). Pure functions: no store, no network.
//!
//! - [`Content::check`] checks the content of a draft, the input of an
//!   author that the product does not control.
//! - [`compose`] makes the RFC 5322 message with `mail-builder`.
//! - [`read`] reads a message back with `mail-parser`, so that the engine
//!   can compute the hash of a draft as Gmail has it.
//! - [`hash`] is the content hash that the approval of a send binds to.
//! - [`reply_subject`] and [`references`] give the reply headers.
//!
//! Attachments, `Bcc` and the display name of `From` are in spec
//! `docs/specs/2026-11-24-send-content.md`.

use mail_builder::headers::address::Address;
use mail_builder::headers::content_type::ContentType;
use mail_builder::headers::message_id::MessageId;
use mail_builder::headers::raw::Raw;
use mail_builder::mime::{BodyPart, MimePart};
use mail_builder::MessageBuilder;
use mail_parser::{MessageParser, PartKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The most recipients of a draft: `to`, `cc` and `bcc` together.
pub const MAX_RECIPIENTS: usize = 50;
/// The most bytes of a body.
pub const MAX_BODY: usize = 256 * 1024;
/// The most characters of a subject (RFC 5322 line length).
pub const MAX_SUBJECT: usize = 998;
/// The most bytes of an address (RFC 5321 path length).
const MAX_ADDRESS: usize = 254;
/// The most attachments of a draft (send content spec, section 1).
pub const MAX_ATTACHMENTS: usize = 10;
/// The most bytes of the message of a draft, for each provider (send
/// content spec, section 5). Gmail: the read back of the release keeps at
/// most 4 MiB (`DRAFT_LIMIT` of `iris-provider`). Outlook: Graph takes
/// the message as base64 text in one request of at most 4 MB.
pub const MAX_MESSAGE: u64 = 4 * 1024 * 1024;
pub const MAX_MESSAGE_OUTLOOK: u64 = 2_900_000;
/// The bytes of the bound for the headers of the message and of each
/// part, apart from the reply headers.
const HEADER_BOUND: u64 = 64 * 1024;
/// The most characters of a file name, and of a MIME type.
const MAX_FILE_NAME: usize = 255;
const MAX_TYPE: usize = 127;
/// The most characters of the display name of `From` (send content spec,
/// section 3).
pub const MAX_NAME: usize = 128;

/// The most bytes of the message of a draft of an account of the
/// provider `provider` (the `provider` of the `account` row).
pub fn max_message(provider: &str) -> u64 {
    if provider == "outlook" {
        MAX_MESSAGE_OUTLOOK
    } else {
        MAX_MESSAGE
    }
}

/// The content of a draft (section 1). The JSON field names are the
/// `args` of a `draft` action.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    /// Empty is not in the JSON, so a draft of an older build keeps its
    /// JSON and its hash.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bcc: Vec<String>,
    #[serde(default)]
    pub subject: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
}

/// One attachment of a draft (send content spec, section 1). The file is
/// in the attachment folder of the store, at its SHA-256. The fields are
/// in sorted order, so the JSON of the hash is canonical.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub name: String,
    /// 64 lower case hexadecimal characters.
    pub sha256: String,
    pub size: u64,
    /// The MIME type, `type/subtype` in lower case.
    #[serde(rename = "type")]
    pub mime: String,
}

impl Attachment {
    /// The entry of the file `bytes`, with a name and a type.
    pub fn of(name: &str, mime: &str, bytes: &[u8]) -> Attachment {
        Attachment {
            name: name.into(),
            sha256: sha256(bytes),
            size: bytes.len() as u64,
            mime: mime.into(),
        }
    }

    /// True if `bytes` is the file of this entry.
    pub fn is_file(&self, bytes: &[u8]) -> bool {
        bytes.len() as u64 == self.size && sha256(bytes) == self.sha256
    }

    /// As [`Attachment::is_file`], for the bytes of `r`, read in parts of
    /// [`FILE_CHUNK`] bytes: the memory holds one part, whatever the size
    /// of the file. It stops at the first byte after [`Attachment::size`].
    pub fn is_file_in(&self, mut r: impl std::io::Read) -> std::io::Result<bool> {
        let mut hash = Sha256::new();
        let mut buf = vec![0u8; FILE_CHUNK];
        let mut len = 0u64;
        loop {
            let n = match r.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            len += n as u64;
            if len > self.size {
                return Ok(false);
            }
            hash.update(&buf[..n]);
        }
        let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        Ok(len == self.size && hex == self.sha256)
    }
}

/// The size of one read of [`Attachment::is_file_in`].
pub const FILE_CHUNK: usize = 64 * 1024;

/// The SHA-256 of `bytes` in lower case hexadecimal.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The reply headers of a draft in a thread (section 3). The IDs have no
/// angle brackets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub in_reply_to: String,
    pub references: Vec<String>,
}

/// A message read back with [`read`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    /// The `Message-ID`, with no angle brackets.
    pub message_id: String,
    pub content: Content,
    pub reply: Option<Reply>,
    /// The decoded file of each entry of `content.attachments`, in order.
    pub files: Vec<Vec<u8>>,
    /// True if the message has a `Bcc` header. A provider can remove it
    /// from a sent copy (send content spec, section 4).
    pub bcc_header: bool,
    /// The `Date` header, in seconds since 1970, if the message has one.
    pub date: Option<i64>,
}

impl Content {
    /// Check the content and give its normal form: addresses in lower case
    /// with no duplicate (an address in `to` leaves `cc` and `bcc`, an
    /// address in `cc` leaves `bcc`), line ends as LF, and file names and
    /// types in their normal form. Gives the reason of a refusal.
    pub fn check(mut self) -> Result<Content, String> {
        let mut seen = std::collections::HashSet::new();
        for list in [&mut self.to, &mut self.cc, &mut self.bcc] {
            let mut out = Vec::with_capacity(list.len());
            for a in list.iter() {
                let a = a.to_ascii_lowercase();
                check_address(&a)?;
                if seen.insert(a.clone()) {
                    out.push(a);
                }
            }
            *list = out;
        }
        if self.to.is_empty() {
            return Err("a draft needs one or more addresses in to".into());
        }
        if self.to.len() + self.cc.len() + self.bcc.len() > MAX_RECIPIENTS {
            return Err(format!("a draft has at most {MAX_RECIPIENTS} recipients"));
        }
        if self.subject.chars().any(char::is_control) {
            return Err("a subject has no control character".into());
        }
        self.subject = spaces(&self.subject);
        let chars = self.subject.chars().count();
        if chars == 0 {
            return Err("a draft needs a subject".into());
        }
        if chars > MAX_SUBJECT {
            return Err(format!("a subject has at most {MAX_SUBJECT} characters"));
        }
        self.body = line_ends(&self.body);
        // One rule for each path that makes a draft: no mail with no text leaves.
        if self.body.trim().is_empty() {
            return Err("a draft needs a body".into());
        }
        if self.body.len() > MAX_BODY {
            return Err(format!("a body has at most {MAX_BODY} bytes"));
        }
        if self.body.contains('\0') {
            return Err("a body has no NUL character".into());
        }
        if self.attachments.len() > MAX_ATTACHMENTS {
            return Err(format!("a draft has at most {MAX_ATTACHMENTS} attachments"));
        }
        for a in &mut self.attachments {
            // Only spaces: a control character at an end stays, and the
            // check refuses it.
            a.name = a.name.trim_matches(' ').to_string();
            a.mime = a.mime.to_ascii_lowercase();
            check_attachment(a)?;
        }
        self.check_size(MAX_MESSAGE)?;
        Ok(self)
    }

    /// An upper bound of the bytes of the message that [`compose_with`]
    /// writes for this content, apart from the reply headers (send content
    /// spec, section 5).
    pub fn size_bound(&self) -> u64 {
        let files: u64 = self
            .attachments
            .iter()
            .map(|a| a.size.div_ceil(57).saturating_mul(78))
            .fold(0, u64::saturating_add);
        HEADER_BOUND
            .saturating_add(3 * self.body.len() as u64)
            .saturating_add(files)
    }

    /// Refuse a content whose [`Content::size_bound`] is over `limit`.
    pub fn check_size(&self, limit: u64) -> Result<(), String> {
        if self.size_bound() > limit {
            return Err(format!(
                "the mail is too large: at most {limit} bytes with its files; remove a file"
            ));
        }
        Ok(())
    }
}

/// The checks of one attachment entry in its normal form (section 1).
fn check_attachment(a: &Attachment) -> Result<(), String> {
    let chars = a.name.chars().count();
    if chars == 0 || chars > MAX_FILE_NAME {
        return Err(format!(
            "a file name has 1 to {MAX_FILE_NAME} characters: {:?}",
            a.name
        ));
    }
    if a.name
        .chars()
        .any(|c| c.is_control() || c == '/' || c == '\\')
        || a.name == "."
        || a.name == ".."
        // A reader decodes `=?..?=` in a file name as an encoded word, so
        // such a name does not read back the same (as a subject, #270).
        || a.name.contains("=?")
    {
        return Err(format!(
            "a file name has no control character, no /, no \\ and no =?: {:?}",
            a.name
        ));
    }
    // An RFC 2045 token: printable ASCII with no space and no tspecial.
    let token = |t: &str| {
        !t.is_empty()
            && t.chars()
                .all(|c| c.is_ascii_graphic() && !"()<>@,;:\\\"/[]?=".contains(c))
    };
    let ok = a.mime.len() <= MAX_TYPE
        && a.mime
            .split_once('/')
            .is_some_and(|(t, s)| token(t) && token(s) && t != "multipart" && t != "message");
    if !ok {
        return Err(format!(
            "a file type is type/subtype, not multipart or message: {:?}",
            a.mime
        ));
    }
    let hex = a.sha256.len() == 64
        && a.sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !hex {
        return Err("a file sha256 is 64 lower case hexadecimal characters".into());
    }
    if a.size == 0 {
        return Err(format!("a file has 1 or more bytes: {:?}", a.name));
    }
    Ok(())
}

/// A display name of `From` that Iris can use: no control character and
/// at most [`MAX_NAME`] characters, with no space at the ends. `None` for
/// an empty name (send content spec, section 3).
pub fn from_name(name: &str) -> Option<String> {
    let n = name.trim_matches(' ');
    (!n.is_empty() && n.chars().count() <= MAX_NAME && !n.chars().any(char::is_control))
        .then(|| n.to_string())
}

/// A subject in its normal form: no space at the ends, and one space for
/// each run of white space, as a header fold and a reader change them.
fn spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Line ends as LF: CRLF and a lone CR become LF.
fn line_ends(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// An address `local@domain`: ASCII, printable, no space and none of the
/// characters that have a meaning in an address header.
pub fn check_address(a: &str) -> Result<(), String> {
    let bad = || Err(format!("not an address of the form local@domain: {a:?}"));
    if a.len() > MAX_ADDRESS {
        return bad();
    }
    let Some((local, domain)) = a.split_once('@') else {
        return bad();
    };
    let special = |c: char| "<>()[],;:\"\\@".contains(c);
    let local_ok = !local.is_empty()
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local.chars().all(|c| c.is_ascii_graphic() && !special(c));
    let domain_ok = domain.split('.').count() >= 2
        && domain.split('.').all(|l| {
            !l.is_empty()
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if local_ok && domain_ok {
        Ok(())
    } else {
        bad()
    }
}

/// The `Message-ID` of a new draft, with no angle brackets:
/// `iris.<32 hex>@<domain of the account>`. The caller gives 16 random
/// bytes.
pub fn message_id(random: [u8; 16], account: &str) -> String {
    let domain = account.rsplit_once('@').map_or("localhost", |(_, d)| d);
    let hex: String = random.iter().map(|b| format!("{b:02x}")).collect();
    format!("iris.{hex}@{domain}")
}

/// The subject of a reply: `Re: ` in front, unless the subject starts with
/// `Re:` in any case. Control characters of the parent become spaces.
pub fn reply_subject(parent: &str) -> String {
    let s: String = parent
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let s = s.trim();
    if s.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("re:")) {
        s.to_string()
    } else {
        format!("Re: {s}")
    }
}

/// The message IDs of a header value (`Message-ID`, `In-Reply-To` or
/// `References`), with no angle brackets. Each `<...>` gives one ID; a
/// value with no `<` gives one ID for each word. An ID that is empty, has
/// white space or a character that is not printable ASCII is dropped, so
/// each ID writes and reads back the same.
fn header_ids(value: &str) -> Vec<String> {
    let ok = |id: &&str| {
        !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_graphic() && c != '<' && c != '>')
    };
    if value.contains('<') {
        value
            .split('<')
            .skip(1)
            .filter_map(|part| part.split_once('>').map(|(id, _)| id))
            .filter(ok)
            .map(str::to_string)
            .collect()
    } else {
        value
            .split_whitespace()
            .filter(ok)
            .map(str::to_string)
            .collect()
    }
}

/// The reply headers to the parent message, from the header values of the
/// parent as Gmail gives them: `In-Reply-To` is the parent ID;
/// `References` is the IDs of the parent `References` (or, if it has none,
/// of its `In-Reply-To`), then the parent ID. `None` if the parent
/// `Message-ID` does not have exactly one ID.
pub fn references(
    parent_id: &str,
    parent_refs: Option<&str>,
    parent_in_reply_to: Option<&str>,
) -> Option<Reply> {
    let [id] = header_ids(parent_id).try_into().ok()?;
    let mut refs = header_ids(parent_refs.unwrap_or(""));
    if refs.is_empty() {
        refs = header_ids(parent_in_reply_to.unwrap_or(""));
    }
    refs.retain(|r| *r != id);
    refs.push(id.clone());
    Some(Reply {
        in_reply_to: id,
        references: refs,
    })
}

/// The content hash (section 1): SHA-256 (hex) of the canonical JSON of the
/// content and the reply headers. Give the content of [`Content::check`].
pub fn hash(content: &Content, reply: Option<&Reply>) -> String {
    // Fields in sorted order, so the JSON is canonical.
    #[derive(Serialize)]
    struct Canonical<'a> {
        // Empty lists are not in the JSON: the hash of a draft of an older
        // build stays the same (send content spec, section 1).
        #[serde(skip_serializing_if = "<[_]>::is_empty")]
        attachments: &'a [Attachment],
        #[serde(skip_serializing_if = "<[_]>::is_empty")]
        bcc: &'a [String],
        body: &'a str,
        cc: &'a [String],
        // New mail has no reply fields (spec R3-01, section 1).
        #[serde(skip_serializing_if = "Option::is_none")]
        in_reply_to: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        references: Option<&'a [String]>,
        subject: &'a str,
        to: &'a [String],
    }
    let c = Canonical {
        attachments: &content.attachments,
        bcc: &content.bcc,
        body: &content.body,
        cc: &content.cc,
        in_reply_to: reply.map(|r| r.in_reply_to.as_str()),
        references: reply.map(|r| r.references.as_slice()),
        subject: &content.subject,
        to: &content.to,
    };
    sha256(&serde_json::to_vec(&c).expect("plain data"))
}

/// The RFC 5322 message of a draft with no attachment and no display
/// name. See [`compose_with`].
pub fn compose(
    from: &str,
    message_id: &str,
    content: &Content,
    reply: Option<&Reply>,
    date: i64,
) -> Vec<u8> {
    compose_with(from, None, message_id, content, reply, date, &[])
}

/// The RFC 5322 message of a draft (section 2). `from` is the account
/// address and `name` its display name, `message_id` has no angle
/// brackets, `date` is in seconds since 1970. `files` has the bytes of
/// each attachment of the content, in order. Give the content of
/// [`Content::check`]. The same input gives the same bytes: the MIME
/// boundary comes from the input, not from a clock.
///
/// Panics if `files` does not have one file for each attachment.
pub fn compose_with(
    from: &str,
    name: Option<&str>,
    message_id: &str,
    content: &Content,
    reply: Option<&Reply>,
    date: i64,
    files: &[&[u8]],
) -> Vec<u8> {
    fn list(v: &[String]) -> Address<'_> {
        Address::new_list(v.iter().map(|a| Address::from(a.as_str())).collect())
    }
    assert_eq!(
        files.len(),
        content.attachments.len(),
        "one file for each attachment"
    );
    let from = match name {
        Some(n) => Address::new_address(Some(n), from),
        None => Address::from(from),
    };
    let mut m = MessageBuilder::new()
        .from(from)
        .to(list(&content.to))
        .date(date)
        .message_id(message_id);
    // mail-builder writes a short ASCII subject as it is, and a reader
    // decodes `=?..?=` in it as an encoded word: such a subject goes out
    // as encoded words, so that it reads back the same (#270).
    m = if content.subject.contains("=?") {
        m.header("Subject", Raw::new(encoded_words(&content.subject)))
    } else {
        m.subject(content.subject.as_str())
    };
    if !content.cc.is_empty() {
        m = m.cc(list(&content.cc));
    }
    if !content.bcc.is_empty() {
        m = m.bcc(list(&content.bcc));
    }
    if let Some(r) = reply {
        m = m
            .in_reply_to(r.in_reply_to.as_str())
            .references(MessageId::new_list(r.references.iter().map(String::as_str)));
    }
    m = if content.attachments.is_empty() {
        m.text_body(content.body.as_str())
    } else {
        // The boundary is the hash of the input: it is the same for the
        // same input, and an author cannot put it into the body, because
        // the body is part of the input.
        let seed = format!("{message_id}\n{date}\n{}", hash(content, reply));
        let boundary = format!("iris-{}", &sha256(seed.as_bytes())[..32]);
        let mut parts = vec![MimePart::new(
            "text/plain",
            BodyPart::Text(content.body.as_str().into()),
        )];
        for (a, bytes) in content.attachments.iter().zip(files) {
            parts.push(
                MimePart::new(a.mime.as_str(), BodyPart::Binary((*bytes).into()))
                    .attachment(a.name.as_str()),
            );
        }
        m.body(MimePart::new(
            ContentType::new("multipart/mixed").attribute("boundary", boundary),
            parts,
        ))
    };
    m.write_to_vec().expect("a write to memory does not fail")
}

/// `text` as RFC 2047 encoded words (`=?utf-8?B?...?=`), one folded line
/// for each word. Each word holds whole characters of at most 45 bytes, so
/// it has at most 72 characters (the limit is 75).
fn encoded_words(text: &str) -> String {
    use base64::Engine;
    let mut words = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = start;
        for (i, ch) in text[start..].char_indices() {
            if i + ch.len_utf8() > 45 {
                break;
            }
            end = start + i + ch.len_utf8();
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&text[start..end]);
        words.push(format!("=?utf-8?B?{b64}?="));
        start = end;
    }
    words.join("\r\n ")
}

/// The headers that Iris writes. A read back refuses each other header.
const HEADERS: [&str; 12] = [
    "from",
    "to",
    "cc",
    "bcc",
    "subject",
    "date",
    "message-id",
    "in-reply-to",
    "references",
    "mime-version",
    "content-type",
    "content-transfer-encoding",
];

/// The headers that change who gets a message or who gets the answer,
/// and that Iris never writes, so a read back refuses each of them, also
/// in [`read_sent`]. Each `Resent-` header is refused too. `Bcc` is in the
/// content (send content spec, section 4).
const ADDRESSING: [&str; 2] = ["reply-to", "sender"];

/// The transport header that Gmail adds to each draft at `drafts.create`
/// (send spec, assumption 3). A draft read back ignores it, at any count.
const TRANSPORT: &str = "received";

/// Read a message back to its content (section 1). `None` if the message
/// does not have exactly the form that Iris writes: one `text/plain` part,
/// or a `multipart/mixed` of one `text/plain` part and file parts; only
/// the headers of [`HEADERS`], each at most one time, `From` equal to
/// `account`, and a `Message-ID`. A `Received` header is ignored, because
/// Gmail adds one to each draft. A draft that the person changed to HTML,
/// or gave a `Reply-To` or a nested part, does not read back, so its hash
/// is never an approved hash.
pub fn read(raw: &[u8], account: &str) -> Option<Read> {
    read_with(raw, account, true)
}

/// Read a sent message back to its content (section 6): as [`read`], but
/// each header that is not in [`HEADERS`] is ignored, because Gmail can
/// add transport headers to the sent copy. A header of `ADDRESSING`
/// still gives `None`. The fields of the hash, the one `text/plain` part
/// and `From` are checked as in [`read`]. A copy with no `Bcc` header
/// takes `approved_bcc` (send content spec, section 4): a provider can
/// drop the header from the copy of the sender. `bcc_header` tells which.
pub fn read_sent(raw: &[u8], account: &str, approved_bcc: &[String]) -> Option<Read> {
    let mut r = read_with(raw, account, false)?;
    if !r.bcc_header {
        r.content.bcc = approved_bcc.to_vec();
    }
    Some(r)
}

fn read_with(raw: &[u8], account: &str, strict: bool) -> Option<Read> {
    let m = MessageParser::default().parse(raw)?;
    let mut names = std::collections::HashSet::new();
    for h in m.headers().iter() {
        let name = h.raw_name().to_ascii_lowercase();
        let known = HEADERS.contains(&name.as_str());
        let addressing = ADDRESSING.contains(&name.as_str()) || name.starts_with("resent-");
        let ignored = !strict || name == TRANSPORT;
        if addressing || (!known && !ignored) || (known && !names.insert(name)) {
            return None;
        }
    }
    let (body, attachments, files) = parts(&m)?;
    let from = m.from()?;
    match (
        from.mailboxes().count(),
        from.first().and_then(|b| b.address()),
    ) {
        (1, Some(a)) if a.eq_ignore_ascii_case(account) => {}
        _ => return None,
    }
    let addresses = |l: Option<mail_parser::AddressList<'_>>| -> Option<Vec<String>> {
        let Some(l) = l else { return Some(Vec::new()) };
        l.mailboxes()
            .map(|b| b.address().map(str::to_string))
            .collect()
    };
    let ids = |l: Option<mail_parser::TextList<'_>>| -> Vec<String> {
        l.map(|l| {
            (0..l.len())
                .filter_map(|i| l.get(i))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
    };
    let content = Content {
        to: addresses(m.to())?,
        cc: addresses(m.cc())?,
        bcc: addresses(m.bcc())?,
        subject: spaces(m.subject().unwrap_or("")),
        // Mail has CRLF line ends; the content has LF (Content::check).
        body: line_ends(&body),
        attachments,
    };
    // The content must be in the normal form of Content::check: a change
    // that the check refuses (too many recipients, a large body) or erases
    // (a duplicate address, an upper case address) is not in the hash, so
    // it gives no content.
    if content.clone().check().ok()? != content {
        return None;
    }
    // Each thread header of the message is in the content hash, or the
    // message gives no content: a header that the read drops (for example
    // References with no In-Reply-To) must not keep the approved hash.
    let in_reply_to = ids(m.in_reply_to());
    let references = ids(m.references());
    let reply = match (
        in_reply_to.as_slice(),
        names.contains("in-reply-to"),
        names.contains("references"),
    ) {
        ([], false, false) => None,
        ([one], true, has_references) if has_references == !references.is_empty() => Some(Reply {
            in_reply_to: one.clone(),
            references,
        }),
        _ => return None,
    };
    let read = Read {
        message_id: m.message_id()?.to_string(),
        content,
        reply,
        files,
        bcc_header: names.contains("bcc"),
        date: m.date().map(|d| d.to_timestamp()),
    };
    // By construction: the message that Iris writes for this content has
    // the same recipient, subject and ID headers, as mail-parser reads them
    // (display names, groups, comments and white space included). So a field that the content
    // does not hold gives no content. `From` is checked above by address
    // only, because Gmail can add the display name of the account.
    let files: Vec<&[u8]> = read.files.iter().map(Vec::as_slice).collect();
    let again = compose_with(
        account,
        None,
        &read.message_id,
        &read.content,
        read.reply.as_ref(),
        0,
        &files,
    );
    let again = MessageParser::default().parse(&again[..])?;
    (fields(&m) == fields(&again)).then_some(read)
}

/// The headers of the hash, parsed, in order. The body is compared as
/// text by the content itself.
const FIELDS: [&str; 7] = [
    "to",
    "cc",
    "bcc",
    "subject",
    "message-id",
    "in-reply-to",
    "references",
];

/// The body text, the attachment entries and the files of a message in
/// the form that Iris writes (send content spec, section 4): a root
/// `text/plain` part; or a root `multipart/mixed` whose first part is
/// `text/plain` and whose other parts are each a file with a name, with
/// no nested part. `None` for each other form, also for a part that
/// mail-parser read with a problem.
#[allow(clippy::type_complexity)]
fn parts(m: &mail_parser::Message<'_>) -> Option<(String, Vec<Attachment>, Vec<Vec<u8>>)> {
    let plain = |p: &mail_parser::MessagePart<'_>| {
        matches!(p.kind(), PartKind::Text)
            && p.is_content_type("text", "plain")
            && p.content_disposition().is_none()
            && !p.has_problems()
    };
    let root = m.root_part();
    if !root.is_multipart() {
        if !plain(&root) || m.parts().len() != 1 {
            return None;
        }
        return Some((root.text()?.into_owned(), vec![], vec![]));
    }
    if !root.is_content_type("multipart", "mixed") || root.has_problems() {
        return None;
    }
    let children: Vec<_> = root.children().collect();
    // Each part of the message is the root or a child of it: no nested part.
    if children.len() < 2 || m.parts().len() != children.len() + 1 {
        return None;
    }
    let (first, rest) = children.split_first()?;
    if !plain(first) {
        return None;
    }
    let mut attachments = Vec::with_capacity(rest.len());
    let mut files = Vec::with_capacity(rest.len());
    for p in rest {
        // A file of any type, also `text/html`: the disposition makes it a
        // file, not a body.
        let is_file = matches!(
            p.kind(),
            PartKind::Text | PartKind::Html | PartKind::Binary | PartKind::InlineBinary
        ) && !p.has_problems()
            && p.content_disposition()
                .is_some_and(|d| d.ctype().eq_ignore_ascii_case("attachment"));
        if !is_file {
            return None;
        }
        let ct = p.content_type()?;
        let mime = format!("{}/{}", ct.ctype(), ct.subtype()?);
        let bytes = p.decoded().into_owned();
        attachments.push(Attachment::of(p.attachment_name()?, &mime, &bytes));
        files.push(bytes);
    }
    Some((first.text()?.into_owned(), attachments, files))
}

fn fields(m: &mail_parser::Message<'_>) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = m
        .headers()
        .iter()
        .filter_map(|h| {
            let name = h.raw_name().to_ascii_lowercase();
            FIELDS
                .contains(&name.as_str())
                .then(|| (name, format!("{:?}", h.value())))
        })
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests;
