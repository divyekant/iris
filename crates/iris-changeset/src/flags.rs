//! The draft flags of spec 2, section 5.3, from the content of a draft
//! (docs/specs/2026-11-17-send-checks.md). Fixed rules, no model.

use iris_compose::Content;
use rusqlite::{Connection, OptionalExtension};

use crate::Result;

/// The number of words in one `untrusted_content` query.
pub const RUN: usize = 8;

const CURRENCY_SIGNS: [char; 5] = ['$', '€', '£', '¥', '₹'];
const CURRENCY_WORDS: [&str; 16] = [
    "usd", "eur", "gbp", "inr", "jpy", "cad", "aud", "chf", "dollar", "dollars", "euro", "euros",
    "pound", "pounds", "rupee", "rupees",
];
const MONTHS: [&str; 24] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "jan",
    "feb",
    "mar",
    "apr",
    "jun",
    "jul",
    "aug",
    "sep",
    "sept",
    "oct",
    "nov",
    "dec",
];
const DAY_WORDS: [&str; 12] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "today",
    "tonight",
    "tomorrow",
    "eod",
    "eow",
];
const ATTACHMENT_WORDS: [&str; 21] = [
    "attach",
    "attaches",
    "attached",
    "attaching",
    "attachment",
    "attachments",
    "enclose",
    "enclosed",
    "encloses",
    "pdf",
    "doc",
    "docx",
    "xls",
    "xlsx",
    "ppt",
    "pptx",
    "csv",
    "zip",
    "screenshot",
    "screenshots",
    "enclosing",
];

/// The words of a text, as the full-text index splits them: runs of
/// letters and digits, in lower case.
pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn digit_start(w: &str) -> bool {
    w.starts_with(|c: char| c.is_ascii_digit())
}

/// An amount: a currency sign next to a digit (one space between is
/// allowed), or a number next to a currency code or word.
pub fn has_money(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let digit_at = |i: usize| chars.get(i).is_some_and(|c| c.is_ascii_digit());
    for (i, c) in chars.iter().enumerate() {
        if CURRENCY_SIGNS.contains(c) {
            let before = i.checked_sub(1).is_some_and(digit_at)
                || (i >= 2 && chars[i - 1] == ' ' && digit_at(i - 2));
            let after = digit_at(i + 1) || (chars.get(i + 1) == Some(&' ') && digit_at(i + 2));
            if before || after {
                return true;
            }
        }
    }
    let w = words(text);
    w.iter().enumerate().any(|(i, t)| {
        let joined = |s: &str| CURRENCY_WORDS.contains(&s);
        // "200usd" and "usd200".
        let digits = t.trim_start_matches(|c: char| c.is_ascii_digit());
        let letters = t.trim_end_matches(|c: char| c.is_ascii_digit());
        if (digits.len() < t.len() && joined(digits))
            || (letters.len() < t.len() && joined(letters))
        {
            return true;
        }
        joined(t)
            && (i.checked_sub(1).is_some_and(|p| digit_start(&w[p]))
                || w.get(i + 1).is_some_and(|n| digit_start(n)))
    })
}

/// A date: a month next to a number, a weekday or a near day, `next
/// week|month|year`, `end of (the) day|week|month`, an ISO date or a
/// numeric date.
pub fn has_date(text: &str) -> bool {
    let w = words(text);
    let near_number = |i: usize| {
        i.checked_sub(1).is_some_and(|p| digit_start(&w[p]))
            || w.get(i + 1).is_some_and(|n| digit_start(n))
    };
    for (i, t) in w.iter().enumerate() {
        let t = t.as_str();
        if DAY_WORDS.contains(&t) || (MONTHS.contains(&t) && near_number(i)) {
            return true;
        }
        let next = w.get(i + 1).map(String::as_str);
        if t == "next" && matches!(next, Some("week" | "month" | "year")) {
            return true;
        }
        if t == "end" && next == Some("of") {
            let n = if w.get(i + 2).map(String::as_str) == Some("the") {
                3
            } else {
                2
            };
            if matches!(
                w.get(i + n).map(String::as_str),
                Some("day" | "week" | "month")
            ) {
                return true;
            }
        }
    }
    numeric_date(text)
}

/// `2026-11-17` (4, 2 and 2 digits with `-`) or `17/11`, `11/17/2026`
/// (two or three groups of 1 to 4 digits with `/`).
fn numeric_date(text: &str) -> bool {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() || (i > 0 && b[i - 1].is_ascii_digit()) {
            i += 1;
            continue;
        }
        // The digit groups from i, with one kind of separator.
        let mut groups = Vec::new();
        let mut sep = None;
        let mut j = i;
        loop {
            let start = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            groups.push(j - start);
            match b.get(j) {
                Some(&s @ (b'-' | b'/'))
                    if sep.is_none_or(|x| x == s)
                        && b.get(j + 1).is_some_and(u8::is_ascii_digit) =>
                {
                    sep = Some(s);
                    j += 1;
                }
                _ => break,
            }
        }
        let date = match sep {
            Some(b'-') => groups.len() == 3 && groups[0] == 4 && groups[1] <= 2 && groups[2] <= 2,
            Some(b'/') => {
                (2..=3).contains(&groups.len()) && groups.iter().all(|g| (1..=4).contains(g))
            }
            _ => false,
        };
        if date {
            return true;
        }
        i = j.max(i + 1);
    }
    false
}

/// A word of the attachment group.
pub fn has_attachment(text: &str) -> bool {
    words(text)
        .iter()
        .any(|t| ATTACHMENT_WORDS.contains(&t.as_str()))
}

/// The flags `new_recipient`, `money`, `date`, `attachment` and
/// `untrusted_content` of a checked content, in this order. `account` is
/// the address of the account that writes the draft.
pub fn draft_flags(c: &Connection, account: &str, content: &Content) -> Result<Vec<&'static str>> {
    let mut flags = Vec::new();
    let mut new = false;
    for a in content.to.iter().chain(&content.cc) {
        if a.eq_ignore_ascii_case(account) {
            continue;
        }
        if !iris_senders::sent_to(c, &a.to_ascii_lowercase())? {
            new = true;
            break;
        }
    }
    let texts = [content.subject.as_str(), content.body.as_str()];
    for (flag, on) in [
        ("new_recipient", new),
        ("money", texts.iter().any(|t| has_money(t))),
        ("date", texts.iter().any(|t| has_date(t))),
        ("attachment", texts.iter().any(|t| has_attachment(t))),
        (
            "untrusted_content",
            copied(c, &content.subject)? || copied(c, &content.body)?,
        ),
    ] {
        if on {
            flags.push(flag);
        }
    }
    Ok(flags)
}

/// True if a run of [`RUN`] words of the text, at a start of 0, 8, 16 and
/// so on, is a phrase of the clean text of an external message: no `SENT`
/// and no `DRAFT` label (the mail of the owner, as in `iris-senders`). A copied run of 15 words or more always has such a start.
fn copied(c: &Connection, text: &str) -> Result<bool> {
    let w = words(text);
    let mut st = c.prepare_cached(
        "SELECT EXISTS (SELECT 1 FROM message_fts f
                        WHERE message_fts MATCH ?1
                          AND NOT EXISTS (SELECT 1 FROM message_label ml
                                          JOIN label l ON l.id = ml.label_id
                                          WHERE ml.message_id = f.rowid
                                            AND l.provider_id IN ('SENT', 'DRAFT')))",
    )?;
    for run in w.chunks_exact(RUN) {
        // The words are letters and digits only: no FTS5 syntax inside.
        let phrase = format!("text : \"{}\"", run.join(" "));
        if st.query_row([phrase], |r| r.get::<_, bool>(0))? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The draft content and the account address of a `draft` or `send`
/// action, or `None` if it has no checked content (submit denies it).
pub fn action_content(c: &Connection, action: i64) -> Result<Option<(String, Content)>> {
    let (verb, targets, args): (String, String, String) = c.query_row(
        "SELECT verb, targets, args FROM changeset_action WHERE id = ?1",
        [action],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    // Another writer can store a draft row with no content: no flags then.
    let found: Option<(String, Option<String>)> = match verb.as_str() {
        "send" => c
            .query_row(
                "SELECT a.address, json_extract(d.content, '$.content')
                 FROM action_draft ad JOIN draft d ON d.id = ad.draft_id
                 JOIN account a ON a.id = d.account_id WHERE ad.action_id = ?1",
                [action],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?,
        "draft" => {
            let address: Option<String> = c
                .query_row(
                    "SELECT a.address FROM account a WHERE 'a-' || a.id IN
                       (SELECT value FROM json_each(?2, '$.accounts'))
                     UNION ALL
                     SELECT a.address FROM action_message am JOIN message m ON m.id = am.message_id
                       JOIN account a ON a.id = m.account_id WHERE am.action_id = ?1
                     LIMIT 1",
                    rusqlite::params![action, targets],
                    |r| r.get(0),
                )
                .optional()?;
            address.map(|a| (a, Some(args)))
        }
        _ => None,
    };
    let Some((address, Some(json))) = found else {
        return Ok(None);
    };
    let Ok(mut content) = serde_json::from_str::<Content>(&json) else {
        return Ok(None);
    };
    if verb == "draft" {
        if let Some(p) = crate::parent(c, action)? {
            content.subject = iris_compose::reply_subject(&p.subject);
        }
    }
    Ok(content.check().ok().map(|c| (address, c)))
}
