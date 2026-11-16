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
//!
//! So the memory use does not grow with the size of the message, and the
//! time is linear in the size of the message. The largest use is a PDF of
//! 10 MiB with one decompressed stream of at most 4 MiB.

mod html;
mod mime;
mod pdf;
mod quote;
mod url;

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
}

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
    let mut parts = 0;
    let mut attach_left = ATTACHMENT_TEXT_LIMIT;
    let (subject, text, cut, files) = message(raw, 0, &mut parts);
    let mut attachments = Vec::new();
    for f in files {
        attachment(f, &mut parts, &mut attach_left, &mut attachments);
    }
    Clean {
        subject,
        text,
        cut,
        attachments,
    }
}

/// Step 1 for one message: the top message, or an attached
/// `message/rfc822` part. Return the subject, the text (with [`CUT_MARK`]
/// if it was cut), the cut flag and the attachments, not read yet. `parts`
/// counts the MIME parts of the whole top message.
fn message<'a>(
    raw: &'a [u8],
    depth: usize,
    parts: &mut usize,
) -> (String, String, bool, Vec<File<'a>>) {
    let h = mime::Headers::parse(mime::split_head(raw).0);
    let subject = mime::decode_words(h.get("subject").unwrap_or(""))
        .trim()
        .to_string();
    // Some clients send a reply without reply headers: the subject shows it.
    let re = subject
        .split_once(':')
        .is_some_and(|(p, _)| REPLY_PREFIXES.contains(&p.trim().to_lowercase().as_str()));
    let is_reply = re || h.get("in-reply-to").is_some() || h.get("references").is_some();
    // Step 1: the body text, and the list of the attachments (not decoded).
    let mut w = Walker {
        parts: *parts,
        texts: Vec::new(),
        chars: 0,
        cut: false,
        files: Vec::new(),
        is_reply,
    };
    w.entity(raw, depth);
    *parts = w.parts;
    let mut text = w.texts.join("\n\n");
    if w.cut {
        text.push_str(CUT_MARK);
    }
    (subject, text, w.cut, w.files)
}

/// An attachment that step 1 found. Step 2 reads it.
struct File<'a> {
    mime_type: String,
    name: Option<String>,
    charset: Option<String>,
    cte: String,
    body: &'a [u8],
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
    files: Vec<File<'a>>,
    /// True if the message is a reply. Quoted history then goes out.
    is_reply: bool,
}

impl<'a> Walker<'a> {
    fn entity(&mut self, raw: &'a [u8], depth: usize) {
        self.parts += 1;
        if self.parts > MAX_PARTS || depth > MAX_DEPTH {
            self.cut = true;
            return;
        }
        let (head, body) = mime::split_head(raw);
        let h = mime::Headers::parse(head);
        let ct = mime::parse_value(h.get("content-type").unwrap_or(""));
        let disp = mime::parse_value(h.get("content-disposition").unwrap_or(""));
        let cte = h
            .get("content-transfer-encoding")
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let mime_type = if ct.main.contains('/') {
            ct.main.clone()
        } else {
            "text/plain".to_string()
        };
        let name =
            mime::param(&disp.params, "filename").or_else(|| mime::param(&ct.params, "name"));
        let charset = mime::param(&ct.params, "charset");
        if let Some(sub) = mime_type
            .strip_prefix("multipart/")
            // A named multipart (for example a saved web page) is a file.
            .filter(|_| disp.main != "attachment" && name.is_none())
        {
            let Some(boundary) = mime::param(&ct.params, "boundary") else {
                return;
            };
            // One part more than the limit, so that the walker sees the overflow.
            let max = MAX_PARTS.saturating_sub(self.parts) + 1;
            let (list, more) = mime::split_multipart(body, &boundary, max);
            // Parts past the limit are not read. A last alternative can be one of them.
            self.cut |= more;
            if sub == "alternative" {
                self.alternative(list, depth);
            } else {
                for part in list {
                    self.entity(part, depth + 1);
                }
            }
            return;
        }
        let is_text = mime_type == "text/plain" || mime_type == "text/html";
        if is_text && disp.main != "attachment" && name.is_none() {
            self.body_part(body, &cte, mime_type == "text/html", charset.as_deref());
            return;
        }
        self.files.push(File {
            mime_type,
            name,
            charset,
            cte,
            body,
            depth,
        });
    }

    /// RFC 2046: the last version is the richest. A mail client shows it, so
    /// the clean text uses it too. The walker reads the versions from the
    /// end and uses the first one with text after the quote and signature
    /// steps. A version that is not used adds no text, no attachment and no cut.
    fn alternative(&mut self, list: Vec<&'a [u8]>, depth: usize) {
        for part in list.into_iter().rev() {
            let before = (self.texts.len(), self.chars, self.cut, self.files.len());
            self.entity(part, depth + 1);
            if self.texts.len() > before.0 {
                return;
            }
            // When the text is full, a version cannot add text. Its cut stays.
            if self.chars >= TEXT_LIMIT && self.cut {
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
    fn body_part(&mut self, body: &[u8], cte: &str, html: bool, charset: Option<&str>) {
        // Also when the text is full, the part goes through the steps: only
        // a later part with clean text makes a cut.
        let (bytes, truncated) = decode_capped(body, cte, PART_LIMIT);
        self.cut |= truncated;
        let text = mime::decode(&bytes, charset);
        drop(bytes);
        let text = if html {
            let (md, full) = html::to_markdown(&text, self.is_reply);
            self.cut |= full;
            md
        } else {
            text
        };
        let mut text = text.replace("\r\n", "\n").replace('\r', "\n");
        // The quote and signature steps keep a list of the lines. Bound it.
        if let Some((i, _)) = text.match_indices('\n').nth(MAX_LINES) {
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
    let size = mime::decoded_len(f.body, &f.cte);
    let mut name = f.name.unwrap_or_default();
    let mut text = None;
    // True if the attachment text is not complete.
    let mut cut = false;
    let mut inner_attachments = Vec::new();
    if f.mime_type == "message/rfc822" {
        // Also with no budget left, the walk lists the attachments of the
        // attached message.
        let had_budget = *attach_left > 0;
        let (inner, clipped) = decode_capped(f.body, &f.cte, INNER_LIMIT);
        let (subject, mut t, inner_cut, files) = message(&inner, f.depth + 1, parts);
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
        let (bytes, truncated) = decode_capped(f.body, &f.cte, *attach_left * wide);
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
        if pdf::is_pdf(&decode_capped(f.body, &f.cte, 1024).0) {
            let (bytes, _) = decode_capped(f.body, &f.cte, PDF_LIMIT as usize);
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

/// Decode at most `cap` bytes of a body. Return the bytes and true if bytes
/// went out. The input is cut before the decode, so the decode never
/// allocates more than about `cap` bytes.
fn decode_capped<'a>(body: &'a [u8], cte: &str, cap: usize) -> (std::borrow::Cow<'a, [u8]>, bool) {
    // Base64 gives 3 bytes for 4 characters. The other encodings give at
    // most 1 byte for each input byte.
    if cte == "quoted-printable" {
        let (bytes, more) = mime::qp_capped(body, cap);
        return (std::borrow::Cow::Owned(bytes), more);
    }
    // Line ends between base64 characters do not count.
    let max_in = if cte == "base64" {
        mime::base64_prefix(body, cap / 3 * 4 + 4)
    } else {
        cap
    };
    let input = &body[..body.len().min(max_in)];
    let mut bytes = mime::transfer_decode(input, cte);
    // Only data after the input is a cut: base64 padding and line ends are not.
    let rest = &body[input.len()..];
    let more = if cte == "base64" {
        mime::has_base64(rest)
    } else {
        !rest.is_empty()
    };
    let truncated = more || bytes.len() > cap;
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

    #[test]
    fn base64_at_the_cap_with_white_space_after_is_not_cut() {
        // 12 characters of data, then padding and line ends.
        let body = b"QUJDREVGR0hJ\r\n\r\n  \r\n";
        for cap in [9, 10, 12] {
            let (bytes, cut) = decode_capped(body, "base64", cap);
            assert_eq!((&bytes[..], cut), (&b"ABCDEFGHI"[..], false), "cap {cap}");
        }
        let (bytes, cut) = decode_capped(body, "base64", 8);
        assert_eq!((&bytes[..], cut), (&b"ABCDEFGH"[..], true));
        let pad = b"QUJDRA==\r\n";
        assert!(!decode_capped(pad, "base64", 4).1);
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
