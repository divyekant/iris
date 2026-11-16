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

use mail_builder::headers::address::Address;
use mail_builder::headers::message_id::MessageId;
use mail_builder::MessageBuilder;
use mail_parser::MessageParser;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The most recipients of a draft: `to` and `cc` together.
pub const MAX_RECIPIENTS: usize = 50;
/// The most bytes of a body.
pub const MAX_BODY: usize = 256 * 1024;
/// The most characters of a subject (RFC 5322 line length).
pub const MAX_SUBJECT: usize = 998;
/// The most bytes of an address (RFC 5321 path length).
const MAX_ADDRESS: usize = 254;

/// The content of a draft (section 1). The JSON field names are the
/// `args` of a `draft` action.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub subject: String,
    pub body: String,
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
}

impl Content {
    /// Check the content and give its normal form: addresses in lower case
    /// with no duplicate (an address in `to` leaves `cc`), and line ends
    /// as LF. Gives the reason of a refusal.
    pub fn check(mut self) -> Result<Content, String> {
        let mut seen = std::collections::HashSet::new();
        for list in [&mut self.to, &mut self.cc] {
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
        if self.to.len() + self.cc.len() > MAX_RECIPIENTS {
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
        // mail-builder writes a short ASCII subject as it is, and a reader
        // decodes `=?..?=` in it as an encoded word: the subject changes.
        if self.subject.contains("=?") {
            return Err("a subject has no \"=?\"".into());
        }
        self.body = line_ends(&self.body);
        if self.body.len() > MAX_BODY {
            return Err(format!("a body has at most {MAX_BODY} bytes"));
        }
        if self.body.contains('\0') {
            return Err("a body has no NUL character".into());
        }
        Ok(self)
    }
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
fn check_address(a: &str) -> Result<(), String> {
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
        body: &content.body,
        cc: &content.cc,
        in_reply_to: reply.map(|r| r.in_reply_to.as_str()),
        references: reply.map(|r| r.references.as_slice()),
        subject: &content.subject,
        to: &content.to,
    };
    let json = serde_json::to_vec(&c).expect("plain data");
    Sha256::digest(&json)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The RFC 5322 message of a draft (section 2). `from` is the account
/// address, `message_id` has no angle brackets, `date` is in seconds since
/// 1970. Give the content of [`Content::check`].
pub fn compose(
    from: &str,
    message_id: &str,
    content: &Content,
    reply: Option<&Reply>,
    date: i64,
) -> Vec<u8> {
    fn list(v: &[String]) -> Address<'_> {
        Address::new_list(v.iter().map(|a| Address::from(a.as_str())).collect())
    }
    let mut m = MessageBuilder::new()
        .from(from)
        .to(list(&content.to))
        .subject(content.subject.as_str())
        .date(date)
        .message_id(message_id);
    if !content.cc.is_empty() {
        m = m.cc(list(&content.cc));
    }
    if let Some(r) = reply {
        m = m
            .in_reply_to(r.in_reply_to.as_str())
            .references(MessageId::new_list(r.references.iter().map(String::as_str)));
    }
    m.text_body(content.body.as_str())
        .write_to_vec()
        .expect("a write to memory does not fail")
}

/// The headers that Iris writes. A read back refuses each other header.
const HEADERS: [&str; 11] = [
    "from",
    "to",
    "cc",
    "subject",
    "date",
    "message-id",
    "in-reply-to",
    "references",
    "mime-version",
    "content-type",
    "content-transfer-encoding",
];

/// The headers that change who gets a message or who gets the answer.
/// Iris never writes them, so a read back refuses each of them, also in
/// [`read_sent`]. Each `Resent-` header is refused too.
const ADDRESSING: [&str; 3] = ["bcc", "reply-to", "sender"];

/// Read a message back to its content (section 1). `None` if the message
/// does not have exactly the form that Iris writes: one `text/plain` part,
/// only the headers of [`HEADERS`], each at most one time, `From` equal to
/// `account`, and a `Message-ID`. A draft that the person changed to HTML,
/// or gave a `Bcc`, a `Reply-To` or an attachment, does not read back, so
/// its hash is never an approved hash.
pub fn read(raw: &[u8], account: &str) -> Option<Read> {
    read_with(raw, account, true)
}

/// Read a sent message back to its content (section 6): as [`read`], but
/// each header that is not in [`HEADERS`] is ignored, because Gmail can
/// add transport headers to the sent copy. A header of `ADDRESSING`
/// still gives `None`. The fields of the hash, the one `text/plain` part
/// and `From` are checked as in [`read`].
pub fn read_sent(raw: &[u8], account: &str) -> Option<Read> {
    read_with(raw, account, false)
}

fn read_with(raw: &[u8], account: &str, strict: bool) -> Option<Read> {
    let m = MessageParser::default().parse(raw)?;
    let mut names = std::collections::HashSet::new();
    for h in m.headers().iter() {
        let name = h.raw_name().to_ascii_lowercase();
        let known = HEADERS.contains(&name.as_str());
        let addressing = ADDRESSING.contains(&name.as_str()) || name.starts_with("resent-");
        if addressing || (strict && !known) || (known && !names.insert(name)) {
            return None;
        }
    }
    if m.has_attachments() || m.text_body().len() != 1 {
        return None;
    }
    let from = m.from()?;
    match (
        from.mailboxes().count(),
        from.first().and_then(|b| b.address()),
    ) {
        (1, Some(a)) if a.eq_ignore_ascii_case(account) => {}
        _ => return None,
    }
    let root = m.root_part();
    let ct = root.content_type()?;
    if !ct.ctype().eq_ignore_ascii_case("text") || !ct.subtype()?.eq_ignore_ascii_case("plain") {
        return None;
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
        subject: spaces(m.subject().unwrap_or("")),
        // Mail has CRLF line ends; the content has LF (Content::check).
        body: line_ends(&m.body_text(0)?),
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
    };
    // By construction: the message that Iris writes for this content has
    // the same recipient and ID headers, as mail-parser reads them (display
    // names, groups and comments included). So a field that the content
    // does not hold gives no content. `From` is checked above by address
    // only, because Gmail can add the display name of the account.
    let again = compose(
        account,
        &read.message_id,
        &read.content,
        read.reply.as_ref(),
        0,
    );
    let again = MessageParser::default().parse(&again[..])?;
    (fields(&m) == fields(&again)).then_some(read)
}

/// The headers of the hash that hold addresses or IDs, parsed, in order.
const FIELDS: [&str; 5] = ["to", "cc", "message-id", "in-reply-to", "references"];

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
