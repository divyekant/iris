//! Understanding: from one mail message to items of the five forms
//! (specification 1, section 7).
//!
//! The pure part reads the raw bytes of one message and gives drafts of
//! items: [`analyze`] (steps 1 to 3: headers, clean text, hostile marks),
//! then [`decide`] (steps 5 to 7: the fixed rules, then the model). The
//! store part, [`understand`], reads one message of the store, runs the
//! steps, and saves the result in step 8.
//!
//! Mail text is data, never instructions. The rules only compare text. The
//! model gets the mail only inside the data block of `iris-model`, and has
//! no tools. Hostile mail never goes to a model and makes no item.
//!
//! The engine makes conversations with a separate rule (section 7.3, crate
//! `iris-talk`), so this crate never makes a conversation item.

mod head;
mod prompt;
mod rules;
mod schema;
mod store;
pub mod time;

use serde_json::{json, Value};

pub use head::{Auth, Head};
pub use iris_model::Usage;
pub use prompt::{INSTRUCTION, PROMPT};
pub use schema::validate;
pub use store::{understand, Error, Outcome, Result as StoreResult, MAX_TRIES, RETRY_MS};

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
    /// The address of the known person with this display name, if there is
    /// one (for the mark `spoof`).
    pub known_person: &'a dyn Fn(&str) -> Option<String>,
    /// The pinned items (with a correction) whose origin is this message.
    /// The model gets them, and an item that it finds again adds nothing.
    pub existing: &'a [Value],
}

/// The clean text of one message: the only use of `iris-clean`.
#[derive(Clone, Debug, Default)]
pub struct Text {
    pub subject: String,
    pub text: String,
    /// `(name, MIME type, size in bytes)`.
    pub attachments: Vec<(String, String, u64)>,
}

impl Text {
    pub fn of(raw: &[u8]) -> Text {
        let c = iris_clean::clean(raw);
        Text {
            subject: c.subject,
            text: c.text,
            attachments: c
                .attachments
                .into_iter()
                .map(|a| (a.name, a.mime_type, a.size))
                .collect(),
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
    let text = Text::of(raw);
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
        if let Some(known) = (cx.known_person)(name) {
            let pass = head
                .auth
                .as_ref()
                .is_some_and(|a| a.dkim_pass(from_domain) && a.dmarc.as_deref() == Some("pass"));
            if !known.eq_ignore_ascii_case(from) && !pass {
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
/// Each party of a sender has its address, and no `p-` ID yet.
#[derive(Clone, Debug)]
pub struct Draft {
    pub item: Value,
    /// The words of the message that support the item.
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

    /// The complete item of the open model. `sender` gives the `p-` row ID of
    /// an address and a display name.
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
        for p in party_slots(&mut v) {
            let Some(o) = p.as_object_mut() else { continue };
            if o.get("kind").and_then(Value::as_str) != Some("sender") {
                continue;
            }
            let Some(addr) = o.get("address").and_then(Value::as_str).map(str::to_string) else {
                continue;
            };
            let name = o.get("name").and_then(Value::as_str).map(str::to_string);
            o.insert(
                "sender".into(),
                json!(format!("p-{}", sender(&addr, name.as_deref()))),
            );
        }
        v
    }
}

/// The row IDs that [`Draft::finish`] needs.
#[derive(Clone, Copy, Debug)]
pub struct Ids {
    pub message: i64,
    pub thread: i64,
    pub now_ms: i64,
}

fn party_slots(v: &mut Value) -> Vec<&mut Value> {
    let Some(o) = v.as_object_mut() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (k, val) in o.iter_mut() {
        match k.as_str() {
            "owner" | "counterparty" | "issuer" | "publisher" => out.push(val),
            "fields" => {
                if let Some(Value::Array(list)) = val.get_mut("parties") {
                    out.extend(list.iter_mut());
                }
            }
            _ => {}
        }
    }
    out
}

/// The party of the sender of the message. A message from the owner (label
/// `SENT`) gives `self` through the caller's kind, not here.
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

/// One model: `iris_model::Client` with its model name.
pub trait Model {
    /// The model name for `derived.model`.
    fn name(&self) -> &str;
    fn ask(
        &self,
        instruction: &str,
        data: &str,
    ) -> std::result::Result<iris_model::Reply, iris_model::Error>;
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

    fn ask(
        &self,
        instruction: &str,
        data: &str,
    ) -> std::result::Result<iris_model::Reply, iris_model::Error> {
        self.client.ask(instruction, data)
    }
}

/// The result of steps 5 to 7 for one message.
#[derive(Clone, Debug, Default)]
pub struct Decision {
    /// Valid drafts. Empty for hostile mail.
    pub drafts: Vec<Draft>,
    /// Hostile mail: a hostile mark (with `phishing` from the model) or a
    /// `hostile` sender.
    pub hostile: bool,
    /// The model answered `phishing` true.
    pub phishing: bool,
    /// Two model answers in a row were not valid: no item, one more try.
    pub failed: bool,
    pub calls: u32,
    pub usage: Usage,
}

/// Steps 5 to 7: hostile mail gives nothing, then the fixed rules, then the
/// model, with one more try after an answer that is not valid. A model
/// error that a new try can fix, or a refused call, is an `Err`: the message
/// stays not understood, and no try counts.
pub fn decide(
    a: &Analysis,
    cx: &Context,
    model: &dyn Model,
) -> std::result::Result<Decision, iris_model::Error> {
    let mut d = Decision::default();
    if !a.marks.is_empty() || cx.kind == Kind::Hostile {
        d.hostile = true;
        return Ok(d);
    }
    if let Some(mut drafts) = rules::apply(a, cx) {
        if drafts.iter().all(|x| check(x).is_ok()) {
            // A rule finds an existing item again by its form and its rule.
            drafts.retain(|x| {
                let By::Rule(rule) = x.by else { return true };
                !cx.existing.iter().any(|e| {
                    e["form"].as_str() == Some(x.form())
                        && e["derived"]["rule"].as_str() == Some(rule)
                })
            });
            d.drafts = drafts;
            return Ok(d);
        }
    }
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
            Err(e) => return Err(e),
        };
        d.calls += 1;
        add(&mut d.usage, reply.usage);
        if reply.stop != iris_model::Stop::End {
            continue;
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
        if drafts.iter().all(|x| check(x).is_ok()) {
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

/// Validate a draft as a complete item with placeholder IDs.
fn check(d: &Draft) -> std::result::Result<(), String> {
    let ids = Ids {
        message: 1,
        thread: 1,
        now_ms: 0,
    };
    validate(&d.finish(&ids, 1, "check", &mut |_, _| 1))
}
