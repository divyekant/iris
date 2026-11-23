//! The drafts of the review (card R4-07): the text of each `draft` and
//! `send` action that waits for the person, cut into parts, with the draft
//! flags of the action on the words that gave them.

use std::io::Write;
use std::ops::Range;

use iris_changeset::{action_content, draft_marks, Field, Mark};
use iris_store::{format_id, Kind, Store};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::review::OPEN;
use crate::{Error, Result};

/// Writes `{"drafts": [...]}` to `out`: one entry for each `draft` and
/// `send` action of the changeset in `proposed` or `in_review`, in the
/// order of the actions. It reads and writes one action at a time, so the
/// memory holds one draft, whatever the count of actions.
///
/// An action whose content Iris cannot read (a `send` whose stored bytes
/// do not hash to the draft hash) has `"readable": false` and no text.
/// `unmarked` names each flag of the action with no mark in the text: a
/// flag of the thread or the author (`first_contact`), or a draft flag
/// whose evidence changed after the submit. A changeset that is not open
/// (a new revision is a draft) gives no entries.
pub fn write_drafts(store: &Store, changeset: i64, out: &mut impl Write) -> Result<()> {
    out.write_all(b"{\"drafts\":[").map_err(io)?;
    let mut after = (0, 0);
    while let Some((at, v)) = store.read(|c| next_draft(c, changeset, after))? {
        if after != (0, 0) {
            out.write_all(b",").map_err(io)?;
        }
        serde_json::to_writer(&mut *out, &v).map_err(|e| io(e.into()))?;
        after = at;
    }
    out.write_all(b"]}").map_err(io)
}

fn io(e: std::io::Error) -> Error {
    Error::Store(e.into())
}

/// The first waiting `draft` or `send` action after `(revision, n)`, as
/// its entry of [`write_drafts`].
fn next_draft(
    c: &Connection,
    changeset: i64,
    (revision, n): (i64, i64),
) -> iris_store::Result<Option<((i64, i64), Value)>> {
    let row = c
        .query_row(
            // Only an open changeset: the review list and the decisions
            // read the same condition.
            &format!(
                "SELECT a.id, a.revision, a.n, a.flags FROM changeset_action a
                 JOIN changeset c ON c.id = a.changeset_id
                 WHERE a.changeset_id = ?1 AND {OPEN}
                   AND a.verb IN ('draft', 'send')
                   AND a.state IN ('proposed', 'in_review')
                   AND (a.revision, a.n) > (?2, ?3)
                 ORDER BY a.revision, a.n LIMIT 1"
            ),
            [changeset, revision, n],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((action, revision, n, flags)) = row else {
        return Ok(None);
    };
    let flags: Vec<String> =
        serde_json::from_str(flags.as_deref().unwrap_or("[]")).unwrap_or_default();
    let (draft, thread) = ids(c, action)?;
    let mut v = json!({"revision": revision, "n": n, "draft": draft, "thread": thread});
    // Only the flags that the action has: the person decides on these.
    let marks: Vec<Mark> = match action_content(c, action).map_err(store_error)? {
        None => {
            v["readable"] = false.into();
            Vec::new()
        }
        Some((account, content)) => {
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
            marks
        }
    };
    let unmarked: Vec<&str> = flags
        .iter()
        .map(String::as_str)
        .filter(|f| !marks.iter().any(|m| m.flag == *f))
        .collect();
    v["unmarked"] = unmarked.into();
    Ok(Some(((revision, n), v)))
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
/// Two marks of one flag with no source and no letter or digit between
/// them are one mark ("Friday, Dec 4"). Two copied runs of one message are
/// one mark only when they overlap or touch: the scanner did not check the
/// text between them against the message (#415). The parts together are
/// the text.
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
                    && *ls == s
                    && (r.start <= last.end
                        || s.is_none()
                            && !text[last.end..r.start].chars().any(char::is_alphanumeric)) =>
            {
                last.end = last.end.max(r.end);
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
    // One pass over the cuts: `open` holds the marks that start at or
    // before the cut and have not ended.
    merged.sort_by_key(|(r, _, _)| r.start);
    let mut next = merged.iter().peekable();
    let mut open: Vec<&(Range<usize>, &str, Option<i64>)> = Vec::new();
    cuts.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            open.retain(|(r, _, _)| b <= r.end);
            while let Some(m) = next.next_if(|(r, _, _)| r.start <= a) {
                if b <= m.0.end {
                    open.push(m);
                }
            }
            let mut v = json!({ "text": &text[a..b] });
            if !open.is_empty() {
                let mut flags: Vec<&str> = open.iter().map(|(_, f, _)| *f).collect();
                flags.sort_unstable();
                flags.dedup();
                v["flags"] = flags.into();
            }
            if let Some(s) = open.iter().find_map(|(_, _, s)| *s) {
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
                    let flag = FLAGS[(next() % 3) as usize];
                    let source = (flag == "untrusted_content").then(|| 1 + (next() % 2) as i64);
                    (a < b).then_some(Mark {
                        flag,
                        field: Field::Body,
                        range: a..b,
                        source,
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
                // A source names a copied run of that message on each byte
                // of the part: the scanner checked each of these bytes
                // against the message (#415).
                if let Some(src) = p["source"].as_str() {
                    for i in at..at + t.len() {
                        let over = marks.iter().any(|m| {
                            m.range.contains(&i)
                                && m.source.map(|s| format_id(Kind::Message, s)).as_deref()
                                    == Some(src)
                        });
                        assert!(over, "byte {i}: {text:?} {marks:?} {parts:?}");
                    }
                }
                at += t.len();
                got.push_str(t);
            }
            assert_eq!(got, text);
        }
    }

    /// Two copied runs that meet, from two messages: each keeps its message.
    #[test]
    fn copied_runs_of_two_messages_keep_their_sources() {
        let text = "one two three. four five six";
        let run = |range: Range<usize>, source| Mark {
            flag: "untrusted_content",
            field: Field::Body,
            range,
            source: Some(source),
        };
        let got: Vec<(String, Value)> = parts(text, &[run(0..13, 1), run(15..28, 2)], Field::Body)
            .into_iter()
            .map(|p| (p["text"].as_str().unwrap().to_string(), p["source"].clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("one two three".into(), json!("m-1")),
                (". ".into(), Value::Null),
                ("four five six".into(), json!("m-2")),
            ]
        );
    }

    /// Two copied runs of one message with changed punctuation between
    /// them: the punctuation has no source (#415).
    #[test]
    fn the_gap_between_two_copied_runs_of_one_message_has_no_source() {
        let text = "one two three; four five six";
        let run = |range: Range<usize>| Mark {
            flag: "untrusted_content",
            field: Field::Body,
            range,
            source: Some(1),
        };
        let got: Vec<(String, Value)> = parts(text, &[run(0..13), run(15..28)], Field::Body)
            .into_iter()
            .map(|p| (p["text"].as_str().unwrap().to_string(), p["source"].clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("one two three".into(), json!("m-1")),
                ("; ".into(), Value::Null),
                ("four five six".into(), json!("m-1")),
            ]
        );
    }

    /// A full body of 256 KiB with a mark on each fifth word: the cut is
    /// one pass, not one scan of the marks for each part.
    #[test]
    fn many_marks_cut_in_one_pass() {
        let text = "today report ".repeat(256 * 1024 / 13);
        let marks: Vec<Mark> = text
            .match_indices("today")
            .map(|(i, w)| Mark {
                flag: "date",
                field: Field::Body,
                range: i..i + w.len(),
                source: None,
            })
            .collect();
        assert!(marks.len() > 20_000);
        let start = std::time::Instant::now();
        let got = parts(&text, &marks, Field::Body);
        let took = start.elapsed();
        assert_eq!(got.len(), 2 * marks.len());
        assert!(got.iter().step_by(2).all(|p| p["flags"] == json!(["date"])));
        // A scan for each part takes about 800 million steps: many seconds.
        assert!(took.as_secs_f64() < 2.0, "{took:?}");
    }
}
