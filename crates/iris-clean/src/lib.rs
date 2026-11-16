//! The clean text of one mail message (specification 1, section 7.2).
//!
//! `clean(raw)` reads the raw RFC 5322 bytes of one message and returns the
//! text that a person reads: MIME and charsets decoded, HTML as Markdown,
//! without quoted history, signatures, hidden HTML and tracking.
//!
//! The functions are pure: no network, no files, no model. Mail text is data.
//! This crate never runs or follows text from a message.
//!
//! Bounds for one message, whatever its size:
//! - The body text parts give at most [`BODY_LIMIT`] decoded bytes.
//! - The text of attachments has at most [`ATTACHMENT_TEXT_LIMIT`] bytes.
//! - An attached message gives at most [`INNER_LIMIT`] decoded bytes to the parser.
//! - Only a PDF of at most [`PDF_LIMIT`] bytes is decoded for its text.
//! - The parser reads at most [`MAX_PARTS`] MIME parts, [`MAX_DEPTH`] levels deep.
//! - The body of an attachment is not decoded to measure its size.
//!
//! So the memory use does not grow with the size of attachments, and the
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
/// The most decoded bytes of body text parts (plain text or HTML) for each message.
pub const BODY_LIMIT: usize = 2 * 1024 * 1024;
/// The most bytes of text that stay after the quote and signature steps, for the URL and tidy steps.
const STEP_LIMIT: usize = 512 * 1024;
/// The most MIME parts that the parser reads in one message.
pub const MAX_PARTS: usize = 500;
/// The most levels of nested MIME parts.
pub const MAX_DEPTH: usize = 8;
/// The most decoded bytes of an attached message that the parser reads.
pub const INNER_LIMIT: usize = 2 * 1024 * 1024;
/// The largest PDF attachment (decoded bytes) that gives text (spec 1 section 9).
pub const PDF_LIMIT: u64 = 10 * 1024 * 1024;
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
    let mut w = Walker {
        parts: 0,
        body_left: BODY_LIMIT,
        attach_left: ATTACHMENT_TEXT_LIMIT,
        cut: false,
        attachments: Vec::new(),
        is_reply: false,
    };
    let (subject, mut text) = w.message(raw, 0);
    let mut cut = w.cut;
    if let Some((i, _)) = text.char_indices().nth(TEXT_LIMIT) {
        text.truncate(i);
        cut = true;
    }
    if cut {
        text.push_str(CUT_MARK);
    }
    Clean {
        subject,
        text,
        cut,
        attachments: w.attachments,
    }
}

struct Walker {
    parts: usize,
    body_left: usize,
    attach_left: usize,
    cut: bool,
    attachments: Vec<Attachment>,
    /// True while the walker reads a reply. HTML quote blocks then go out.
    is_reply: bool,
}

impl Walker {
    /// The subject and the clean text of a message: the top entity, or an
    /// attached `message/rfc822` part.
    fn message(&mut self, raw: &[u8], depth: usize) -> (String, String) {
        let h = mime::Headers::parse(mime::split_head(raw).0);
        let subject = mime::decode_words(h.get("subject").unwrap_or(""))
            .trim()
            .to_string();
        let is_reply = h.get("in-reply-to").is_some() || h.get("references").is_some();
        let outer = std::mem::replace(&mut self.is_reply, is_reply);
        let bodies = self.entity(raw, depth);
        self.is_reply = outer;
        (subject, self.render(bodies, is_reply))
    }

    fn render(&mut self, bodies: Vec<String>, is_reply: bool) -> String {
        let mut text = String::new();
        let mut left = STEP_LIMIT;
        for t in bodies {
            // Each part has its own quotes and signature: a cut in one part
            // must not remove the text of a later part. The body budget
            // bounds the input, and only the text that stays uses the step
            // budget, so quoted history cannot use up the room of later parts.
            let t = t.replace("\r\n", "\n").replace('\r', "\n");
            let mut t = quote::strip_signature(&quote::strip_quotes(&t, is_reply));
            if t.trim().is_empty() {
                continue;
            }
            if t.len() > left {
                t.truncate(floor_char(&t, left));
                self.cut = true;
            }
            left -= t.len();
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&t);
        }
        tidy(&url::clean_urls(&text))
    }

    /// Walk one entity. Return its body texts in order. Attachments go to
    /// `self.attachments`.
    fn entity(&mut self, raw: &[u8], depth: usize) -> Vec<String> {
        self.parts += 1;
        if self.parts > MAX_PARTS || depth > MAX_DEPTH {
            self.cut = true;
            return Vec::new();
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
                return Vec::new();
            };
            // One part more than the limit, so that the walker sees the overflow.
            let max = MAX_PARTS.saturating_sub(self.parts) + 1;
            let (parts, more) = mime::split_multipart(body, &boundary, max);
            // Parts past the limit are not read. A last alternative can be one of them.
            self.cut |= more;
            if sub == "alternative" {
                // RFC 2046: the last version is the richest. A mail client
                // shows it, so the clean text uses it too. Walk from the end,
                // so that a version that is not used takes none of the budget.
                // A version with no visible text (empty or all hidden) is skipped.
                for part in parts.into_iter().rev() {
                    let before = (
                        self.body_left,
                        self.cut,
                        self.attach_left,
                        self.attachments.len(),
                    );
                    let bodies = self.entity(part, depth + 1);
                    // Visible means text that stays after the quote and signature steps.
                    let visible = bodies.iter().any(|t| {
                        let t = quote::strip_quotes(&t.replace('\r', ""), self.is_reply);
                        !quote::strip_signature(&t).trim().is_empty()
                    });
                    if visible {
                        return bodies;
                    }
                    // A version with no visible text gives its budgets and attachments back.
                    let listed;
                    (self.body_left, self.cut, self.attach_left, listed) = before;
                    self.attachments.truncate(listed);
                }
                return Vec::new();
            }
            let mut bodies = Vec::new();
            for part in parts {
                bodies.extend(self.entity(part, depth + 1));
            }
            return bodies;
        }
        let is_text = mime_type == "text/plain" || mime_type == "text/html";
        if is_text && disp.main != "attachment" && name.is_none() {
            let bytes = self.take(body, &cte);
            let text = mime::decode(&bytes, charset.as_deref());
            if mime_type != "text/html" {
                return vec![text];
            }
            let (md, full) = html::to_markdown(&text, self.is_reply);
            self.cut |= full;
            return vec![md];
        }
        self.attachment(mime_type, name, charset, body, &cte, depth);
        Vec::new()
    }

    fn attachment(
        &mut self,
        mime_type: String,
        name: Option<String>,
        charset: Option<String>,
        body: &[u8],
        cte: &str,
        depth: usize,
    ) {
        let size = mime::decoded_len(body, cte);
        // An attached message adds its own attachments. This one goes before them.
        let at = self.attachments.len();
        let mut name = name.unwrap_or_default();
        let mut text = None;
        // True if the attachment text is not complete.
        let mut cut = false;
        if mime_type == "message/rfc822" && self.attach_left > 0 {
            let (inner, clipped) = decode_capped(body, cte, INNER_LIMIT);
            // The attached message has its own body budget and cut state, so
            // that it does not take text from the outer message. Its text
            // cannot be longer than the attachment text that is left.
            let outer = (self.body_left, self.cut);
            self.body_left = self.attach_left;
            self.cut = false;
            let (subject, mut t) = self.message(&inner, depth + 1);
            if clipped {
                // The end of the attached message is not read, so the sizes
                // of its own attachments are not known. Do not list them.
                self.attachments.truncate(at);
            }
            if self.cut || clipped {
                t.push_str(CUT_MARK);
            }
            (self.body_left, self.cut) = outer;
            // The body of the attached message has priority over the text of
            // its own attachments: take budget back from them, last first.
            let mut need = t.len().saturating_sub(self.attach_left);
            for a in self.attachments[at..].iter_mut().rev() {
                if need == 0 {
                    break;
                }
                let Some(inner_text) = a.text.as_mut() else {
                    continue;
                };
                let keep = inner_text.len().saturating_sub(need + CUT_MARK.len());
                let freed = if keep == 0 {
                    a.text.take().map_or(0, |x| x.len())
                } else {
                    let before = inner_text.len();
                    inner_text.truncate(floor_char(inner_text, keep));
                    inner_text.push_str(CUT_MARK);
                    before - inner_text.len()
                };
                self.attach_left += freed;
                need = need.saturating_sub(freed);
            }
            if name.is_empty() {
                name = subject;
            }
            text = Some(t);
        } else if mime_type == "text/plain" && self.attach_left > 0 {
            let (bytes, truncated) = decode_capped(body, cte, self.attach_left);
            text = Some(mime::decode(&bytes, charset.as_deref()));
            cut = truncated;
        } else if mime_type == "application/pdf" && self.attach_left > 0 && size <= PDF_LIMIT {
            // Check the marker first, so that a file that is not a PDF costs no memory.
            if pdf::is_pdf(&decode_capped(body, cte, 1024).0) {
                let (bytes, _) = decode_capped(body, cte, PDF_LIMIT as usize);
                // One byte more than the budget, so that a cut shows below.
                text = pdf::text(&bytes, self.attach_left + 1);
            }
        }
        if let Some(t) = text.as_mut() {
            if t.len() > self.attach_left {
                t.truncate(floor_char(t, self.attach_left));
                cut = true;
            }
            if cut {
                t.push_str(CUT_MARK);
            }
            self.attach_left = self.attach_left.saturating_sub(t.len());
        }
        let name: String = name
            .chars()
            .filter(|c| !c.is_control())
            .take(NAME_LIMIT)
            .collect();
        self.attachments.insert(
            at,
            Attachment {
                name: name.trim().to_string(),
                mime_type,
                size,
                text,
            },
        );
    }

    /// Decode at most the bytes that are left in the body budget, and use them.
    fn take(&mut self, body: &[u8], cte: &str) -> Vec<u8> {
        let (bytes, truncated) = decode_capped(body, cte, self.body_left);
        self.cut |= truncated;
        self.body_left -= bytes.len();
        bytes.into_owned()
    }
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
    let truncated = input.len() < body.len() || bytes.len() > cap;
    if bytes.len() > cap {
        bytes.to_mut().truncate(cap);
    }
    (bytes, truncated)
}

use mime::floor_char;

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
    fn tidy_lines() {
        assert_eq!(tidy("\n\na \u{200c}\u{a0}\n\n\n\nb\u{feff}\n \n"), "a\n\nb");
    }

    #[test]
    fn floor_char_finds_a_boundary() {
        assert_eq!(floor_char("aé", 2), 1);
        assert_eq!(floor_char("aé", 9), 3);
    }
}
