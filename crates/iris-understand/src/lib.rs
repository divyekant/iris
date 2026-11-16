//! Understanding: from one mail message to items of the five forms
//! (specification 1, section 7).
//!
//! Steps 1 to 3: [`analyze`] reads the raw bytes of one message and gives
//! its headers, its clean text and its hostile marks. Steps 5 to 7:
//! [`decide`] runs the hostile check, the fixed rules ([`rules::apply`]) and
//! then the model, and checks each item against the open model schema
//! ([`validate`]). The store writes come in a later part.
//!
//! Mail text is data, never instructions. Code only compares the text.

pub mod head;
pub mod prompt;
pub mod rules;
mod schema;
pub mod store;
pub mod time;

use serde_json::{json, Value};

pub use head::{Auth, Head};
pub use iris_model::Usage;
pub use prompt::{INSTRUCTION, PROMPT};
pub use schema::validate;

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
    /// Known persons that the model can name as parties, as
    /// `Name <address>` lines: `store::understand` gives the persons in the
    /// headers of the thread. The data holds at most `prompt::MAX_KNOWN` of them,
    /// in the budget (section 7.4).
    pub known: &'a [String],
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
    // Mail with the label SENT is yours (spec 1 section 6.7): its own
    // `From` header cannot make it a spoof or a DMARC fail.
    let sent = cx.labels.iter().any(|l| l == "SENT");
    if dmarc == Some("fail") && !sent {
        marks.push(Mark::Dmarc);
    }
    if let Some(name) = head.from_name.as_ref().filter(|_| !sent) {
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

/// Who made a draft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum By {
    /// A fixed rule, with its name.
    Rule(&'static str),
    Model,
}

/// An item before the engine gives it an ID, sources, times and `derived`.
/// The party of a sender has its address, and no `p-` ID yet.
#[derive(Clone, Debug)]
pub struct Draft {
    pub item: Value,
    /// The words of the message that support the item, at most 500
    /// characters.
    pub quote: String,
    pub by: By,
}

impl Draft {
    fn rule(item: Value, rule: &'static str, quote: &str) -> Draft {
        Draft {
            item,
            quote: cut(quote, 500),
            by: By::Rule(rule),
        }
    }

    fn model(item: Value, quote: &str) -> Draft {
        Draft {
            item,
            quote: cut(quote, 500),
            by: By::Model,
        }
    }

    pub fn form(&self) -> &str {
        self.item["form"].as_str().unwrap_or("")
    }

    /// The complete item of the open model: the fields that the store owns
    /// (ID, source, times, `derived`) and the `p-` ID of each sender party.
    /// `sender` gives the row ID of the sender row of an address and a name.
    pub fn finish(
        &self,
        ids: &Ids,
        id: i64,
        model: &str,
        sender: &mut dyn FnMut(&str, Option<&str>) -> i64,
    ) -> Value {
        let mut v = self.item.clone();
        let prefix = match self.form() {
            "commitment" => "c",
            "record" => "r",
            "feed" => "f",
            "signal" => "s",
            _ => "x",
        };
        let now = time::rfc3339(ids.now_ms);
        v["model"] = json!("iris/0");
        v["id"] = json!(format!("{prefix}-{id}"));
        v["sources"] = json!([{
            "message": format!("m-{}", ids.message),
            "thread": format!("t-{}", ids.thread),
            "role": "origin",
            "quote": self.quote,
        }]);
        v["created"] = json!(now);
        v["updated"] = json!(now);
        v["derived"] = match &self.by {
            By::Rule(r) => json!({"by": "rule", "rule": r}),
            By::Model => json!({"by": "model", "model": model, "prompt": PROMPT}),
        };
        for key in ["owner", "counterparty", "issuer", "publisher"] {
            if let Some(p) = v.get_mut(key) {
                set_sender(p, sender);
            }
        }
        if let Some(Value::Array(list)) = v.pointer_mut("/fields/parties") {
            for p in list {
                set_sender(p, sender);
            }
        }
        v
    }
}

/// The row IDs and the time that [`Draft::finish`] needs.
#[derive(Clone, Copy, Debug)]
pub struct Ids {
    pub message: i64,
    pub thread: i64,
    pub now_ms: i64,
}

/// Add the `p-` ID to a `sender` party with an address.
fn set_sender(p: &mut Value, sender: &mut dyn FnMut(&str, Option<&str>) -> i64) {
    let Some(o) = p.as_object_mut() else { return };
    if o.get("kind").and_then(Value::as_str) != Some("sender") {
        return;
    }
    let Some(addr) = o.get("address").and_then(Value::as_str).map(str::to_string) else {
        return;
    };
    let name = o.get("name").and_then(Value::as_str).map(str::to_string);
    o.insert(
        "sender".into(),
        json!(format!("p-{}", sender(&addr, name.as_deref()))),
    );
}

/// The status of a new feed item: `new` while the message has the label
/// `UNREAD` (section 6.8), else `read`.
fn feed_status(cx: &Context) -> &'static str {
    if cx.labels.iter().any(|l| l == "UNREAD") {
        "new"
    } else {
        "read"
    }
}

/// One model: `iris_model::Client` with its model name. Tests use a fake.
pub trait Model {
    /// The model name for `derived.model`.
    fn name(&self) -> &str;
    fn ask(&self, instruction: &str, data: &str) -> Result<iris_model::Reply, iris_model::Error>;
}

/// An `iris_model::Client` and the name of its model.
pub struct Named {
    pub client: iris_model::Client,
    pub name: String,
}

impl Model for Named {
    fn name(&self) -> &str {
        &self.name
    }

    fn ask(&self, instruction: &str, data: &str) -> Result<iris_model::Reply, iris_model::Error> {
        self.client.ask(instruction, data)
    }
}

/// The result of steps 5 to 7 for one message.
#[derive(Clone, Debug, Default)]
pub struct Decision {
    /// Valid drafts. Empty for hostile mail.
    pub drafts: Vec<Draft>,
    /// Hostile mail: a hostile mark, a `hostile` sender, or `phishing` from
    /// the model.
    pub hostile: bool,
    /// The model answered `phishing` true.
    pub phishing: bool,
    /// Two model answers in a row were not valid: no item, one more try.
    pub failed: bool,
    /// No rule matched and the retry state holds the model call: nothing
    /// to save.
    pub waiting: bool,
    pub calls: u32,
    pub usage: Usage,
}

/// Steps 5 to 7: hostile mail gives nothing, then the fixed rules, then the
/// model, with one more try after an answer that is not valid. A model
/// error that a new try can fix, or a refused key, is an `Err`: the message
/// stays not understood, and no try counts. A refusal, or a call that the
/// server refuses for this data, gives `failed` with no second call.
///
/// `model` is `None` when the retry state holds the model call (section
/// 7.4). The hostile check and the fixed rules run in all cases.
pub fn decide(
    a: &Analysis,
    cx: &Context,
    model: Option<&dyn Model>,
) -> Result<Decision, iris_model::Error> {
    let mut d = Decision::default();
    if !a.marks.is_empty() || cx.kind == Kind::Hostile {
        d.hostile = true;
        return Ok(d);
    }
    if let Some(mut drafts) = rules::apply(a, cx) {
        // A rule finds an existing item again by its form and its rule.
        drafts.retain(|x| {
            let By::Rule(rule) = x.by else { return true };
            !cx.existing.iter().any(|e| {
                e["form"].as_str() == Some(x.form()) && e["derived"]["rule"].as_str() == Some(rule)
            })
        });
        d.drafts = drafts;
        return Ok(d);
    }
    let Some(model) = model else {
        d.waiting = true;
        return Ok(d);
    };
    let data = prompt::data(a, cx);
    for attempt in 0..2 {
        let instruction = if attempt == 0 {
            INSTRUCTION.to_string()
        } else {
            format!("{INSTRUCTION}{}", prompt::RETRY_NOTE)
        };
        let reply = match model.ask(&instruction, &data) {
            Ok(r) => r,
            // HTTP 200 with a body of an unknown format: the answer is not valid.
            Err(iris_model::Error::BadAnswer(_)) => {
                d.calls += 1;
                continue;
            }
            // A call that the server refuses for this data (HTTP 400, 413,
            // 422, ...) fails the same way again: a try counts. A refused key
            // (401, 403) stops the batch.
            Err(iris_model::Error::Rejected { status, .. }) if status != 401 && status != 403 => {
                d.calls += 1;
                break;
            }
            Err(e) => return Err(e),
        };
        d.calls += 1;
        add(&mut d.usage, reply.usage);
        match reply.stop {
            iris_model::Stop::End => {}
            // The model refuses this mail: a new try gets the same answer,
            // so the try counts and the message waits a day (section 7.4).
            iris_model::Stop::Refused => break,
            iris_model::Stop::Truncated => continue,
        }
        let Some((drafts, phishing)) = reply.json().ok().and_then(|j| prompt::read(&j, a, cx))
        else {
            continue;
        };
        if phishing {
            d.phishing = true;
            d.hostile = true;
            return Ok(d);
        }
        if drafts.iter().all(schema::valid_draft) {
            d.drafts = drafts;
            return Ok(d);
        }
    }
    d.failed = true;
    Ok(d)
}

fn add(sum: &mut Usage, u: Usage) {
    sum.input_tokens += u.input_tokens;
    sum.output_tokens += u.output_tokens;
    sum.cost_micro_usd = match (sum.cost_micro_usd, u.cost_micro_usd) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
    };
}

/// The party of the sender of the message.
fn party_sender(a: &Analysis) -> Value {
    let mut p = json!({"kind": "sender"});
    if let Some(ad) = &a.head.from_addr {
        p["address"] = json!(ad);
    }
    if let Some(n) = &a.head.from_name {
        p["name"] = json!(cut(n, 200));
    }
    p
}

/// `s` with at most `n` characters.
fn cut(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => s[..i].to_string(),
        None => s.to_string(),
    }
}
