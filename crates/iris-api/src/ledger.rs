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
    /// The messages whose change the recovery cannot confirm
    /// (`ledger_message.listed = 'unknown'`, section 7.3).
    pub unknown: i64,
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
    /// For an applied undo: the targets of `tried` that it did not
    /// change, because they changed again after the entry, an earlier undo
    /// restored them, or they are gone (decision D18). 0 for each other entry.
    pub skipped: i64,
    /// For an undo: the targets that this step tries to restore, that is
    /// each message that the undone entry changed (also a gone one) and each
    /// item of a state entry. Step 2 of the undo of `trash` (`archive`) has
    /// only the messages with no `INBOX` before the entry. 0 for each other
    /// entry.
    pub tried: i64,
    /// For an undo: true when an undo of the undone entry can
    /// start now (`ledger_can_undo`). False for each other entry.
    pub retry: bool,
    /// For a `send` or a `draft` entry: the subject and the recipients of
    /// its draft (issue 490). `None` for each other entry, and for a mail
    /// whose content Iris cannot read.
    pub mail: Option<Mail>,
    /// For a `draft` entry: true when it changed a draft that exists
    /// (`drafts.update`), not a create. An undo of a change is a change
    /// with `undo_of`.
    pub change: bool,
    /// The action of the entry, to read the content of its draft.
    #[serde(skip)]
    action: i64,
}

/// The words of the mail of a `send` or a `draft` entry.
#[derive(Debug, Serialize, PartialEq)]
pub struct Mail {
    /// At most 200 characters.
    pub subject: String,
    /// The first recipient of `to`. A checked draft has one or more.
    pub to: Option<String>,
    /// The number of all recipients (to, cc and bcc).
    pub recipients: usize,
}

/// The longest subject that an entry gives, in characters.
const SUBJECT: usize = 200;

/// Read the mail of a `send` or a `draft` entry, as it was at that entry
/// (issue 490). A later change of the draft, at Gmail or in Iris, never
/// changes the words of an older entry. An entry whose content Iris cannot
/// read keeps no mail.
fn with_mail(c: &Connection, mut e: LedgerEntry) -> LedgerEntry {
    if e.verb == "send" || e.verb == "draft" {
        e.mail = entry_content(c, e.action).ok().flatten().map(|m| Mail {
            subject: m.subject.chars().take(SUBJECT).collect(),
            to: m.to.first().cloned(),
            recipients: m.to.len() + m.cc.len() + m.bcc.len(),
        });
    }
    e
}

/// The `post` of a ledger row of a draft create or change: the content
/// that the row wrote (`iris-apply`, `Stored`).
#[derive(serde::Deserialize)]
struct Post {
    content: iris_compose::Content,
}

/// The content of the mail of the ledger entry of `action`. A create or a
/// change of a draft keeps its content in `post`. A send keeps only the
/// hash of the content that it sent (`ops.hash`). Each row with the same
/// hash in its ops wrote the same content, so the post of such a row
/// gives it. With no such row, the stored draft gives it only while its
/// content has that hash.
fn entry_content(c: &Connection, action: i64) -> rusqlite::Result<Option<iris_compose::Content>> {
    let from_post = |p: &str| serde_json::from_str::<Post>(p).ok().map(|p| p.content);
    let Some((ops, post)) = c
        .query_row(
            "SELECT ops, post FROM ledger WHERE action_id = ?1",
            [action],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    if let Some(post) = post {
        return Ok(from_post(&post));
    }
    let Some(hash) = serde_json::from_str::<Value>(&ops)
        .ok()
        .and_then(|o| o["hash"].as_str().map(String::from))
    else {
        return Ok(None);
    };
    let post: Option<String> = c
        .query_row(
            "SELECT post FROM ledger
              WHERE post IS NOT NULL AND json_valid(ops) AND json_extract(ops, '$.hash') = ?1
              ORDER BY id DESC LIMIT 1",
            [&hash],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(post) = post {
        return Ok(from_post(&post));
    }
    let draft: Option<(String, Vec<u8>)> = c
        .query_row(
            "SELECT a.address, d.raw FROM action_draft ad JOIN draft d ON d.id = ad.draft_id
               JOIN account a ON a.id = d.account_id WHERE ad.action_id = ?1",
            [action],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(draft.and_then(|(address, raw)| {
        let read = iris_compose::read(&raw, &address)?;
        (iris_compose::hash(&read.content, read.reply.as_ref()) == hash).then_some(read.content)
    }))
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

/// The entry that an undo undoes, or NULL for an entry that is no undo.
const UNDONE: &str = "(SELECT u.ledger_id FROM ledger_undo u WHERE u.action_id = a.id)";

/// The targets that one step of an undo tries to restore. Step 1: each
/// message that the undone entry changed (also one that is gone now,
/// because submit leaves it out of the undo), and each item of a state
/// entry. Step 2 (the `archive` of the undo of `trash`): each such message
/// that had no `INBOX` before the entry. NULL for an entry that is no undo.
const TRIED: &str = "(SELECT (SELECT count(*) FROM ledger_message lm
                WHERE lm.ledger_id = u.ledger_id AND lm.done = 1
                  AND (u.step = 1 OR NOT EXISTS (SELECT 1 FROM json_each(lm.pre) WHERE value = 'INBOX')))
               + CASE WHEN u.step = 1
                      THEN (SELECT count(*) FROM action_item ai JOIN ledger le ON le.id = u.ledger_id
                            WHERE ai.action_id = le.action_id)
                      ELSE 0 END
         FROM ledger_undo u WHERE u.action_id = a.id)";

/// The entry query. `{args}` is [`ARGS`]. Each text has at most 2,000
/// characters (`substr(x, 1, 2000)`), so a page of 200 entries stays small
/// also when an action has a large text (the body of a draft has up to
/// 256 KiB).
const ENTRY: &str = "SELECT l.id, a.verb, {args}, l.state, l.at, substr(l.error, 1, 2000),
        (SELECT count(*) FROM ledger_message lm WHERE lm.ledger_id = l.id),
        (SELECT count(*) FROM ledger_message lm WHERE lm.ledger_id = l.id AND lm.done = 1),
        (SELECT count(*) FROM ledger_message lm WHERE lm.ledger_id = l.id AND lm.listed = 'unknown'),
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
        CASE WHEN l.state = 'applied' AND {undone} IS NOT NULL
             THEN max(0, {tried}
                       - (SELECT count(*) FROM ledger_message lm WHERE lm.ledger_id = l.id AND lm.done = 1)
                       - (SELECT count(*) FROM json_each(coalesce(l.post, '{}'))))
             ELSE 0 END,
        coalesce({tried}, 0),
        EXISTS (SELECT 1 FROM ledger_can_undo v WHERE v.ledger_id = {undone}),
        a.id,
        json_valid(l.ops) AND json_extract(l.ops, '$.call') IS 'drafts.update'
     FROM ledger l
     JOIN changeset_action a ON a.id = l.action_id
     JOIN changeset c ON c.id = a.changeset_id
     LEFT JOIN job j ON c.author_job = 'j-' || j.id";

fn entry_sql(tail: &str) -> String {
    // The title of an item of hostile mail is not shown (issue 112).
    let entry = ENTRY
        .replace("{tried}", TRIED)
        .replace("{undone}", UNDONE)
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
        unknown: r.get(8)?,
        items: r.get(9)?,
        title: r.get(10)?,
        changeset: iris_changeset::format_changeset_id(r.get(11)?),
        author: Author {
            kind: r.get(12)?,
            name: r.get(13)?,
            job: r.get(14)?,
            goal: r.get(15)?,
        },
        decided_by: r.get(16)?,
        rule: r.get(17)?,
        policy: r.get(18)?,
        reason: r.get(19)?,
        why: r.get(20)?,
        can_undo: r.get(21)?,
        undo_open: r.get(22)?,
        undone_by: l(r.get(23)?),
        undo_of: l(r.get(24)?),
        skipped: r.get(25)?,
        tried: r.get(26)?,
        retry: r.get(27)?,
        mail: None,
        change: r.get(29)?,
        action: r.get(28)?,
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
        let rows: Vec<LedgerEntry> = rows.collect::<rusqlite::Result<_>>()?;
        Ok(rows.into_iter().map(|e| with_mail(c, e)).collect())
    })?;
    let more = entries.len() > limit as usize;
    entries.truncate(limit as usize);
    let next = more
        .then(|| entries.last().map(|e| e.id["l-".len()..].to_string()))
        .flatten();
    Ok(LedgerPage { entries, next })
}

fn one(c: &Connection, id: i64) -> rusqlite::Result<Option<LedgerEntry>> {
    Ok(c.query_row(&entry_sql("WHERE l.id = ?1"), [id], entry)
        .optional()?
        .map(|e| with_mail(c, e)))
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

/// Dismiss the line of the brief for the entries `ids` (`l-<n>`) whose
/// messages the recovery cannot confirm (specification 2, section 7.3).
/// The client sends the entries that the line showed, so an entry that
/// fails later shows a new line. An entry with no such message, or one
/// that is dismissed already, is left as it is. Gives the number of new
/// dismissals. An ID that is not in the canonical form is
/// [`Error::Query`], and nothing is written.
pub fn dismiss_unconfirmed(store: &Store, ids: &[String], now: i64) -> Result<usize> {
    if ids.len() > crate::MAX_LIMIT as usize {
        return Err(Error::Query(format!(
            "at most {} entries",
            crate::MAX_LIMIT
        )));
    }
    let rows = ids
        .iter()
        .map(|id| parse_id(id).ok_or_else(|| Error::Query(format!("no such entry form: {id:?}"))))
        .collect::<Result<Vec<i64>>>()?;
    Ok(store.write(|tx| {
        let mut n = 0;
        for id in &rows {
            n += tx.execute(
                "INSERT OR IGNORE INTO ledger_unknown_seen(ledger_id, at)
                 SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM ledger_message
                   WHERE ledger_id = ?1 AND listed = 'unknown')",
                params![id, now],
            )?;
        }
        Ok(n)
    })?)
}

/// The line of the brief (specification 2, section 7.3): the newest
/// [`crate::MAX_LIMIT`] entries with messages that the recovery cannot
/// confirm, that the person did not dismiss, of an account that is not
/// removed. `messages` counts the messages of these entries only, so the
/// line is the entries that it lists, and one dismissal of them covers
/// the line. `more` counts the other such entries: they show in the next
/// line.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Unconfirmed {
    pub messages: i64,
    /// At most [`crate::MAX_LIMIT`] entries, as `l-<n>`, newest first.
    pub entries: Vec<String>,
    pub more: i64,
}

pub(crate) fn unconfirmed(c: &Connection) -> rusqlite::Result<Unconfirmed> {
    // The brief shows nothing of an account in `removing` or `removed`
    // (remove account spec).
    const OPEN: &str = "FROM ledger_message lm
           JOIN ledger l ON l.id = lm.ledger_id
           JOIN account a ON a.id = l.account_id
         WHERE lm.listed = 'unknown'
           AND (a.state IS NULL OR a.state NOT IN ('removing', 'removed'))
           AND NOT EXISTS (SELECT 1 FROM ledger_unknown_seen s
                           WHERE s.ledger_id = lm.ledger_id)";
    let rows: Vec<(i64, i64)> = c
        .prepare(&format!(
            "SELECT lm.ledger_id, count(*) {OPEN}
             GROUP BY lm.ledger_id ORDER BY lm.ledger_id DESC LIMIT ?1"
        ))?
        .query_map([crate::MAX_LIMIT], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let all: i64 = c.query_row(
        &format!("SELECT count(DISTINCT lm.ledger_id) {OPEN}"),
        [],
        |r| r.get(0),
    )?;
    Ok(Unconfirmed {
        messages: rows.iter().map(|r| r.1).sum(),
        more: all - rows.len() as i64,
        entries: rows.iter().map(|r| format!("l-{}", r.0)).collect(),
    })
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
