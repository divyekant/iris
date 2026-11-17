//! The drafts of the review (card R4-07): the text of each `draft` and
//! `send` action that waits for the person, cut into parts, with the draft
//! flags of the action on the words that gave them.

use std::ops::Range;

use iris_changeset::{action_content, draft_marks, Field, Mark};
use iris_store::{format_id, Kind, Store};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::Result;

/// `{"drafts": [...]}`: one entry for each `draft` and `send` action of
/// the changeset in `proposed` or `in_review`, in the order of the
/// actions. An action whose content Iris cannot read (a `send` whose
/// stored bytes do not hash to the draft hash) has `"readable": false`
/// and no text.
pub fn drafts(store: &Store, changeset: i64) -> Result<Value> {
    Ok(store.read(|c| {
        let mut st = c.prepare(
            "SELECT id, revision, n, flags FROM changeset_action
             WHERE changeset_id = ?1 AND verb IN ('draft', 'send')
               AND state IN ('proposed', 'in_review')
             ORDER BY revision, n",
        )?;
        let rows = st
            .query_map([changeset], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (action, revision, n, flags) in rows {
            let flags: Vec<String> =
                serde_json::from_str(flags.as_deref().unwrap_or("[]")).unwrap_or_default();
            let (draft, thread) = ids(c, action)?;
            let mut v = json!({"revision": revision, "n": n, "draft": draft, "thread": thread});
            match action_content(c, action).map_err(store_error)? {
                None => v["readable"] = false.into(),
                Some((account, content)) => {
                    // Only the flags that the action has: the person decides on these.
                    let marks: Vec<Mark> = draft_marks(c, &account, &content)
                        .map_err(store_error)?
                        .into_iter()
                        .filter(|m| flags.iter().any(|f| f == m.flag))
                        .collect();
                    let people = |list: &[String], field: fn(usize) -> Field| -> Vec<Value> {
                        list.iter()
                            .enumerate()
                            .map(|(i, a)| {
                                let on: Vec<&str> = marks
                                    .iter()
                                    .filter(|m| m.field == field(i))
                                    .map(|m| m.flag)
                                    .collect();
                                if on.is_empty() {
                                    json!({"address": a})
                                } else {
                                    json!({"address": a, "flags": on})
                                }
                            })
                            .collect()
                    };
                    v["readable"] = true.into();
                    v["to"] = people(&content.to, Field::To).into();
                    v["cc"] = people(&content.cc, Field::Cc).into();
                    v["subject"] = parts(&content.subject, &marks, Field::Subject).into();
                    v["body"] = parts(&content.body, &marks, Field::Body).into();
                }
            }
            out.push(v);
        }
        Ok(json!({ "drafts": out }))
    })?)
}

fn store_error(e: iris_changeset::Error) -> iris_store::Error {
    match e {
        iris_changeset::Error::Store(e) => e,
        other => iris_store::Error::Query(other.to_string()),
    }
}

/// The draft of a `send` (`d-N`) and the thread of the mail: the thread
/// of the draft row, or of the parent of a reply. `null` for a new mail.
fn ids(c: &Connection, action: i64) -> rusqlite::Result<(Option<String>, Option<String>)> {
    let (draft, thread): (Option<i64>, Option<i64>) = c
        .query_row(
            "SELECT d.id, coalesce(d.thread_id,
                      (SELECT m.thread_id FROM action_message am
                       JOIN message m ON m.id = am.message_id WHERE am.action_id = a.id))
             FROM changeset_action a
             LEFT JOIN action_draft ad ON ad.action_id = a.id
             LEFT JOIN draft d ON d.id = ad.draft_id
             WHERE a.id = ?1",
            [action],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .unwrap_or_default();
    Ok((
        draft.map(|d| format_id(Kind::Draft, d)),
        thread.map(|t| format_id(Kind::Thread, t)),
    ))
}

/// The text cut at each end of a mark of `field`: each part with the flags
/// of the marks over it, and for copied text the message it came from.
/// Two marks of one flag with no letter or digit between them are one
/// mark ("Friday, Dec 4"). The parts together are the text.
pub fn parts(text: &str, marks: &[Mark], field: Field) -> Vec<Value> {
    let mut ranges: Vec<(Range<usize>, &str, Option<i64>)> = marks
        .iter()
        .filter(|m| m.field == field)
        .map(|m| (m.range.clone(), m.flag, m.source))
        .collect();
    ranges.sort_by_key(|(r, f, _)| (*f, r.start, r.end));
    let mut merged: Vec<(Range<usize>, &str, Option<i64>)> = Vec::new();
    for (r, f, s) in ranges {
        match merged.last_mut() {
            Some((last, lf, ls))
                if *lf == f
                    && (r.start <= last.end
                        || !text[last.end..r.start].chars().any(char::is_alphanumeric)) =>
            {
                last.end = last.end.max(r.end);
                *ls = ls.or(s);
            }
            _ => merged.push((r, f, s)),
        }
    }
    let mut cuts: Vec<usize> = vec![0, text.len()];
    for (r, _, _) in &merged {
        cuts.extend([r.start, r.end]);
    }
    cuts.retain(|&i| i <= text.len() && text.is_char_boundary(i));
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let over: Vec<&(Range<usize>, &str, Option<i64>)> = merged
                .iter()
                .filter(|(r, _, _)| r.start <= a && b <= r.end)
                .collect();
            let mut v = json!({ "text": &text[a..b] });
            if !over.is_empty() {
                let mut flags: Vec<&str> = over.iter().map(|(_, f, _)| *f).collect();
                flags.dedup();
                v["flags"] = flags.into();
            }
            if let Some(s) = over.iter().find_map(|(_, _, s)| *s) {
                v["source"] = format_id(Kind::Message, s).into();
            }
            v
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Random marks on random text: the parts together are the text, each
    /// part is not empty, and each byte of a mark is in a part with its flag.
    #[test]
    fn parts_hold_the_text_and_each_mark() {
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        const BITS: [&str; 7] = ["a", " ", ",", "é", "日本", "$4", "\n"];
        const FLAGS: [&str; 3] = ["money", "date", "untrusted_content"];
        for _ in 0..2000 {
            let text: String = (0..next() % 30)
                .map(|_| BITS[(next() % BITS.len() as u64) as usize])
                .collect();
            let bounds: Vec<usize> = (0..=text.len())
                .filter(|&i| text.is_char_boundary(i))
                .collect();
            let marks: Vec<Mark> = (0..next() % 5)
                .filter_map(|_| {
                    let a = bounds[(next() % bounds.len() as u64) as usize];
                    let b = bounds[(next() % bounds.len() as u64) as usize];
                    (a < b).then(|| Mark {
                        flag: FLAGS[(next() % 3) as usize],
                        field: Field::Body,
                        range: a..b,
                        source: Some(1),
                    })
                })
                .collect();
            let parts = parts(&text, &marks, Field::Body);
            let mut at = 0;
            let mut got = String::new();
            for p in &parts {
                let t = p["text"].as_str().unwrap();
                assert!(!t.is_empty() || text.is_empty(), "{parts:?}");
                for m in &marks {
                    if m.range.start <= at && at + t.len() <= m.range.end && !t.is_empty() {
                        let on = p["flags"]
                            .as_array()
                            .is_some_and(|f| f.iter().any(|f| f == m.flag));
                        assert!(on, "{text:?} {marks:?} {parts:?}");
                    }
                }
                at += t.len();
                got.push_str(t);
            }
            assert_eq!(got, text);
        }
    }
}
