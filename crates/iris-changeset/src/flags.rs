//! The draft flags of spec 2, section 5.3, from the content of a draft
//! (docs/specs/2026-11-17-send-checks.md). Fixed rules, no model.

use iris_compose::Content;
use std::ops::Range;

use rusqlite::{Connection, OptionalExtension};

use crate::Result;

/// The number of words in one `untrusted_content` query.
pub const RUN: usize = 8;

const CURRENCY_SIGNS: [char; 5] = ['$', '€', '£', '¥', '₹'];
const CURRENCY_WORDS: [&str; 16] = [
    "usd", "eur", "gbp", "inr", "jpy", "cad", "aud", "chf", "dollar", "dollars", "euro", "euros",
    "pound", "pounds", "rupee", "rupees",
];
/// The month names, then the short forms. "may" is its own short form, so
/// it is in the list once.
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

/// The byte ranges of the words of a text, as the full-text index splits
/// them: runs of letters and digits.
fn word_spans(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, ch) in text.char_indices() {
        match (ch.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push(s..i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(s..text.len());
    }
    out
}

/// The words of a text, as the full-text index splits them: runs of
/// letters and digits, in lower case.
pub fn words(text: &str) -> Vec<String> {
    word_spans(text)
        .into_iter()
        .map(|r| text[r].to_lowercase())
        .collect()
}

fn digit_start(w: &str) -> bool {
    w.starts_with(|c: char| c.is_ascii_digit())
}

/// The words of a text in lower case, each with its byte range.
fn spanned(text: &str) -> (Vec<String>, Vec<Range<usize>>) {
    let spans = word_spans(text);
    (
        spans
            .iter()
            .map(|r| text[r.clone()].to_lowercase())
            .collect(),
        spans,
    )
}

/// The range of word `i`, widened to the previous word if `before` and to
/// the next word if `after`.
fn widen(spans: &[Range<usize>], i: usize, before: bool, after: bool) -> Range<usize> {
    let start = if before {
        spans[i - 1].start
    } else {
        spans[i].start
    };
    let end = if after {
        spans[i + 1].end
    } else {
        spans[i].end
    };
    start..end
}

/// An amount: a currency sign next to a digit (one space between is
/// allowed), or a number next to a currency code or word.
pub fn has_money(text: &str) -> bool {
    !money_spans(text).is_empty()
}

/// The range grown over the number characters (digits, `,` and `.`) on
/// each side, so a mark covers the whole amount ("1,250.50"). A `,` or a
/// `.` at an end of the range is not part of the number.
fn grow(text: &str, r: Range<usize>) -> Range<usize> {
    let number = |c: char| c.is_ascii_digit() || c == ',' || c == '.';
    let mut start = r.start;
    while let Some(c) = text[..start].chars().next_back().filter(|&c| number(c)) {
        start -= c.len_utf8();
    }
    let mut end = r.end;
    while let Some(c) = text[end..].chars().next().filter(|&c| number(c)) {
        end += c.len_utf8();
    }
    let s = &text[start..end];
    let lead = s.len() - s.trim_start_matches([',', '.']).len();
    let trail = s.len() - s.trim_end_matches([',', '.']).len();
    (start + lead).min(r.start)..(end - trail).max(r.end)
}

/// The byte range of each amount of [`has_money`]: the sign or the
/// currency word with its whole number.
pub fn money_spans(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let digit_at = |i: usize| chars.get(i).is_some_and(|(_, c)| c.is_ascii_digit());
    let end_of = |i: usize| chars[i].0 + chars[i].1.len_utf8();
    for (i, &(_, c)) in chars.iter().enumerate() {
        if CURRENCY_SIGNS.contains(&c) {
            let near = i.checked_sub(1).is_some_and(digit_at);
            let before = near || (i >= 2 && chars[i - 1].1 == ' ' && digit_at(i - 2));
            let close = digit_at(i + 1);
            let after = close || (chars.get(i + 1).map(|x| x.1) == Some(' ') && digit_at(i + 2));
            if !(before || after) {
                continue;
            }
            let first = match (before, near) {
                (false, _) => i,
                (true, true) => i - 1,
                (true, false) => i - 2,
            };
            let last = match (after, close) {
                (false, _) => i,
                (true, true) => i + 1,
                (true, false) => i + 2,
            };
            out.push(grow(text, chars[first].0..end_of(last)));
        }
    }
    let (w, spans) = spanned(text);
    for (i, t) in w.iter().enumerate() {
        let joined = |s: &str| CURRENCY_WORDS.contains(&s);
        // "200usd" and "usd200".
        let digits = t.trim_start_matches(|c: char| c.is_ascii_digit());
        let letters = t.trim_end_matches(|c: char| c.is_ascii_digit());
        if (digits.len() < t.len() && joined(digits))
            || (letters.len() < t.len() && joined(letters))
        {
            out.push(grow(text, spans[i].clone()));
            continue;
        }
        if joined(t) {
            let before = i.checked_sub(1).is_some_and(|p| digit_start(&w[p]));
            let after = w.get(i + 1).is_some_and(|n| digit_start(n));
            if before || after {
                out.push(grow(text, widen(&spans, i, before, after)));
            }
        }
    }
    out
}

/// A date: a month next to a number, a weekday or a near day, `next
/// week|month|year`, `end of (the) day|week|month`, an ISO date or a
/// numeric date.
pub fn has_date(text: &str) -> bool {
    !date_spans(text).is_empty()
}

/// The byte range of each date of [`has_date`].
pub fn date_spans(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let (w, spans) = spanned(text);
    for (i, t) in w.iter().enumerate() {
        let t = t.as_str();
        let before = i.checked_sub(1).is_some_and(|p| digit_start(&w[p]));
        let after = w.get(i + 1).is_some_and(|n| digit_start(n));
        if DAY_WORDS.contains(&t) {
            out.push(spans[i].clone());
        } else if MONTHS.contains(&t) && (before || after) {
            out.push(widen(&spans, i, before, after));
        }
        let next = w.get(i + 1).map(String::as_str);
        if t == "next" && matches!(next, Some("week" | "month" | "year")) {
            out.push(spans[i].start..spans[i + 1].end);
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
                out.push(spans[i].start..spans[i + n].end);
            }
        }
    }
    out.extend(numeric_dates(text));
    out
}

/// `2026-11-17` (4, 2 and 2 digits with `-`) or `17/11`, `11/17/2026`
/// (two or three groups of 1 to 4 digits with `/`).
fn numeric_dates(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
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
            out.push(i..j);
        }
        i = j.max(i + 1);
    }
    out
}

/// A word of the attachment group.
pub fn has_attachment(text: &str) -> bool {
    !attachment_spans(text).is_empty()
}

/// The byte range of each word of [`has_attachment`].
pub fn attachment_spans(text: &str) -> Vec<Range<usize>> {
    let (w, spans) = spanned(text);
    w.iter()
        .zip(spans)
        .filter(|(t, _)| ATTACHMENT_WORDS.contains(&t.as_str()))
        .map(|(_, r)| r)
        .collect()
}

/// The draft flags in the order of spec 2, section 5.3.
pub const DRAFT_FLAGS: [&str; 5] = [
    "new_recipient",
    "money",
    "date",
    "attachment",
    "untrusted_content",
];

/// The part of a draft that a mark is in, in the order of the draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    /// The address with this index in `to`.
    To(usize),
    /// The address with this index in `cc`.
    Cc(usize),
    /// The address with this index in `bcc`.
    Bcc(usize),
    Subject,
    Body,
}

/// One part of a draft that gave a draft flag (card R4-07): the review
/// shows the flag on this part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub flag: &'static str,
    pub field: Field,
    /// The byte range in the text of the field: the whole address for
    /// `To` and `Cc`.
    pub range: Range<usize>,
    /// For `untrusted_content`: the store ID of an external message that
    /// holds the run.
    pub source: Option<i64>,
}

/// The flags `new_recipient`, `money`, `date`, `attachment` and
/// `untrusted_content` of a checked content, in this order. `account` is
/// the address of the account that writes the draft.
pub fn draft_flags(c: &Connection, account: &str, content: &Content) -> Result<Vec<&'static str>> {
    Ok(names(&scan(c, account, content, false)?))
}

/// Each part of a checked content that gives a draft flag. The flags of
/// the marks are the flags of [`draft_flags`]: the same rules give both.
pub fn draft_marks(c: &Connection, account: &str, content: &Content) -> Result<Vec<Mark>> {
    scan(c, account, content, true)
}

/// The flags of the marks, in the order of [`DRAFT_FLAGS`].
pub fn names(marks: &[Mark]) -> Vec<&'static str> {
    DRAFT_FLAGS
        .into_iter()
        .filter(|f| marks.iter().any(|m| m.flag == *f))
        .collect()
}

/// The marks of a content. With `all` false, the store checks stop at the
/// first new address and the first copied run: the flags are the same.
fn scan(c: &Connection, account: &str, content: &Content, all: bool) -> Result<Vec<Mark>> {
    let mut marks = Vec::new();
    let mark = |flag, field, range, source| Mark {
        flag,
        field,
        range,
        source,
    };
    let addresses = content
        .to
        .iter()
        .enumerate()
        .map(|(i, a)| (Field::To(i), a))
        .chain(
            content
                .cc
                .iter()
                .enumerate()
                .map(|(i, a)| (Field::Cc(i), a)),
        )
        .chain(
            content
                .bcc
                .iter()
                .enumerate()
                .map(|(i, a)| (Field::Bcc(i), a)),
        );
    for (field, a) in addresses {
        if a.eq_ignore_ascii_case(account) {
            continue;
        }
        if !iris_senders::sent_to(c, &a.to_ascii_lowercase())? {
            marks.push(mark("new_recipient", field, 0..a.len(), None));
            if !all {
                break;
            }
        }
    }
    let texts = [
        (Field::Subject, content.subject.as_str()),
        (Field::Body, content.body.as_str()),
    ];
    // "No file attached" only when the draft has no file (send content
    // spec, section 6).
    let no_file = content.attachments.is_empty();
    for (flag, find) in [
        ("money", money_spans as fn(&str) -> Vec<Range<usize>>),
        ("date", date_spans),
        ("attachment", attachment_spans),
    ] {
        if flag == "attachment" && !no_file {
            continue;
        }
        for (field, text) in texts {
            marks.extend(find(text).into_iter().map(|r| mark(flag, field, r, None)));
        }
    }
    for (field, text) in texts {
        if !all && marks.iter().any(|m| m.flag == "untrusted_content") {
            break;
        }
        for (range, source) in copied(c, text, all)? {
            marks.push(mark("untrusted_content", field, range, Some(source)));
        }
    }
    Ok(merge(marks))
}

/// One mark for each piece of text: the marks of one flag, one field and
/// one source that overlap or touch become one mark ("$1,200 USD" is one
/// amount). Marks of two sources stay apart, so each copied run keeps its
/// message; the copied runs never overlap. The marks come in the order of [`DRAFT_FLAGS`],
/// then of the field, then of the text.
fn merge(mut marks: Vec<Mark>) -> Vec<Mark> {
    let rank = |f: &str| DRAFT_FLAGS.iter().position(|x| *x == f);
    marks.sort_by_key(|m| (rank(m.flag), m.field, m.range.start, m.range.end));
    let mut out: Vec<Mark> = Vec::with_capacity(marks.len());
    for m in marks {
        match out.last_mut() {
            Some(last)
                if last.flag == m.flag
                    && last.field == m.field
                    && last.source == m.source
                    && m.range.start <= last.range.end =>
            {
                last.range.end = last.range.end.max(m.range.end);
            }
            _ => out.push(m),
        }
    }
    out
}

/// Each run of [`RUN`] words of the text, at a start of 0, 8, 16 and so
/// on, that is a phrase of the clean text of an external message: no
/// `SENT` and no `DRAFT` label (the mail of the owner, as in
/// `iris-senders`). A copied run of 15 words or more always has such a
/// start. Gives the byte range of the run and one such message. With
/// `all` false, it stops at the first run.
fn copied(c: &Connection, text: &str, all: bool) -> Result<Vec<(Range<usize>, i64)>> {
    let (w, spans) = spanned(text);
    let mut st = c.prepare_cached(
        "SELECT f.rowid FROM message_fts f
         WHERE message_fts MATCH ?1
           AND NOT EXISTS (SELECT 1 FROM message_label ml
                           JOIN label l ON l.id = ml.label_id
                           WHERE ml.message_id = f.rowid
                             AND l.provider_id IN ('SENT', 'DRAFT'))
         ORDER BY f.rowid LIMIT 1",
    )?;
    let mut out = Vec::new();
    for (k, run) in w.as_chunks::<RUN>().0.iter().enumerate() {
        // The words are letters and digits only: no FTS5 syntax inside.
        let phrase = format!("text : \"{}\"", run.join(" "));
        if let Some(id) = st.query_row([phrase], |r| r.get::<_, i64>(0)).optional()? {
            out.push((spans[k * RUN].start..spans[k * RUN + RUN - 1].end, id));
            if !all {
                break;
            }
        }
    }
    Ok(out)
}

/// The draft content and the account address of a `draft` or `send`
/// action, or `None` if it has no checked content. Submit denies a `send`
/// with no checked content (`check_write`), and gives no draft flag to a
/// `draft` with none (submit denies it too).
///
/// A `send` reads the stored raw bytes of its draft, the bytes that Gmail
/// gets, and only if they hash to the draft hash that the approval binds.
/// The JSON `content` of the row is not read: another writer can change it.
pub fn action_content(c: &Connection, action: i64) -> Result<Option<(String, Content)>> {
    let (verb, targets, args): (String, String, String) = c.query_row(
        "SELECT verb, targets, args FROM changeset_action WHERE id = ?1",
        [action],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    match verb.as_str() {
        "send" => {
            let row: Option<(String, Vec<u8>, String)> = c
                .query_row(
                    "SELECT a.address, d.raw, d.hash
                     FROM action_draft ad JOIN draft d ON d.id = ad.draft_id
                     JOIN account a ON a.id = d.account_id WHERE ad.action_id = ?1",
                    [action],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            Ok(row.and_then(|(address, raw, hash)| {
                let read = iris_compose::read(&raw, &address)?;
                (iris_compose::hash(&read.content, read.reply.as_ref()) == hash)
                    .then_some((address, read.content))
            }))
        }
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
            let Some(address) = address else {
                return Ok(None);
            };
            let Ok(mut content) = serde_json::from_str::<Content>(&args) else {
                return Ok(None);
            };
            // The subject from the mirror. The apply sets the subject from
            // Gmail and checks the flags of the final content again.
            if let Some(p) = crate::parent(c, action)? {
                content.subject = iris_compose::reply_subject(&p.subject);
            }
            Ok(content.check().ok().map(|c| (address, c)))
        }
        _ => Ok(None),
    }
}

/// The draft flags of `content` that the approved flags of `action` do
/// not have. The apply of a `draft` and the release of a `send` call it on
/// the content that Gmail gets, and deny the action if it is not empty.
pub fn new_flags(
    c: &Connection,
    action: i64,
    account: &str,
    content: &Content,
) -> Result<Vec<&'static str>> {
    let approved: String = c.query_row(
        "SELECT coalesce(flags, '[]') FROM changeset_action WHERE id = ?1",
        [action],
        |r| r.get(0),
    )?;
    let approved: Vec<String> = serde_json::from_str(&approved).unwrap_or_default();
    Ok(draft_flags(c, account, content)?
        .into_iter()
        .filter(|f| !approved.iter().any(|a| a == f))
        .collect())
}
