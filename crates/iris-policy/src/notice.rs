//! Notices by rule (card R5-08, `docs/specs/2026-11-16-notices.md`): one
//! notice for each new mail that a `notify` rule of the active policy
//! matches. The engine shows each new notice in the system, and the web
//! client shows each notice that is not done.

use iris_store::{Result, Store};
use rusqlite::{params, Connection, OptionalExtension};

/// A mail is new for one hour after its fetch.
pub const WINDOW_MS: i64 = 3_600_000;

/// A mail that the understanding reads late is a candidate up to this time
/// after its fetch. After that, the interrupt comes too late to help.
pub const LATE_MS: i64 = 24 * WINDOW_MS;

/// The most characters of the title and of the text of a notice.
pub const WORDS_MAX: usize = 200;

/// The most notices that [`list`] gives.
pub const LIST_MAX: i64 = 50;

/// One notice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub id: i64,
    pub message_id: i64,
    pub rule: u32,
    pub rule_name: String,
    /// The sender, one line.
    pub title: String,
    /// The subject, one line.
    pub text: String,
    /// Milliseconds since 1970-01-01 UTC.
    pub created_at: i64,
}

/// Adds a notice for each new mail that a `notify` rule of the active
/// policy matches, at the time `now_ms`, and gives the new notices. A mail
/// is a candidate when it came into the mirror, or the understanding wrote
/// its items, [`WINDOW_MS`] or less before `now_ms`, so that a rule on a form
/// waits for the model, up to [`LATE_MS`] after the fetch. Its date must be at most [`WINDOW_MS`] before the
/// fetch, so that the old mail of a sync gives no notice. A mail with no `body` row waits: the
/// hostile check writes with the body. Each guard reads the store in the
/// transaction that writes the notice. A notice keeps the rule and the
/// policy version of its time.
pub fn refresh(store: &Store, now_ms: i64) -> Result<Vec<Notice>> {
    store.write(|tx| {
        let active = crate::active(tx)?;
        let Some(version) = active.version else {
            return Ok(Vec::new());
        };
        if !active
            .policy
            .rules
            .iter()
            .any(|r| r.effect == crate::Effect::Notify)
        {
            return Ok(Vec::new());
        }
        let ids: Vec<i64> = tx
            .prepare(
                "SELECT m.id FROM message m
                 WHERE (m.fetched_at >= ?1 OR (m.understood_at >= ?1 AND m.fetched_at >= ?3))
                   AND m.gone_at IS NULL
                   AND m.internal_date >= m.fetched_at - ?2
                   AND EXISTS (SELECT 1 FROM body b WHERE b.message_id = m.id)
                   AND NOT EXISTS (SELECT 1 FROM notice n WHERE n.message_id = m.id)
                   AND NOT EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                       WHERE ml.message_id = m.id AND l.provider_id IN ('SPAM', 'TRASH'))
                 ORDER BY m.id",
            )?
            .query_map(params![now_ms - WINDOW_MS, WINDOW_MS, now_ms - LATE_MS], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let now = now_ms.div_euclid(1000);
        let mut out = Vec::new();
        for id in ids {
            let Some(f) = crate::facts(tx, id, now)? else {
                continue;
            };
            let Some(rule) = active.policy.notice(&f.message(), now) else {
                continue;
            };
            let rule_name = active
                .policy
                .rules
                .iter()
                .find(|r| r.id == rule)
                .map_or(String::new(), |r| r.name.clone());
            let (title, text) = words(tx, id)?;
            let added = tx.execute(
                "INSERT INTO notice(message_id, rule, rule_name, policy_version, title, text, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(message_id) DO NOTHING",
                params![id, rule, rule_name, version, title, text, now_ms],
            )?;
            if added > 0 {
                out.push(Notice {
                    id: tx.last_insert_rowid(),
                    message_id: id,
                    rule,
                    rule_name,
                    title,
                    text,
                    created_at: now_ms,
                });
            }
        }
        Ok(out)
    })
}

/// The sender and the subject of a message, one line each.
fn words(tx: &Connection, id: i64) -> Result<(String, String)> {
    let (name, addr, subject): (Option<String>, Option<String>, Option<String>) = tx
        .query_row(
            "SELECT h.from_name, h.from_addr, coalesce(h.subject, b.subject)
             FROM message m LEFT JOIN header h ON h.message_id = m.id
             LEFT JOIN body b ON b.message_id = m.id WHERE m.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .unwrap_or_default();
    let pick = |v: Option<String>| v.map(|s| line(&s)).filter(|s| !s.is_empty());
    let title = pick(name)
        .or_else(|| pick(addr))
        .unwrap_or_else(|| "Unknown sender".into());
    let text = pick(subject).unwrap_or_else(|| "(No subject)".into());
    Ok((title, text))
}

/// One line of at most [`WORDS_MAX`] characters: each control character and
/// each run of white space becomes one space.
pub fn line(s: &str) -> String {
    let one: String = s
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if one.chars().count() <= WORDS_MAX {
        one
    } else {
        one.chars().take(WORDS_MAX - 3).collect::<String>() + "..."
    }
}

/// The notices that are not done, newest first, at most [`LIST_MAX`].
pub fn list(conn: &Connection) -> Result<Vec<Notice>> {
    let rows = conn
        .prepare(
            "SELECT id, message_id, rule, rule_name, title, text, created_at FROM notice
             WHERE done_at IS NULL ORDER BY id DESC LIMIT ?1",
        )?
        .query_map([LIST_MAX], |r| {
            Ok(Notice {
                id: r.get(0)?,
                message_id: r.get(1)?,
                rule: r.get(2)?,
                rule_name: r.get(3)?,
                title: r.get(4)?,
                text: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Marks notice `id` done at `now_ms`. False if no notice has the ID. A
/// notice that is done already stays as it is.
pub fn done(store: &Store, id: i64, now_ms: i64) -> Result<bool> {
    store.write(|tx| {
        tx.execute(
            "UPDATE notice SET done_at = ?2 WHERE id = ?1 AND done_at IS NULL",
            params![id, now_ms],
        )?;
        Ok(tx
            .query_row("SELECT 1 FROM notice WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .is_some())
    })
}
