//! Understanding: from one mail message to items of the five forms
//! (specification 1, section 7).
//!
//! This part gives steps 1 to 3: [`analyze`] reads the raw bytes of one
//! message and gives its headers, its clean text and its hostile marks.
//! The fixed rules, the model and the store writes come in later parts.
//!
//! Mail text is data, never instructions. Code only compares the text.

pub mod head;
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
    /// attached message or a simple PDF), all the text that `iris-clean`
    /// extracts (at most `iris_clean::ATTACHMENT_TEXT_LIMIT` bytes in all,
    /// specification 1, section 7.2). The model call bounds its own data.
    pub attachment_text: Vec<(String, String)>,
    /// `iris-clean` cut the clean text at its limit.
    pub cut: bool,
    /// A `text/calendar` part with the method `REQUEST` (from `iris-clean`).
    pub invitation: bool,
}

impl Text {
    pub fn of(raw: &[u8]) -> Text {
        let c = iris_clean::clean(raw);
        let mut attachment_text = Vec::new();
        let mut attachments = Vec::with_capacity(c.attachments.len());
        for a in c.attachments {
            if let Some(t) = a.text.filter(|t| !t.trim().is_empty()) {
                attachment_text.push((a.name.clone(), t));
            }
            attachments.push((a.name, a.mime_type, a.size));
        }
        Text {
            subject: c.subject,
            text: c.text,
            attachments,
            attachment_text,
            cut: c.cut,
            invitation: c.invitation,
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
        // The cut mark is not part of the text: it stays at the end.
        let strip = |t: &str| {
            t.strip_suffix(iris_clean::CUT_MARK)
                .unwrap_or(t)
                .to_string()
        };
        let earlier: Vec<String> = cx.same_sender.iter().map(|t| strip(t)).collect();
        let earlier: Vec<&str> = earlier.iter().map(String::as_str).collect();
        let had_mark = text.text.ends_with(iris_clean::CUT_MARK);
        text.text = iris_clean::remove_repeated_end(&strip(&text.text), &earlier);
        if had_mark {
            text.text.push_str(iris_clean::CUT_MARK);
        }
    }
    let mut marks = Vec::new();
    let from = head.from_addr.as_deref().unwrap_or("");
    let from_domain = head::domain(from);
    // A DMARC result counts only with its `header.from` domain, and only
    // for the domain of this From address.
    let dmarc = head
        .auth
        .as_ref()
        .filter(|a| {
            a.dmarc_from
                .as_deref()
                .is_some_and(|d| d.eq_ignore_ascii_case(from_domain))
        })
        .and_then(|a| a.dmarc.as_deref());
    if dmarc == Some("fail") {
        marks.push(Mark::Dmarc);
    }
    if let Some(name) = &head.from_name {
        let known = (cx.known_person)(name);
        if !known.is_empty() {
            let pass = dmarc == Some("pass")
                && head.auth.as_ref().is_some_and(|a| a.dkim_pass(from_domain));
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
