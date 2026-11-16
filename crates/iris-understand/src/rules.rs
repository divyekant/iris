//! The fixed rules of specification 1, section 7.3. They run before the
//! model: fast, free and the same each time. Mail text is data: the rules
//! only compare text.

use serde_json::{json, Value};

use crate::{cut, feed_status, party_sender, time, Analysis, Context, Draft, Kind};

/// The default life of a code signal (specification 1, section 6.6).
const CODE_LIFE_MS: i64 = 15 * 60 * 1000;
/// The most characters of a quote (the schema of a source).
const QUOTE_CHARS: usize = 500;
/// The longest gap, in bytes, between a code and its code word. It keeps the
/// search of a long line linear.
const CODE_GAP: usize = 100;
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
/// Whole words of a request to pay or to act: the model reads the message.
const ACTION_WORDS: [&str; 9] = [
    "pay",
    "payment",
    "due",
    "overdue",
    "bill",
    "balance",
    "renew",
    "renewal",
    "action required",
];
/// Whole words of a sign-in. A code counts only in a message with one.
const LOGIN_WORDS: [&str; 12] = [
    "sign in",
    "sign-in",
    "log in",
    "login",
    "log-in",
    "verify",
    "verification",
    "two-factor",
    "2fa",
    "authentication",
    "one-time",
    "passcode",
];
/// Whole words of a promotion: its code is not a sign-in code.
// shortcut: the word lists are short and in English, issue #95.
const PROMO_WORDS: [&str; 8] = [
    "discount",
    "coupon",
    "promo",
    "promotion",
    "voucher",
    "gift",
    "referral",
    "sale",
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
    // A receipt or a statement can hold a code, a calendar request or a
    // List-Id too: the model reads it, so that the record and the payment
    // are not lost.
    if looks_like_record(a) {
        return None;
    }
    let subject = a.text.subject.as_str();
    let quote = or_none(subject);
    // You owe an answer only to a request that names you as a recipient.
    let to_me = a
        .head
        .to
        .iter()
        .chain(&a.head.cc)
        .any(|r| r.eq_ignore_ascii_case(cx.me));
    if a.text.invitation && cx.kind != Kind::SelfOwner && to_me {
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
        let found = if is_login(a) {
            find_code(subject, &a.text.text)
        } else {
            None
        };
        // A code word with no clear code: the model reads it, also when the
        // list rule could match.
        if found == Some(None) {
            return None;
        }
        if let Some(Some((code, line))) = found {
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
        // A file can hold a record that no word of the text names.
        let list = a.head.list_id.is_some() || a.head.list_unsubscribe;
        if list && a.text.attachments.is_empty() {
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
    // The whole clean text and the attachment text: both are bounded, and a
    // receipt can come late or only in an attached PDF.
    let mut hay = format!("{}\n{}", a.text.subject, a.text.text);
    for (_, t) in &a.text.attachment_text {
        hay.push('\n');
        hay.push_str(t);
    }
    let low = hay.to_lowercase();
    RECORD_WORDS.iter().any(|w| low.contains(w)) || !words(&hay, &ACTION_WORDS).is_empty()
}

/// The message is about a sign-in, and not a promotion: a code in it is a
/// sign-in code (specification 1, section 6.6).
fn is_login(a: &Analysis) -> bool {
    let hay = format!("{}\n{}", a.text.subject, a.text.text);
    !words(&hay, &LOGIN_WORDS).is_empty() && words(&hay, &PROMO_WORDS).is_empty()
}

/// The one code of the message. A code is a run of 4 to 8 digits:
/// - strong: the code word, then only `:`, `-`, `=`, `#` or `is`, then the
///   code;
/// - weak: at most 3 other words, and at most [`CODE_GAP`] bytes, between the
///   code and the code word, before or after it. A weak code that looks like a year (1900 to 2099) does not
///   count (shortcut: issue #94).
///
/// A line gives one code only if one number binds, and that number is
/// strong or is the only number of the line. Another line with a bound
/// number is not clear. The message gives a code only if each line that
/// gives a code gives the same value, and no line is unclear. Else the
/// result is `Some(None)`, and the model reads the message.
///
/// If a line has a code word and no number, the next line continues it: a
/// number there binds to the code word with the words of both lines in the
/// gap. Across the line break only a strong code counts, like a line of only
/// the code. A weak one is not clear. A next line with its own code word is
/// read as its own line, not as a continuation.
///
/// Gives the code and the line that has the code (at most 500 characters),
/// `Some(None)` if the code is not clear, or `None` if there is no code.
#[allow(clippy::option_option)]
fn find_code<'a>(subject: &'a str, text: &'a str) -> Option<Option<(String, &'a str)>> {
    // Empty lines do not count: HTML paragraphs give empty lines between lines.
    let lines: Vec<&str> = std::iter::once(subject)
        .chain(text.lines())
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    // Each cue line gives no code, an unclear code or one code. The message
    // gives a code only if all cue lines agree on one value.
    let mut found: Option<(String, &str)> = None;
    for (i, &l) in lines.iter().enumerate() {
        let cues = cues(l);
        if cues.is_empty() {
            continue;
        }
        let nums = numbers(l);
        let pick = if !nums.is_empty() {
            the_code(l, &cues, &nums, None)
                .map(|o| o.map(|(s, e)| (l[s..e].to_string(), excerpt(l, s, e - s))))
        } else if let Some(&next) = lines.get(i + 1).filter(|n| self::cues(n).is_empty()) {
            // A next line with its own code word is a cue line of its own.
            // The cue line and the next line as one text, with the cues at
            // their own offsets and the numbers after the line break.
            let both = format!("{l}\n{next}");
            let shift = l.len() + 1;
            the_code(&both, &cues, &numbers(next), Some(shift)).map(|o| {
                o.map(|(s, e)| {
                    let (s, e) = (s - shift, e - shift);
                    (next[s..e].to_string(), excerpt(next, s, e - s))
                })
            })
        } else {
            None
        };
        match (pick, &found) {
            (None, _) => {}
            (Some(Some(c)), None) => found = Some(c),
            (Some(Some(c)), Some(f)) if c.0 == f.0 => {}
            _ => return Some(None),
        }
    }
    found.map(Some)
}

/// The code among `nums` (moved by `shift` in `text`) for the `cues` of
/// `text`, as a range of `text`:
/// - `None`: no number binds;
/// - `Some(None)`: numbers bind, but not one clear code;
/// - `Some(Some(range))`: one clear code.
///
/// With `next`, the numbers are on the line after the cues, at `next` in
/// `text`. Across a line break only a strong number is clear: a weak one
/// gives `Some(None)`.
#[allow(clippy::option_option)]
fn the_code(
    text: &str,
    cues: &[(usize, usize)],
    nums: &[(usize, usize)],
    next: Option<usize>,
) -> Option<Option<(usize, usize)>> {
    let shift = next.unwrap_or(0);
    // Only the cues near a number can bind to it: `cues` is in order.
    let level = |(s, e): (usize, usize)| {
        let year = e - s == 4 && matches!(&text[s..s + 2], "19" | "20");
        let from = cues.partition_point(|c| c.1 + CODE_GAP < s);
        let near = cues[from..].iter().take_while(|c| c.0 <= e + CODE_GAP);
        let mut best = Bind::None;
        for &cue in near {
            match bind(text, (s, e), cue) {
                Bind::Strong => return Bind::Strong,
                Bind::Weak if !year => best = Bind::Weak,
                _ => {}
            }
        }
        best
    };
    let all: Vec<_> = nums.iter().map(|&(s, e)| (s + shift, e + shift)).collect();
    let bound: Vec<_> = all
        .iter()
        .map(|&n| (n, level(n)))
        .filter(|&(_, b)| b != Bind::None)
        .collect();
    match bound.as_slice() {
        [] => None,
        [(n, Bind::Strong)] => Some(Some(*n)),
        [(n, _)] if all.len() == 1 && next.is_none() => Some(Some(*n)),
        _ => Some(None),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bind {
    Strong,
    Weak,
    None,
}

/// How a number at `num` is bound to a code word at `cue` in `line`.
fn bind(line: &str, num: (usize, usize), cue: (usize, usize)) -> Bind {
    let (gap, after) = if num.0 >= cue.1 {
        (&line[cue.1..num.0], true)
    } else if cue.0 >= num.1 {
        (&line[num.1..cue.0], false)
    } else {
        return Bind::None;
    };
    if gap.len() > CODE_GAP {
        return Bind::None;
    }
    let words: Vec<&str> = gap
        .split(|c: char| c.is_whitespace() || matches!(c, ':' | '-' | '=' | '#'))
        .filter(|w| !w.is_empty())
        .collect();
    match words.as_slice() {
        [] if after => Bind::Strong,
        [w] if after && w.eq_ignore_ascii_case("is") => Bind::Strong,
        w if w.len() <= 3 => Bind::Weak,
        _ => Bind::None,
    }
}

/// The byte ranges of the code words in `line`, as whole words: "barcode"
/// does not hold the word "code".
fn cues(line: &str) -> Vec<(usize, usize)> {
    words(line, &CODE_WORDS)
}

/// The byte ranges of `list` in `text` as whole words, in order. The words
/// of `list` are lowercase ASCII.
fn words(text: &str, list: &[&str]) -> Vec<(usize, usize)> {
    // ASCII lowercase keeps the byte offsets of `text`.
    let low = text.to_ascii_lowercase();
    let mut out = Vec::new();
    for w in list {
        for (i, _) in low.match_indices(w) {
            let before = low[..i].chars().next_back();
            let after = low[i + w.len()..].chars().next();
            if !before.is_some_and(char::is_alphanumeric)
                && !after.is_some_and(char::is_alphanumeric)
            {
                out.push((i, i + w.len()));
            }
        }
    }
    out.sort_unstable();
    out
}

/// The byte ranges of the runs of 4 to 8 ASCII digits in `line` with no
/// letter or digit next to them, in order. A digit group of a number, like
/// 12.50 or 1,234, is not such a run.
fn numbers(line: &str) -> Vec<(usize, usize)> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let mut back = line[..start].chars().rev();
        let mut ahead = line[i..].chars();
        let edge = |c: Option<char>, next: Option<char>| match c {
            None => true,
            Some('.' | ',') => !next.is_some_and(|n| n.is_ascii_digit()),
            Some(c) => !c.is_alphanumeric(),
        };
        let (b1, b2) = (back.next(), back.next());
        let (a1, a2) = (ahead.next(), ahead.next());
        if (4..=8).contains(&(i - start)) && edge(b1, b2) && edge(a1, a2) {
            out.push((start, i));
        }
    }
    out
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
        let code = |s: &str, t: &str| find_code(s, t).flatten().map(|(c, _)| c);
        let unclear = |s: &str| find_code(s, "") == Some(None);
        assert!(unclear("Your code is 1234 or 5678"));
        assert!(unclear("Your code is 482913, old code is 123456"));
        assert!(unclear("Account 123456: your code is 482913"));
        assert_eq!(
            find_code("Verification for account 123456", "Your code: 482913"),
            Some(None)
        );
        assert_eq!(
            code("Your code is 482913", "Your code: 482913"),
            Some("482913".into())
        );
        assert_eq!(code("Your code is 482913.", ""), Some("482913".into()));
        assert_eq!(
            code("Your verification code is 2026", ""),
            Some("2026".into())
        );
        assert_eq!(code("Verification in 2026", ""), None);
        assert_eq!(code("482913 is your code", ""), Some("482913".into()));
        assert_eq!(code("Order 123456789 code", ""), None);
        assert_eq!(code("Code: total 1234.50 or 1,2345", ""), None);
        assert_eq!(code("code ab1234", ""), None);
        assert_eq!(
            code("Sign in", "Your verification code:\n  7731\nThanks"),
            Some("7731".into())
        );
        assert_eq!(code("Weekly news", "Read 2024 stories"), None);
        // An account number next to the code makes the line not clear.
        assert_eq!(
            code("Your verification code for account 123456 is 482913", ""),
            None
        );
        assert_eq!(code("Your code is 482913, old code is 123456", ""), None);
        assert_eq!(
            code("Your verification code", "Code: 123456"),
            Some("123456".into())
        );
        // Across a line break, only a strong code is clear.
        assert_eq!(
            find_code("Your verification code", "Use 482913 to sign in"),
            Some(None)
        );
        assert_eq!(
            find_code(
                "Sign in",
                "Your verification code:\nExpires in 3600 seconds\nUse 482913 to sign in"
            ),
            Some(None)
        );
        assert_eq!(code("Your code", "Expires in 2026"), None);
        assert_eq!(code("Your code is", "2026"), Some("2026".into()));
        assert_eq!(
            code("Your code", "7731\nVerification help 2026"),
            Some("7731".into())
        );
        assert_eq!(code("barcode 123456", ""), None);
        assert_eq!(code("decode 123456", ""), None);
        assert_eq!(
            code("Your code", "for the shop of the town hall 7731"),
            None
        );
    }
}
