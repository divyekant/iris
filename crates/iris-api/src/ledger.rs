//! The ledger of the web client (card R2-07, specification 2, section 7):
//! each change that Iris made, newest first, with who decided it and why,
//! and the undo of an entry (section 7.4).

use iris_store::Store;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use crate::{Error, Result};

/// One ledger entry, with the action that it applies.
#[derive(Debug, Serialize, PartialEq)]
pub struct LedgerEntry {
    /// `l-` and the row ID.
    pub id: String,
    pub verb: String,
    /// Only `label`, `until`, `date` and `field` ([`ARGS`]).
    pub args: Value,
    /// `writing`, `applied`, `failed` or `undone`.
    pub state: String,
    pub at: String,
    /// Why the entry failed, or why the engine could not confirm each message.
    pub error: Option<String>,
    /// The messages of the entry, and the messages that Gmail confirmed.
    pub messages: i64,
    pub changed: i64,
    /// The items of a state verb, and the title of the first item.
    pub items: i64,
    pub title: Option<String>,
    pub changeset: String,
    pub author: Author,
    /// Who approved it: `rule` (with `rule`) or `person`.
    pub decided_by: Option<String>,
    pub rule: Option<i64>,
    pub policy: Option<i64>,
    /// The reason of the decision, and the reason of the author: of the
    /// action, else of the revision of the action.
    pub reason: Option<String>,
    pub why: Option<String>,
    /// True when an undo can start now (view `ledger_can_undo`).
    pub can_undo: bool,
    /// True while an undo of the entry waits to apply or applies.
    pub undo_open: bool,
    /// The entry of the undo, after the undo.
    pub undone_by: Option<String>,
    /// The entry that this entry undoes.
    pub undo_of: Option<String>,
    /// For an applied undo (step 1): the messages or items that it did not
    /// change, because they changed again after the entry or are gone
    /// (decision D18). 0 for each other entry.
    pub skipped: i64,
}

/// The author of the changeset of an entry. `goal` is the goal of the job of
/// an agent.
#[derive(Debug, Serialize, PartialEq)]
pub struct Author {
    pub kind: String,
    pub name: Option<String>,
    pub job: Option<String>,
    pub goal: Option<String>,
}

/// One page of [`ledger`].
#[derive(Debug, Serialize, PartialEq)]
pub struct LedgerPage {
    pub entries: Vec<LedgerEntry>,
    /// The `after` value of the next page, or `None` on the last page.
    pub next: Option<String>,
}

/// The arguments that the list gives: the short ones that the words of a
/// verb need. Each other argument (for example the body of a draft) stays
/// out. `json_patch` drops the keys with no text value.
const ARGS: &str = "json_patch('{}', json_object(
          'label', CASE WHEN json_type(a.args, '$.label') = 'text' THEN substr(json_extract(a.args, '$.label'), 1, 2000) END,
          'until', CASE WHEN json_type(a.args, '$.until') = 'text' THEN substr(json_extract(a.args, '$.until'), 1, 2000) END,
          'date', CASE WHEN json_type(a.args, '$.date') = 'text' THEN substr(json_extract(a.args, '$.date'), 1, 2000) END,
          'field', CASE WHEN json_type(a.args, '$.field') = 'text' THEN substr(json_extract(a.args, '$.field'), 1, 2000) END))";

/// The entry query. `{args}` is [`ARGS`]. Each text has at most 2,000
/// characters (`substr(x, 1, 2000)`), so a page of 200 entries stays small
/// also when an action has a large text (the body of a draft has up to
/// 256 KiB).
const ENTRY: &str = "SELECT l.id, a.verb, {args}, l.state, l.at, substr(l.error, 1, 2000),
        (SELECT count(*) FROM ledger_message lm WHERE lm.ledger_id = l.id),
        (SELECT count(*) FROM ledger_message lm WHERE lm.ledger_id = l.id AND lm.done = 1),
        (SELECT count(*) FROM action_item ai WHERE ai.action_id = a.id),
        (SELECT CASE WHEN json_type(i.doc, '$.title') = 'text' THEN substr(json_extract(i.doc, '$.title'), 1, 2000) END
         FROM action_item ai JOIN item i ON i.id = ai.item_id
         WHERE ai.action_id = a.id AND NOT {hostile} ORDER BY ai.item_id LIMIT 1),
        a.changeset_id, c.author_kind, substr(c.author_name, 1, 2000), substr(c.author_job, 1, 2000),
        substr(j.goal, 1, 2000), a.decided_by, a.rule, a.policy, substr(a.reason, 1, 2000),
        substr(coalesce(a.why, (SELECT r.why FROM changeset_revision r
                                WHERE r.changeset_id = a.changeset_id AND r.revision = a.revision)), 1, 2000),
        EXISTS (SELECT 1 FROM ledger_can_undo v WHERE v.ledger_id = l.id),
        EXISTS (SELECT 1 FROM ledger_undo u JOIN changeset_action o ON o.id = u.action_id
                WHERE u.ledger_id = l.id
                  AND coalesce(o.state, 'proposed') IN ('proposed', 'in_review', 'approved')),
        l.undone_by,
        (SELECT u.ledger_id FROM ledger_undo u WHERE u.action_id = a.id),
        CASE WHEN l.state = 'applied'
                  AND EXISTS (SELECT 1 FROM ledger_undo u WHERE u.action_id = a.id AND u.step = 1)
             THEN max(0, (SELECT count(*) FROM action_message am WHERE am.action_id = a.id
                            AND NOT EXISTS (SELECT 1 FROM ledger_message lm
                                            WHERE lm.ledger_id = l.id
                                              AND lm.message_id = am.message_id AND lm.done = 1))
                       + (SELECT count(*) FROM action_item ai WHERE ai.action_id = a.id)
                       - (SELECT count(*) FROM json_each(coalesce(l.post, '{}'))))
             ELSE 0 END
     FROM ledger l
     JOIN changeset_action a ON a.id = l.action_id
     JOIN changeset c ON c.id = a.changeset_id
     LEFT JOIN job j ON c.author_job = 'j-' || j.id";

fn entry_sql(tail: &str) -> String {
    // The title of an item of hostile mail is not shown (issue 112).
    let entry = ENTRY
        .replace("{args}", ARGS)
        .replace("{hostile}", iris_store::from_hostile_mail!("i"));
    format!("{entry} {tail}")
}

fn entry(r: &rusqlite::Row) -> rusqlite::Result<LedgerEntry> {
    let l = |n: Option<i64>| n.map(|n| format!("l-{n}"));
    Ok(LedgerEntry {
        id: format!("l-{}", r.get::<_, i64>(0)?),
        verb: r.get(1)?,
        // `json_patch` gives valid JSON.
        args: serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or(Value::Null),
        state: r.get(3)?,
        at: crate::rfc3339(r.get(4)?),
        error: r.get(5)?,
        messages: r.get(6)?,
        changed: r.get(7)?,
        items: r.get(8)?,
        title: r.get(9)?,
        changeset: iris_changeset::format_changeset_id(r.get(10)?),
        author: Author {
            kind: r.get(11)?,
            name: r.get(12)?,
            job: r.get(13)?,
            goal: r.get(14)?,
        },
        decided_by: r.get(15)?,
        rule: r.get(16)?,
        policy: r.get(17)?,
        reason: r.get(18)?,
        why: r.get(19)?,
        can_undo: r.get(20)?,
        undo_open: r.get(21)?,
        undone_by: l(r.get(22)?),
        undo_of: l(r.get(23)?),
        skipped: r.get(24)?,
    })
}

/// The ledger entries, newest first. `after` is the `next` value of the
/// page before. Keyset on the row ID: a row ID never changes, so a page
/// does not repeat or skip an entry.
pub fn ledger(store: &Store, limit: u32, after: Option<&str>) -> Result<LedgerPage> {
    if !(1..=crate::MAX_LIMIT).contains(&limit) {
        return Err(Error::Query(format!(
            "limit must be 1 to {}",
            crate::MAX_LIMIT
        )));
    }
    let before = match after {
        None => i64::MAX,
        Some(t) => crate::review::parse_after(t)
            .ok_or_else(|| Error::Query(format!("after {t} is not the next value of a page")))?,
    };
    let mut entries: Vec<LedgerEntry> = store.read(|c| {
        let mut st = c.prepare_cached(&entry_sql("WHERE l.id < ?1 ORDER BY l.id DESC LIMIT ?2"))?;
        let rows = st.query_map(params![before, limit + 1], entry)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })?;
    let more = entries.len() > limit as usize;
    entries.truncate(limit as usize);
    let next = more
        .then(|| entries.last().map(|e| e.id["l-".len()..].to_string()))
        .flatten();
    Ok(LedgerPage { entries, next })
}

fn one(c: &Connection, id: i64) -> rusqlite::Result<Option<LedgerEntry>> {
    c.query_row(&entry_sql("WHERE l.id = ?1"), [id], entry)
        .optional()
}

/// The row ID of `l-<n>`, only in its canonical form.
fn parse_id(id: &str) -> Option<i64> {
    id.strip_prefix("l-").and_then(crate::review::parse_after)
}

/// Start the undo of the entry `id` (`l-<n>`), approved by the person
/// (section 7.4). The engine applies it at its next step. Gives the entry
/// after the start, or `None` when no entry has the ID. An entry with no
/// undo now (for example, an open undo, or no changed message) is
/// [`Error::Conflict`] with the reason, and nothing is written.
pub fn undo_entry(store: &Store, id: &str, now_ms: i64) -> Result<Option<LedgerEntry>> {
    let Some(n) = parse_id(id) else {
        return Ok(None);
    };
    if store.read(|c| Ok(one(c, n)?))?.is_none() {
        return Ok(None);
    }
    match iris_apply::undo(store, n, now_ms) {
        Ok(_) => {}
        Err(iris_apply::Error::Store(e)) => return Err(Error::Store(e)),
        Err(iris_apply::Error::Undo(why)) => return Err(Error::Conflict(why)),
        Err(e) => return Err(Error::Store(iris_store::Error::Query(e.to_string()))),
    }
    Ok(store.read(|c| Ok(one(c, n)?))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generated input: `parse_id` takes back each `l-<n>` of a row ID, and
    /// refuses each other text (no prefix, signs, zeros in front, spaces,
    /// other characters, 0, a value out of range).
    #[test]
    fn the_id_parser_accepts_only_canonical_ids() {
        let mut x: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let canonical = |t: &str| {
            t.strip_prefix("l-").is_some_and(|n| {
                !n.is_empty()
                    && n.bytes().all(|b| b.is_ascii_digit())
                    && !n.starts_with('0')
                    && n.parse::<i64>().is_ok()
            })
        };
        const PARTS: [&str; 17] = [
            "l-",
            "l",
            "-",
            "L-",
            "0",
            "1",
            "7",
            "42",
            "+",
            " ",
            "x",
            "\u{e9}",
            "\0",
            "9223372036854775807",
            "9223372036854775808",
            "00",
            ".",
        ];
        for _ in 0..20_000 {
            let id = (next() >> 1) as i64;
            if id > 0 {
                assert_eq!(parse_id(&format!("l-{id}")), Some(id));
                assert_eq!(parse_id(&id.to_string()), None);
            }
            let t: String = (0..1 + next() % 5)
                .map(|_| PARTS[(next() % PARTS.len() as u64) as usize])
                .collect();
            let want = canonical(&t).then(|| t[2..].parse::<i64>().unwrap());
            assert_eq!(parse_id(&t), want, "{t:?}");
        }
        for t in ["", "l-", "l-0", "l--1", "l-+1", "l-01", "l-1 ", " l-1"] {
            assert_eq!(parse_id(t), None, "{t:?}");
        }
    }
}
