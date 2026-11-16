//! The read functions of the local API. The HTTP handlers and the read verbs
//! of the `iris` program call the same functions, so both give the same data.

use std::path::PathBuf;

use iris_store::{format_id, parse_id, Kind, Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use crate::{Error, Result};

/// The largest page of a list, and the largest list of a brief section.
pub const MAX_LIMIT: u32 = 200;

const FORMS: [(&str, Kind); 5] = [
    ("commitment", Kind::Commitment),
    ("conversation", Kind::Conversation),
    ("record", Kind::Record),
    ("feed", Kind::Feed),
    ("signal", Kind::Signal),
];

/// The status values of each form (the schemas of `docs/spec/schema/`).
const STATUSES: [(&str, &[&str]); 5] = [
    (
        "commitment",
        &["open", "done", "declined", "deferred", "cancelled", "gone"],
    ),
    ("conversation", &["active", "quiet", "muted", "gone"]),
    ("record", &["kept", "corrected", "gone"]),
    ("feed", &["new", "read", "skipped", "gone"]),
    (
        "signal",
        &["active", "used", "dismissed", "expired", "gone"],
    ),
];

const RECORD_TYPES: [&str; 7] = [
    "receipt",
    "booking",
    "statement",
    "ticket",
    "contract",
    "event",
    "other",
];

const DAY_MS: i64 = 86_400_000;

/// A limit must be 1 to [`MAX_LIMIT`].
fn check_limit(limit: u32) -> Result<()> {
    if (1..=MAX_LIMIT).contains(&limit) {
        Ok(())
    } else {
        Err(Error::Query(format!("limit must be 1 to {MAX_LIMIT}")))
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Status {
    /// The policy mode (specification 2). Release 1 applies nothing, so
    /// the mode is always `observe`.
    pub mode: &'static str,
    pub accounts: Vec<Account>,
    pub messages: Counts,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Account {
    pub id: i64,
    pub address: String,
    pub provider: String,
    pub state: Option<String>,
    /// When the first sync ended. `None` while the mirror is not ready.
    pub mirror_ready_at: Option<String>,
    /// The unfinished first sync or full resync, if there is one.
    pub sync: Option<SyncState>,
    pub last_sync: Option<SyncRun>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SyncState {
    pub kind: String,
    pub step: Option<i64>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SyncRun {
    pub kind: String,
    pub started: String,
    pub ended: Option<String>,
    pub result: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Counts {
    pub total: i64,
    pub fetched: i64,
    pub understood: i64,
    pub gone: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Address {
    pub name: Option<String>,
    pub address: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MessageSummary {
    pub id: String,
    pub thread: String,
    pub date: Option<String>,
    pub from: Address,
    pub subject: Option<String>,
    pub labels: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Message {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub to: Option<String>,
    pub cc: Option<String>,
    /// The clean text (specification 1, section 7.2). `None` before the
    /// understanding step cleaned the message. The text is data: show it as
    /// text, never as markup.
    pub text: Option<String>,
    pub gone: bool,
    /// The hostile marks of the message (specification 1, section 6.7).
    pub hostile: Vec<String>,
    /// The items that are not gone and that have this message as a source.
    pub items: Vec<String>,
}

/// One section of the brief: the full count, and at most [`MAX_LIMIT`] items.
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct Section {
    pub count: i64,
    pub items: Vec<Value>,
    /// Possible duplicates (specification 1, section 7.5): for an item of
    /// `items`, the IDs of the newer items with `duplicate_of` set to it. The
    /// brief shows the pair once, so the newer items are not in `items`.
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    pub duplicates: serde_json::Map<String, Value>,
}

/// The count of the proposed actions, in total and for each verb.
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct Proposal {
    pub count: i64,
    pub verbs: serde_json::Map<String, Value>,
}

/// The first brief (specification 1, section 8).
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct Brief {
    pub you_owe: Section,
    pub owed_to_you: Section,
    pub conversations: Section,
    /// The count of each record type.
    pub records: serde_json::Map<String, Value>,
    pub feed: Section,
    pub signals: Section,
    pub strangers: Section,
    /// The count of hostile mail of the last 7 days.
    pub hostile: i64,
    /// The count of messages that the model failed 3 times.
    pub not_understood: i64,
    /// The date of the oldest fetched message that is not gone: the brief
    /// covers the mail from this date (specification 1, section 4.2).
    pub oldest_fetched: Option<String>,
    /// The triage proposal of specification 1, section 8: the mailbox
    /// changes that observe mode would propose, with nothing applied.
    /// `None` until observe mode (tracker item R1-16) gives the function.
    pub proposal: Option<Proposal>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Page {
    pub items: Vec<Value>,
    /// The `before` value of the next page, or `None` on the last page.
    pub next: Option<String>,
}

/// A filter of [`items`]. Each field that is `None` matches each item.
#[derive(Debug, Default, Clone)]
pub struct Filter<'a> {
    pub form: Option<&'a str>,
    pub status: Option<&'a str>,
    /// The record type. Only for the form `record`.
    pub kind: Option<&'a str>,
    /// Give only items older than this item ID.
    pub before: Option<&'a str>,
}

pub fn status(store: &Store) -> Result<Status> {
    Ok(store.read(|c| {
        let mut st = c.prepare(
            "SELECT id, address, provider, state, mirror_ready_at, sync_kind, sync_step
             FROM account ORDER BY id",
        )?;
        let rows = st.query_map([], |r| {
            let kind: Option<String> = r.get(5)?;
            let step: Option<i64> = r.get(6)?;
            Ok(Account {
                id: r.get(0)?,
                address: r.get(1)?,
                provider: r.get(2)?,
                state: r.get(3)?,
                mirror_ready_at: r.get::<_, Option<i64>>(4)?.map(rfc3339),
                sync: kind.map(|kind| SyncState { kind, step }),
                last_sync: None,
            })
        })?;
        let mut accounts = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        for a in &mut accounts {
            a.last_sync = c
                .query_row(
                    "SELECT kind, started_at, ended_at, result FROM sync_log
                     WHERE account_id = ?1 ORDER BY id DESC LIMIT 1",
                    [a.id],
                    |r| {
                        Ok(SyncRun {
                            kind: r.get(0)?,
                            started: rfc3339(r.get(1)?),
                            ended: r.get::<_, Option<i64>>(2)?.map(rfc3339),
                            result: r.get(3)?,
                        })
                    },
                )
                .optional()?;
        }
        let messages = c.query_row(
            "SELECT count(*), count(fetched_at), count(understood_at), count(gone_at) FROM message",
            [],
            |r| {
                Ok(Counts {
                    total: r.get(0)?,
                    fetched: r.get(1)?,
                    understood: r.get(2)?,
                    gone: r.get(3)?,
                })
            },
        )?;
        // shortcut: release 1 has no policy file to read; read its mode when
        // release 2 adds the policy.
        Ok(Status {
            mode: "observe",
            accounts,
            messages,
        })
    })?)
}

/// Search the messages that are not gone, newest first (specification 1,
/// section 5.6). A search text that is not valid gives [`Error::Query`].
pub fn search(store: &Store, text: &str, limit: u32) -> Result<Vec<MessageSummary>> {
    check_limit(limit)?;
    let ids = store.search(text, limit)?;
    Ok(store.read(|c| {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some((summary, _)) = message_row(c, id)? {
                out.push(summary);
            }
        }
        Ok(out)
    })?)
}

/// One message by its `m-` ID, also when it is gone.
pub fn message(store: &Store, id: &str) -> Result<Option<Message>> {
    let row = row_of(id, Kind::Message)?;
    Ok(store.read(|c| {
        let Some((summary, more)) = message_row(c, row)? else {
            return Ok(None);
        };
        let mut st = c.prepare(
            "SELECT i.id, i.form FROM item_source s JOIN item i ON i.id = s.item_id
             WHERE s.message_id = ?1 AND i.status != 'gone' ORDER BY i.id",
        )?;
        let items = st
            .query_map([row], |r| Ok(item_id(r.get(0)?, &r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Some(Message {
            summary,
            items: items.into_iter().flatten().collect(),
            ..more
        }))
    })?)
}

/// The path of the raw source (RFC 5322) of one message, or `None` if the
/// message or its content file is not there.
pub fn raw(store: &Store, id: &str) -> Result<Option<PathBuf>> {
    let row = row_of(id, Kind::Message)?;
    let sha: Option<String> = store.read(|c| {
        Ok(c.query_row(
            "SELECT content_sha256 FROM message WHERE id = ?1",
            [row],
            |r| r.get(0),
        )
        .optional()?
        .flatten())
    })?;
    Ok(sha
        .and_then(|s| store.content_path(&s))
        .filter(|p| p.is_file()))
}

/// One item by its ID (`c-`, `v-`, `r-`, `f-` or `s-`), also when it is gone.
pub fn item(store: &Store, id: &str) -> Result<Option<Value>> {
    let (kind, row) = parse_id(id).ok_or_else(|| bad_id(id))?;
    let form = FORMS
        .iter()
        .find(|(_, k)| *k == kind)
        .map(|(f, _)| *f)
        .ok_or_else(|| bad_id(id))?;
    Ok(store.read(|c| {
        Ok(c.query_row(
            "SELECT id, form, status, doc FROM item WHERE id = ?1 AND form = ?2",
            params![row, form],
            doc_of,
        )
        .optional()?)
    })?)
}

/// The items that match `filter`, newest first. Items with the status
/// `gone` are left out, unless the filter asks for that status.
pub fn items(store: &Store, filter: &Filter, limit: u32) -> Result<Page> {
    if let Some(form) = filter.form {
        if !FORMS.iter().any(|(f, _)| *f == form) {
            return Err(Error::Query(format!("unknown form {form}")));
        }
    }
    if let Some(status) = filter.status {
        let known = STATUSES
            .iter()
            .filter(|(f, _)| filter.form.is_none_or(|form| form == *f))
            .any(|(_, list)| list.contains(&status));
        if !known {
            return Err(Error::Query(format!("unknown status {status}")));
        }
    }
    if let Some(kind) = filter.kind {
        if filter.form != Some("record") {
            return Err(Error::Query("type needs form=record".into()));
        }
        if !RECORD_TYPES.contains(&kind) {
            return Err(Error::Query(format!("unknown record type {kind}")));
        }
    }
    check_limit(limit)?;
    let before = match filter.before {
        // The cursor is an item ID of the form of the filter.
        Some(id) => match parse_id(id) {
            Some((kind, row))
                if FORMS
                    .iter()
                    .any(|(f, k)| *k == kind && filter.form.is_none_or(|form| form == *f)) =>
            {
                Some(row)
            }
            _ => return Err(bad_id(id)),
        },
        None => None,
    };
    let items = store.read(|c| {
        let mut st = c.prepare(
            "SELECT id, form, status, doc FROM item
             WHERE (?1 IS NULL OR form = ?1)
               AND (status = ?2 OR (?2 IS NULL AND status != 'gone'))
               AND (?3 IS NULL OR json_extract(doc, '$.type') = ?3)
               AND (?4 IS NULL OR id < ?4)
             ORDER BY id DESC LIMIT ?5",
        )?;
        let rows = st.query_map(
            params![filter.form, filter.status, filter.kind, before, limit + 1],
            doc_of,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    })?;
    let mut items = items;
    let next = if items.len() > limit as usize {
        items.truncate(limit as usize);
        items
            .last()
            .and_then(|d| d["id"].as_str().map(str::to_string))
    } else {
        None
    };
    Ok(Page { items, next })
}

/// The kind of the sender row of the `p-` ID at the JSON path `path` of the
/// item document `doc`. A sender with no row or no kind yet is a stranger.
fn kind_sql(doc: &str, path: &str) -> String {
    format!(
        "coalesce((SELECT kind FROM sender WHERE id =
           CAST(substr(json_extract({doc}, '{path}'), 3) AS INTEGER)), 'stranger')"
    )
}

/// The first brief at the time `now` (milliseconds since the Unix epoch).
pub fn brief(store: &Store, now: i64) -> Result<Brief> {
    let counter = kind_sql("doc", "$.counterparty.sender");
    let owner = kind_sql("doc", "$.owner.sender");
    let known = "IN ('person', 'service')";
    let stranger_participant = format!(
        "EXISTS (SELECT 1 FROM json_each(doc, '$.participants') p
           WHERE json_extract(p.value, '$.kind') = 'sender'
             AND {} = 'stranger')",
        kind_sql("p.value", "$.sender")
    );
    let commitment = "form = 'commitment' AND status = 'open'";
    // Times are compared as instants: RFC 3339 text with an offset does
    // not sort in time order. A date with no time is midnight UTC.
    let due = "unixepoch(coalesce(json_extract(doc, '$.due.at'), json_extract(doc, '$.due.date')), 'subsec')";
    let you_owe = (
        format!(
            "{commitment} AND json_extract(doc, '$.direction') = 'you_owe' AND {counter} {known}"
        ),
        format!("{due} IS NULL, {due}, created_at, id"),
    );
    let owed = (
        format!(
            "{commitment} AND json_extract(doc, '$.direction') = 'owed_to_you' AND {owner} {known}"
        ),
        "created_at, id".to_string(),
    );
    let conversations = (
        // A conversation with a stranger is also in `strangers`.
        "form = 'conversation' AND status = 'active'
         AND (json_extract(doc, '$.new') > 0 OR json_extract(doc, '$.your_turn') = 1)"
            .to_string(),
        "unixepoch(json_extract(doc, '$.latest.at'), 'subsec') DESC, id DESC".to_string(),
    );
    // Group by the address of the sender row of the publisher (the address of
    // the document when there is no row), then by the sender ID.
    let feed = (
        "form = 'feed' AND status = 'new'".to_string(),
        "coalesce((SELECT address FROM sender WHERE id =
            CAST(substr(json_extract(doc, '$.publisher.sender'), 3) AS INTEGER)),
            json_extract(doc, '$.publisher.address')),
         json_extract(doc, '$.publisher.sender'),
         unixepoch(json_extract(doc, '$.published'), 'subsec') DESC, id DESC"
            .to_string(),
    );
    let signals = (
        format!(
            "form = 'signal' AND status = 'active'
             AND unixepoch(json_extract(doc, '$.expires'), 'subsec') * 1000 > {now}"
        ),
        "unixepoch(json_extract(doc, '$.expires'), 'subsec'), id".to_string(),
    );
    let strangers = (
        format!(
            "(({commitment} AND (
                 (json_extract(doc, '$.direction') = 'you_owe' AND {counter} = 'stranger')
              OR (json_extract(doc, '$.direction') = 'owed_to_you' AND {owner} = 'stranger')))
              OR (form = 'conversation' AND status = 'active' AND {stranger_participant}))"
        ),
        "created_at DESC, id DESC".to_string(),
    );
    Ok(store.read(|c| {
        let section = |(filter, order): &(String, String)| -> iris_store::Result<Section> {
            // A possible duplicate whose original is in the same section
            // shows with the original, not as a line of its own. The inner
            // `item` of the subquery is the original.
            let shown = format!(
                "{filter} AND NOT (duplicate_of IS NOT NULL
                   AND duplicate_of IN (SELECT id FROM item WHERE {filter}))"
            );
            let count = c.query_row(
                &format!("SELECT count(*) FROM item WHERE {shown}"),
                [],
                |r| r.get(0),
            )?;
            let mut st = c.prepare(&format!(
                "SELECT id, form, status, doc FROM item WHERE {shown} ORDER BY {order} LIMIT {MAX_LIMIT}"
            ))?;
            let items = st
                .query_map([], doc_of)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut duplicates = serde_json::Map::new();
            // Only a duplicate that this section would show is part of the pair.
            let mut st = c.prepare(&format!(
                "SELECT id, form FROM item WHERE duplicate_of = ?1 AND {filter} ORDER BY id"
            ))?;
            for doc in &items {
                let Some(id) = doc["id"].as_str() else { continue };
                let Some((_, row)) = parse_id(id) else { continue };
                let dups: Vec<Value> = st
                    .query_map([row], |r| Ok(item_id(r.get(0)?, &r.get::<_, String>(1)?)))?
                    .filter_map(|d| d.transpose())
                    .map(|d| d.map(Value::String))
                    .collect::<rusqlite::Result<_>>()?;
                if !dups.is_empty() {
                    duplicates.insert(id.to_string(), dups.into());
                }
            }
            Ok(Section { count, items, duplicates })
        };
        let mut records = serde_json::Map::new();
        let mut st = c.prepare(
            "SELECT json_extract(doc, '$.type'), count(*) FROM item
             WHERE form = 'record' AND status != 'gone' GROUP BY 1 ORDER BY 1",
        )?;
        for row in st.query_map([], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?))
        })? {
            let (kind, n) = row?;
            records.insert(kind.unwrap_or_else(|| "other".into()), n.into());
        }
        let hostile = c.query_row(
            "SELECT count(*) FROM message m LEFT JOIN header h ON h.message_id = m.id
             WHERE m.gone_at IS NULL AND m.internal_date >= ?1
               AND ((m.hostile IS NOT NULL AND m.hostile NOT IN ('', '[]'))
                 OR EXISTS (SELECT 1 FROM sender s
                            WHERE s.address = h.from_addr COLLATE NOCASE AND s.kind = 'hostile'))",
            [now - 7 * DAY_MS],
            |r| r.get(0),
        )?;
        let not_understood = c.query_row(
            "SELECT count(*) FROM message
             WHERE gone_at IS NULL AND understood_at IS NULL AND tries >= 3",
            [],
            |r| r.get(0),
        )?;
        let oldest_fetched = c
            .query_row(
                "SELECT min(internal_date) FROM message
                 WHERE fetched_at IS NOT NULL AND gone_at IS NULL",
                [],
                |r| r.get::<_, Option<i64>>(0),
            )?
            .map(rfc3339);
        Ok(Brief {
            you_owe: section(&you_owe)?,
            owed_to_you: section(&owed)?,
            conversations: section(&conversations)?,
            records,
            feed: section(&feed)?,
            signals: section(&signals)?,
            strangers: section(&strangers)?,
            hostile,
            not_understood,
            oldest_fetched,
            // shortcut: no triage proposal until crates/iris-observe (R1-16)
            // merges; then call it here.
            proposal: None,
        })
    })?)
}

/// The summary of message `row`, and the other fields of [`Message`]
/// with no summary and no items.
fn message_row(c: &Connection, row: i64) -> rusqlite::Result<Option<(MessageSummary, Message)>> {
    let found = c
        .query_row(
            "SELECT m.thread_id, coalesce(h.date, m.internal_date), h.from_name, h.from_addr,
                    h.subject, h.to_addrs, h.cc_addrs, b.text, m.gone_at IS NOT NULL, m.hostile
             FROM message m LEFT JOIN header h ON h.message_id = m.id
             LEFT JOIN body b ON b.message_id = m.id WHERE m.id = ?1",
            [row],
            |r| {
                let hostile: Option<String> = r.get(9)?;
                Ok((
                    MessageSummary {
                        id: format_id(Kind::Message, row),
                        thread: format_id(Kind::Thread, r.get(0)?),
                        date: r.get::<_, Option<i64>>(1)?.map(rfc3339),
                        from: Address {
                            name: r.get(2)?,
                            address: r.get(3)?,
                        },
                        subject: r.get(4)?,
                        labels: Vec::new(),
                    },
                    Message {
                        summary: empty_summary(),
                        to: r.get(5)?,
                        cc: r.get(6)?,
                        text: r.get(7)?,
                        gone: r.get(8)?,
                        hostile: hostile
                            .and_then(|h| serde_json::from_str(&h).ok())
                            .unwrap_or_default(),
                        items: Vec::new(),
                    },
                ))
            },
        )
        .optional()?;
    let Some((mut summary, more)) = found else {
        return Ok(None);
    };
    let mut st = c.prepare_cached(
        "SELECT l.name FROM message_label ml JOIN label l ON l.id = ml.label_id
         WHERE ml.message_id = ?1 ORDER BY l.kind, l.name",
    )?;
    summary.labels = st
        .query_map([row], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some((summary, more)))
}

fn empty_summary() -> MessageSummary {
    MessageSummary {
        id: String::new(),
        thread: String::new(),
        date: None,
        from: Address {
            name: None,
            address: None,
        },
        subject: None,
        labels: Vec::new(),
    }
}

/// The document of an item row (id, form, status, doc). The row is the true
/// copy of the ID and the status, so they replace the values of the document.
fn doc_of(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    let id: i64 = r.get(0)?;
    let form: String = r.get(1)?;
    let text: String = r.get(3)?;
    let mut doc: Value = serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    if let Some(map) = doc.as_object_mut() {
        if let Some(id) = item_id(id, &form) {
            map.insert("id".into(), id.into());
        }
        map.insert("status".into(), Value::String(r.get(2)?));
    }
    Ok(doc)
}

fn item_id(row: i64, form: &str) -> Option<String> {
    FORMS
        .iter()
        .find(|(f, _)| *f == form)
        .map(|(_, k)| format_id(*k, row))
}

fn row_of(id: &str, kind: Kind) -> Result<i64> {
    match parse_id(id) {
        Some((k, row)) if k == kind => Ok(row),
        _ => Err(bad_id(id)),
    }
}

fn bad_id(id: &str) -> Error {
    Error::Query(format!("not a valid ID: {id}"))
}

/// Milliseconds since the Unix epoch as RFC 3339 in UTC, to the second.
pub fn rfc3339(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let rem = secs.rem_euclid(86_400);
    // Days since 1970-01-01 to the civil date: H. Hinnant's `civil_from_days`.
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_gives_utc_dates() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_763_283_731_999), "2025-11-16T09:02:11Z");
        assert_eq!(rfc3339(-1000), "1969-12-31T23:59:59Z");
    }
}
