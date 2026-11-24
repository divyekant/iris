//! The policy file of Iris (spec 2, section 6): the rules in `policy.toml`
//! that decide which actions apply without review, and which items the brief
//! hides or bundles, and which new mail gives a notice.
//!
//! This crate reads and checks the file, and evaluates actions and items.
//! It keeps the active policy in `iris.db` and confirms a changed file
//! (section 6.3). It writes the file only for a change that the person makes
//! in the web client (`change`, `discard`), and keeps its comments.

mod active;
mod attention;
pub use attention::{decide_action, facts, refresh as refresh_attention, Facts, PASS_EVERY};
mod edit;
mod eval;
pub mod notice;
pub mod propose;

pub use active::{
    active, confirm, file_state, next_rule_id, version_text, Active, Confirm, FileState,
};
pub use edit::{
    change, discard, edit_text, end_grants, grant_rule, Change, Discarded, Edited, DEFAULT_TEXT,
};
pub use eval::{Action, Attention, Decision, Item, Message, Outcome};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::Path;

/// The name of the policy file in the data folder.
pub const FILE: &str = "policy.toml";

/// The largest policy file that Iris reads. Spec 2 section 9 keeps the parsed
/// policy under 1 MB. About 1,000 rules of 250 bytes fit.
pub const MAX_BYTES: usize = 256 * 1024;

/// The largest number of rules in one file.
pub const MAX_RULES: usize = 1000;

/// Defines a small enum of fixed names with `name` and `from_name`.
macro_rules! names {
    ($(#[$m:meta])* $ty:ident { $($v:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $ty { $($v),+ }

        impl $ty {
            pub const ALL: &'static [$ty] = &[$($ty::$v),+];

            pub fn name(self) -> &'static str {
                match self { $($ty::$v => $s),+ }
            }

            pub fn from_name(s: &str) -> Option<$ty> {
                Self::ALL.iter().copied().find(|v| v.name() == s)
            }
        }
    };
}

names!(
    /// What the engine does with the result of the rules. `observe`: Iris
    /// applies nothing, and each action stays `proposed`. `apply`: the rules
    /// decide, and the rest goes to review.
    Mode { Observe = "observe", Apply = "apply" }
);

names!(
    /// The effect of a rule.
    Effect { Allow = "allow", Review = "review", Deny = "deny", Hide = "hide", Bundle = "bundle", Notify = "notify" }
);

impl Effect {
    fn is_action(self) -> bool {
        matches!(self, Effect::Allow | Effect::Review | Effect::Deny)
    }
}

/// The verbs of release 2 (spec 2, section 4), and `skip`, `mute` and
/// `dismiss` of release 5 (docs/specs/2026-11-17-preference-rules.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verb {
    Archive,
    Inbox,
    Label,
    Unlabel,
    Read,
    Unread,
    Trash,
    Close,
    Defer,
    Decline,
    Correct,
    Skip,
    Mute,
    Dismiss,
}

impl Verb {
    pub const ALL: [Verb; 14] = [
        Verb::Archive,
        Verb::Inbox,
        Verb::Label,
        Verb::Unlabel,
        Verb::Read,
        Verb::Unread,
        Verb::Trash,
        Verb::Close,
        Verb::Defer,
        Verb::Decline,
        Verb::Correct,
        Verb::Skip,
        Verb::Mute,
        Verb::Dismiss,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Verb::Archive => "archive",
            Verb::Inbox => "inbox",
            Verb::Label => "label",
            Verb::Unlabel => "unlabel",
            Verb::Read => "read",
            Verb::Unread => "unread",
            Verb::Trash => "trash",
            Verb::Close => "close",
            Verb::Defer => "defer",
            Verb::Decline => "decline",
            Verb::Correct => "correct",
            Verb::Skip => "skip",
            Verb::Mute => "mute",
            Verb::Dismiss => "dismiss",
        }
    }

    pub fn from_name(s: &str) -> Option<Verb> {
        Verb::ALL.into_iter().find(|v| v.name() == s)
    }
}

names!(
    /// The five forms of spec 1 section 6.
    Form { Commitment = "commitment", Conversation = "conversation", Record = "record", Feed = "feed", Signal = "signal" }
);
names!(
    /// The sender kinds of spec 1 section 6.7, with the names of `sender.kind`.
    SenderKind { SelfOwner = "self", Person = "person", Service = "service", Stranger = "stranger", Hostile = "hostile" }
);
names!(
    /// The record types of spec 1 section 6.4.
    RecordType { Receipt = "receipt", Booking = "booking", Statement = "statement", Ticket = "ticket", Contract = "contract", Event = "event", Other = "other" }
);
names!(
    /// The commitment kinds of spec 1 section 6.2.
    CommitmentKind { Task = "task", Reply = "reply", Payment = "payment", Invitation = "invitation", Decision = "decision" }
);
names!(
    /// The author kinds of a changeset (spec 2, section 5.1).
    Author { Person = "person", Agent = "agent", Triage = "triage", Local = "local" }
);

names!(
    /// Who wrote a rule.
    By { Person = "person", Proposal = "proposal" }
);

/// The conditions of a rule. Each condition that is set must match.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct When {
    pub form: Option<Form>,
    /// An address, or a domain with `@` in front. Lower case.
    pub sender: Option<String>,
    pub sender_kind: Option<SenderKind>,
    pub label: Option<String>,
    pub record_type: Option<RecordType>,
    pub commitment_kind: Option<CommitmentKind>,
    pub author: Option<Author>,
    pub older_than_days: Option<u32>,
}

/// One rule of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub id: u32,
    pub name: String,
    pub effect: Effect,
    /// Empty for `hide`, `bundle` and `notify`.
    pub verbs: Vec<Verb>,
    pub when: When,
    pub by: By,
    /// The date, as `YYYY-MM-DD`.
    pub added: String,
    pub job: Option<String>,
    /// The line of the rule in the file, from 1.
    pub line: usize,
}

/// A policy that passed each check. Only this type can be evaluated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub mode: Mode,
    /// The rules in the order of the file.
    pub rules: Vec<Rule>,
}

impl Default for Policy {
    /// The policy of the first start: the mode `observe` and no rules.
    fn default() -> Policy {
        Policy {
            mode: Mode::Observe,
            rules: Vec::new(),
        }
    }
}

/// A policy file that is not valid. `line` is from 1, and 0 when the error
/// has no place in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}: {}", FILE, self.message)
        } else {
            write!(f, "{} line {}: {}", FILE, self.line, self.message)
        }
    }
}

impl std::error::Error for Error {}

/// A policy file read from disk: the policy, the text and its SHA-256 hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub policy: Policy,
    pub text: String,
    /// The SHA-256 hash of the text, in lower-case hex.
    pub sha256: String,
}

/// Reads `policy.toml` from the data folder. Gives `Ok(None)` when the file
/// does not exist.
pub fn load(data_dir: &Path) -> Result<Option<Loaded>, Error> {
    use std::io::Read;
    let path = data_dir.join(FILE);
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(no_line(format!("cannot read the file: {e}"))),
    };
    // Read one byte more than the limit, so that a larger file is refused
    // without a read of all of it.
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| no_line(format!("cannot read the file: {e}")))?;
    let text = String::from_utf8(bytes).map_err(|_| no_line("the file is not UTF-8 text"))?;
    let policy = parse(&text)?;
    Ok(Some(Loaded {
        sha256: sha256_hex(&text),
        policy,
        text,
    }))
}

/// The SHA-256 hash of a policy text, in lower-case hex.
pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn no_line(message: impl Into<String>) -> Error {
    Error {
        line: 0,
        message: message.into(),
    }
}

// The file as TOML gives it. Unknown fields are errors: a field with a typo
// must not make a rule wider than the person wrote it.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    version: i64,
    mode: String,
    #[serde(default)]
    rule: Vec<toml::Spanned<RawRule>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    id: i64,
    name: String,
    effect: String,
    verbs: Option<Vec<String>>,
    when: RawWhen,
    by: String,
    added: toml::value::Datetime,
    job: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWhen {
    form: Option<String>,
    sender: Option<String>,
    sender_kind: Option<String>,
    label: Option<String>,
    record_type: Option<String>,
    commitment_kind: Option<String>,
    author: Option<String>,
    older_than_days: Option<i64>,
}

/// Reads and checks the text of a policy file. The error names the line.
pub fn parse(text: &str) -> Result<Policy, Error> {
    if text.len() > MAX_BYTES {
        return Err(no_line(format!(
            "the file is larger than {} KiB",
            MAX_BYTES / 1024
        )));
    }
    let raw: RawFile = toml::from_str(text).map_err(|e| Error {
        line: e.span().map_or(0, |s| line_of(text, s.start)),
        message: e.message().trim().to_string(),
    })?;
    // Version 0 and version 1 have the same meaning (spec 4, rule 2.7).
    if raw.version != 0 && raw.version != 1 {
        return Err(Error {
            line: find_line(text, "version"),
            message: format!("version {} is not known; use version = 1", raw.version),
        });
    }
    let mode = match raw.mode.as_str() {
        "observe" => Mode::Observe,
        "apply" => Mode::Apply,
        m => {
            return Err(Error {
                line: find_line(text, "mode"),
                message: format!("mode \"{m}\" is not known; use \"observe\" or \"apply\""),
            })
        }
    };
    if raw.rule.len() > MAX_RULES {
        return Err(no_line(format!("the file has more than {MAX_RULES} rules")));
    }
    let mut rules: Vec<Rule> = Vec::with_capacity(raw.rule.len());
    for spanned in raw.rule {
        let line = line_of(text, spanned.span().start);
        let rule =
            check_rule(spanned.into_inner(), line).map_err(|message| Error { line, message })?;
        if let Some(first) = rules.iter().find(|r| r.id == rule.id) {
            return Err(Error {
                line,
                message: format!(
                    "the id {} is also the id of the rule on line {}",
                    rule.id, first.line
                ),
            });
        }
        rules.push(rule);
    }
    Ok(Policy { mode, rules })
}

fn check_rule(raw: RawRule, line: usize) -> Result<Rule, String> {
    let id = u32::try_from(raw.id)
        .ok()
        .filter(|&id| id > 0)
        .ok_or_else(|| format!("the id {} is not a positive integer", raw.id))?;
    // Check the text as written, before the trim can remove a line break.
    if raw.name.chars().any(char::is_control) {
        return Err("the name must be one line of text".into());
    }
    let name = raw.name.trim().to_string();
    if name.is_empty() {
        return Err("the name is empty".into());
    }
    let effect = match raw.effect.as_str() {
        "allow" => Effect::Allow,
        "review" => Effect::Review,
        "deny" => Effect::Deny,
        "hide" => Effect::Hide,
        "bundle" => Effect::Bundle,
        "notify" => Effect::Notify,
        e => {
            return Err(format!(
                "the effect \"{e}\" is not known; use allow, review, deny, hide, bundle or notify"
            ))
        }
    };
    let verbs = match (effect.is_action(), raw.verbs) {
        (true, None) => return Err(format!("an {} rule needs verbs", raw.effect)),
        (true, Some(v)) if v.is_empty() => return Err("verbs is empty".into()),
        (true, Some(v)) => v
            .iter()
            .map(|s| Verb::from_name(s).ok_or_else(|| format!("the verb \"{s}\" is not known")))
            .collect::<Result<Vec<_>, _>>()?,
        (false, None) => Vec::new(),
        (false, Some(_)) => return Err(format!("a {} rule has no verbs", raw.effect)),
    };
    let when = check_when(raw.when)?;
    if !effect.is_action() && (when.author.is_some() || raw.job.is_some()) {
        // An item has no author and no job. Such a rule could never match.
        return Err(format!("a {} rule cannot have author or job", raw.effect));
    }
    let by = match raw.by.as_str() {
        "person" => By::Person,
        "proposal" => By::Proposal,
        b => return Err(format!("by \"{b}\" is not known; use person or proposal")),
    };
    let added = match (raw.added.date, raw.added.time, raw.added.offset) {
        (Some(d), None, None) => d.to_string(),
        _ => return Err("added must be a date such as 2026-11-16".into()),
    };
    let job = match raw.job {
        Some(j) if j.trim().is_empty() => return Err("job is empty".into()),
        j => j,
    };
    Ok(Rule {
        id,
        name,
        effect,
        verbs,
        when,
        by,
        added,
        job,
        line,
    })
}

fn check_when(raw: RawWhen) -> Result<When, String> {
    fn one<T>(
        v: Option<String>,
        what: &str,
        f: fn(&str) -> Option<T>,
    ) -> Result<Option<T>, String> {
        v.map(|s| f(&s).ok_or_else(|| format!("{what} \"{s}\" is not known")))
            .transpose()
    }
    let when = When {
        form: one(raw.form, "form", Form::from_name)?,
        sender: raw.sender.map(check_sender).transpose()?,
        sender_kind: one(raw.sender_kind, "sender_kind", SenderKind::from_name)?,
        label: match raw.label {
            Some(l) if l.trim().is_empty() => return Err("label is empty".into()),
            l => l,
        },
        record_type: one(raw.record_type, "record_type", RecordType::from_name)?,
        commitment_kind: one(
            raw.commitment_kind,
            "commitment_kind",
            CommitmentKind::from_name,
        )?,
        author: one(raw.author, "author", Author::from_name)?,
        older_than_days: raw
            .older_than_days
            .map(|d| {
                u32::try_from(d).map_err(|_| format!("older_than_days {d} is not a number of days"))
            })
            .transpose()?,
    };
    if when == When::default() {
        // An empty `when` matches each message. A person must write that wish
        // as a condition, so that no rule is wide by accident.
        return Err("when has no condition".into());
    }
    Ok(when)
}

/// Checks `name@domain` or `@domain`, and gives it in lower case.
pub(crate) fn check_sender(s: String) -> Result<String, String> {
    let lower = s.trim().to_lowercase();
    let ok = match lower.split_once('@') {
        Some((local, domain)) => {
            !domain.is_empty()
                && !domain.contains('@')
                && !local.chars().any(char::is_whitespace)
                && !domain.chars().any(char::is_whitespace)
        }
        None => false,
    };
    if ok {
        Ok(lower)
    } else {
        Err(format!("sender \"{s}\" is not an address or an @domain"))
    }
}

/// The line, from 1, of a byte offset in the text.
fn line_of(text: &str, offset: usize) -> usize {
    let end = offset.min(text.len());
    text.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// The line of the first top-level key `key`, or 0.
fn find_line(text: &str, key: &str) -> usize {
    text.lines()
        .position(|l| {
            l.trim_start()
                .strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with('='))
        })
        .map_or(0, |i| i + 1)
}

#[cfg(test)]
mod tests;
