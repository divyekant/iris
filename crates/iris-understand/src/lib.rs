//! Understanding: from one mail message to items of the five forms
//! (specification 1, section 7).
//!
//! This part gives steps 1 to 3: [`analyze`] reads the raw bytes of one
//! message and gives its headers, its clean text and its hostile marks.
//! The fixed rules, the model and the store writes come in later parts.
//!
//! Mail text is data, never instructions. Code only compares the text.

mod head;
pub mod time;

use serde_json::Value;

pub use head::{Auth, Head};

/// The kind of the sender of one message (section 6.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The owner of the mailbox: the message has the label `SENT`.
    SelfOwner,
    Person,
    Service,
    Stranger,
    Hostile,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::SelfOwner => "self",
            Kind::Person => "person",
            Kind::Service => "service",
            Kind::Stranger => "stranger",
            Kind::Hostile => "hostile",
        }
    }

    /// The kind of a `sender.kind` value. An unknown value is a stranger.
    pub fn parse(s: &str) -> Kind {
        match s {
            "self" => Kind::SelfOwner,
            "person" => Kind::Person,
            "service" => Kind::Service,
            "hostile" => Kind::Hostile,
            _ => Kind::Stranger,
        }
    }
}

/// A hostile mark of one message (section 6.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    Dmarc,
    Spoof,
    Spam,
    Phishing,
}

impl Mark {
    pub fn name(self) -> &'static str {
        match self {
            Mark::Dmarc => "dmarc",
            Mark::Spoof => "spoof",
            Mark::Spam => "spam",
            Mark::Phishing => "phishing",
        }
    }
}

/// The facts about one message that are not in its bytes.
pub struct Context<'a> {
    /// The address of the owner of the mailbox, lowercase.
    pub me: &'a str,
    /// The sender kind (step 3).
    pub kind: Kind,
    /// Provider IDs of the labels of the message, for example `SPAM`.
    pub labels: &'a [String],
    /// `internalDate` of the message, in milliseconds. Item dates use it.
    pub internal_date: Option<i64>,
    pub now_ms: i64,
    /// The addresses of the known persons with this display name (for the
    /// mark `spoof`). Two persons can have the same name.
    pub known_person: &'a dyn Fn(&str) -> Vec<String>,
    /// The pinned items (with a correction) whose origin is this message.
    /// The model gets them, and an item that it finds again adds nothing.
    pub existing: &'a [Value],
    /// The earlier messages of the thread, oldest first, as short text
    /// blocks. The model gets them as context (section 7.4).
    pub thread: &'a [String],
    /// The clean texts of earlier messages of the same sender in the thread.
    /// An end block that the text shares with one of them goes out, for
    /// example a signature without a `-- ` line (section 7.2).
    pub same_sender: &'a [String],
}

/// The clean text of one message: the only use of `iris-clean`.
#[derive(Clone, Debug, Default)]
pub struct Text {
    pub subject: String,
    pub text: String,
    /// `(name, MIME type, size in bytes)`.
    pub attachments: Vec<(String, String, u64)>,
    /// `(name, text)` of each attachment with text (a plain-text file, an
    /// attached message or a simple PDF), at most [`ATTACHMENT_TEXT_CHARS`]
    /// characters in all.
    pub attachment_text: Vec<(String, String)>,
}

/// The most characters of attachment text that the model gets for one
/// message, in addition to the clean text. It bounds the cost of a call.
pub const ATTACHMENT_TEXT_CHARS: usize = 32_000;

impl Text {
    pub fn of(raw: &[u8]) -> Text {
        let c = iris_clean::clean(raw);
        let mut left = ATTACHMENT_TEXT_CHARS;
        let mut attachment_text = Vec::new();
        for a in &c.attachments {
            let Some(t) = a.text.as_deref().filter(|t| !t.trim().is_empty()) else {
                continue;
            };
            if left == 0 {
                break;
            }
            let t = cut(t, left);
            left -= t.chars().count();
            attachment_text.push((a.name.clone(), t));
        }
        Text {
            subject: c.subject,
            text: c.text,
            attachments: c
                .attachments
                .into_iter()
                .map(|a| (a.name, a.mime_type, a.size))
                .collect(),
            attachment_text,
        }
    }
}

/// Steps 1 to 3 for one message.
#[derive(Clone, Debug)]
pub struct Analysis {
    pub head: Head,
    pub text: Text,
    /// Hostile marks from the headers and the labels. The model can add
    /// [`Mark::Phishing`] later.
    pub marks: Vec<Mark>,
}

impl Analysis {
    /// The date of the message: `internalDate`, else the `Date` header, else now.
    pub fn date(&self, cx: &Context) -> i64 {
        cx.internal_date.or(self.head.date).unwrap_or(cx.now_ms)
    }
}

/// Steps 1 to 3: parse the headers, make the clean text, compute the hostile
/// marks `dmarc`, `spoof` and `spam`.
pub fn analyze(raw: &[u8], cx: &Context) -> Analysis {
    let head = Head::parse(raw);
    let mut text = Text::of(raw);
    if !cx.same_sender.is_empty() {
        let earlier: Vec<&str> = cx.same_sender.iter().map(String::as_str).collect();
        text.text = iris_clean::remove_repeated_end(&text.text, &earlier);
    }
    let mut marks = Vec::new();
    let from = head.from_addr.as_deref().unwrap_or("");
    let from_domain = head::domain(from);
    if let Some(a) = &head.auth {
        let for_from = a.dmarc_from.as_deref().is_none_or(|d| d == from_domain);
        if a.dmarc.as_deref() == Some("fail") && for_from {
            marks.push(Mark::Dmarc);
        }
    }
    if let Some(name) = &head.from_name {
        let known = (cx.known_person)(name);
        if !known.is_empty() {
            let pass = head
                .auth
                .as_ref()
                .is_some_and(|a| a.dkim_pass(from_domain) && a.dmarc.as_deref() == Some("pass"));
            if !known.iter().any(|k| k.eq_ignore_ascii_case(from)) && !pass {
                marks.push(Mark::Spoof);
            }
        }
    }
    if cx.labels.iter().any(|l| l == "SPAM") {
        marks.push(Mark::Spam);
    }
    Analysis { head, text, marks }
}

/// `s` with at most `n` characters.
fn cut(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => s[..i].to_string(),
        None => s.to_string(),
    }
}
