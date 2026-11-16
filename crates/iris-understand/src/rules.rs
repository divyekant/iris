//! The fixed rules of specification 1, section 7.3. They run before the
//! model: fast, free and the same each time. Mail text is data: the rules
//! only compare text.

use serde_json::{json, Value};

use crate::{cut, feed_status, party_sender, time, Analysis, Context, Draft, Kind};

/// The default life of a code signal (specification 1, section 6.6).
const CODE_LIFE_MS: i64 = 15 * 60 * 1000;
/// The most characters of a quote (the schema of a source).
const QUOTE_CHARS: usize = 500;
const CODE_WORDS: [&str; 5] = ["code", "verification", "one-time", "one time", "passcode"];
const RECORD_WORDS: [&str; 8] = [
    "receipt",
    "invoice",
    "order confirmation",
    "your order",
    "statement",
    "payment received",
    "order #",
    "total:",
];

/// The items of the first rule that matches, or `None` if no rule matches
/// and the message goes to the model. Hostile mail (a hostile mark or a
/// `hostile` sender) matches the hostile rule: no item and no model call.
///
/// The quote of a rule item is the line that the rule matched: the subject
/// for a list and an invitation (`(no subject)` if it is empty), the code
/// line for a code (spec 1 section 7.5).
pub fn apply(a: &Analysis, cx: &Context) -> Option<Vec<Draft>> {
    let drafts = matched(a, cx)?;
    // A rule item that the schema refuses (for example a From address that
    // is not a valid email address) does not count: the model reads the
    // message.
    drafts
        .iter()
        .all(crate::schema::valid_draft)
        .then_some(drafts)
}

fn matched(a: &Analysis, cx: &Context) -> Option<Vec<Draft>> {
    if cx.kind == Kind::Hostile || !a.marks.is_empty() {
        return Some(Vec::new());
    }
    let subject = a.text.subject.as_str();
    let quote = or_none(subject);
    if a.text.invitation && cx.kind != Kind::SelfOwner {
        let owner = json!({ "kind": "self" });
        let item = json!({
            "form": "commitment",
            "status": "open",
            "title": cut(&format!("Invitation: {}", or_none(subject)), 200),
            "direction": "you_owe",
            "owner": owner,
            "counterparty": party_sender(a),
            "kind": "invitation",
        });
        return Some(vec![Draft::rule(item, "invitation", quote)]);
    }
    if cx.kind == Kind::Service {
        if let Some((code, line)) = find_code(subject, &a.text.text) {
            let at = a.date(cx);
            let expires = at + CODE_LIFE_MS;
            let issuer = party_sender(a);
            let who = a
                .head
                .from_name
                .clone()
                .or_else(|| a.head.from_addr.clone())
                .unwrap_or_default();
            let item = json!({
                "form": "signal",
                "status": if expires <= cx.now_ms { "expired" } else { "active" },
                "title": cut(&format!("Code from {who}"), 200),
                "kind": "code",
                "issuer": issuer,
                "code": code,
                "expires": time::rfc3339(expires),
            });
            return Some(vec![Draft::rule(item, "code", line)]);
        }
        if (a.head.list_id.is_some() || a.head.list_unsubscribe) && !looks_like_record(a) {
            let mut item = json!({
                "form": "feed",
                "status": feed_status(cx),
                "title": cut(or_none(subject), 200),
                "publisher": party_sender(a),
                "published": time::rfc3339(a.date(cx)),
            });
            // A longer List-Id is not cut: a cut ID names another list.
            if let Some(l) = a.head.list_id.as_ref().filter(|l| l.chars().count() <= 500) {
                item["list"] = Value::String(l.clone());
            }
            return Some(vec![Draft::rule(item, "list", quote)]);
        }
    }
    None
}

/// The subject, or `(no subject)` if it is empty.
fn or_none(s: &str) -> &str {
    if s.trim().is_empty() {
        "(no subject)"
    } else {
        s
    }
}

fn looks_like_record(a: &Analysis) -> bool {
    // The whole clean text: it is bounded, and a receipt can come late.
    let hay = format!("{}\n{}", a.text.subject, a.text.text).to_lowercase();
    RECORD_WORDS.iter().any(|w| hay.contains(w))
}

/// The first code in the order of the message. Each line with a code
/// word gives one candidate: a code of 4 to 8 digits on that line, else the
/// next line if it is only such a code. A number on any other line is never
/// a code. Gives the code and the line that has the code.
fn find_code<'a>(subject: &'a str, text: &'a str) -> Option<(String, &'a str)> {
    // Empty lines do not count: HTML paragraphs give empty lines between lines.
    let lines: Vec<&str> = std::iter::once(subject)
        .chain(text.lines())
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let cue = |l: &str| {
        let low = l.to_lowercase();
        CODE_WORDS.iter().any(|w| has_word(&low, w))
    };
    lines.iter().enumerate().find_map(|(i, &l)| {
        if !cue(l) {
            return None;
        }
        if let Some((code, at)) = digits(l) {
            let quote = excerpt(l, at, code.len());
            return Some((code, quote));
        }
        let next = *lines.get(i + 1)?;
        let (code, _) = digits(next)?;
        (next == code).then_some((code, next))
    })
}

/// True if `w` is in `hay` as a word: no letter or digit next to it, so
/// "barcode" does not hold the word "code".
fn has_word(hay: &str, w: &str) -> bool {
    hay.match_indices(w).any(|(i, _)| {
        let before = hay[..i].chars().next_back();
        let after = hay[i + w.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// The first run of 4 to 8 ASCII digits with no letter or digit next to it,
/// and its byte offset in `line`. A year from 1900 to 2099 is not a code
/// (shortcut: issue #94).
fn digits(line: &str) -> Option<(String, usize)> {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let mut back = line[..start].chars().rev();
            let mut ahead = line[i..].chars();
            // A digit group of a number, like 12.50 or 1,234, is not a code.
            let edge = |c: Option<char>, next: Option<char>| match c {
                None => true,
                Some('.' | ',') => !next.is_some_and(|n| n.is_ascii_digit()),
                Some(c) => !c.is_alphanumeric(),
            };
            let (b1, b2) = (back.next(), back.next());
            let (a1, a2) = (ahead.next(), ahead.next());
            let run = &line[start..i];
            let year = run.len() == 4 && matches!(&run[..2], "19" | "20");
            if (4..=8).contains(&run.len()) && !year && edge(b1, b2) && edge(a1, a2) {
                return Some((run.to_string(), start));
            }
        } else {
            i += 1;
        }
    }
    None
}

/// At most [`QUOTE_CHARS`] characters of `line` that hold the bytes
/// `at..at + len`: the whole line if it fits.
fn excerpt(line: &str, at: usize, len: usize) -> &str {
    if line.chars().count() <= QUOTE_CHARS {
        return line;
    }
    // Start up to 100 characters before the code, at a character boundary.
    let start = line[..at]
        .char_indices()
        .rev()
        .nth(99)
        .map_or(0, |(i, _)| i);
    let end = line[start..]
        .char_indices()
        .nth(QUOTE_CHARS)
        .map_or(line.len(), |(i, _)| start + i);
    debug_assert!(end >= at + len);
    &line[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(digits("Your code is 482913."), Some(("482913".into(), 13)));
        assert_eq!(digits("In 2026 code 7731"), Some(("7731".into(), 13)));
        assert_eq!(digits("Order 123456789"), None);
        assert_eq!(digits("Total 1234.50 or 1,2345"), None);
        assert_eq!(digits("ab1234"), None);
        assert_eq!(
            find_code("Sign in", "Your verification code:\n  7731\nThanks"),
            Some(("7731".into(), "7731"))
        );
        assert_eq!(find_code("Weekly news", "Read 2024 stories"), None);
        assert!(has_word("your code: 1234", "code"));
        assert!(!has_word("barcode 123456", "code"));
        assert!(!has_word("decode it", "code"));
    }
}
