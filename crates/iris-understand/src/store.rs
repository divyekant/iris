//! Understanding of one message of the store (specification 1, section 7.1).
//!
//! Three writes:
//! 1. Steps 1 to 3: the header row, the clean text and the hostile marks.
//! 2. No write while the model works, so the model never holds the write lock.
//! 3. Step 8: the items, their sources and `understood_at`, in one
//!    transaction. A crash before the commit leaves the message not
//!    understood, and the next run starts again from step 3.

use std::fmt;

use iris_store::{Body, Store};
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::Value;

use crate::{
    analyze, decide, validate, Analysis, Context, Decision, Ids, Kind, Mark, Model, Usage,
};

/// The wait after a failed try of the model (section 7.4).
pub const RETRY_MS: i64 = 86_400_000;

#[derive(Debug)]
pub enum Error {
    Store(iris_store::Error),
    /// The model call failed. The message stays not understood, with no
    /// added try. If `is_retryable()`, try again later.
    Model(iris_model::Error),
    /// The message is not in the store, or it has no content file.
    NoMessage(i64),
    /// A finished item is not valid: a defect of the engine. Nothing of
    /// step 8 is written.
    Invalid(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Store(e) => write!(f, "store: {e}"),
            Error::Model(e) => write!(f, "model: {e}"),
            Error::NoMessage(id) => write!(f, "message {id} is not in the store or has no content"),
            Error::Invalid(e) => write!(f, "item not valid: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<iris_store::Error> for Error {
    fn from(e: iris_store::Error) -> Self {
        Error::Store(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Store(e.into())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Store(e.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The result of [`understand`] for one message.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// Row IDs of the new items.
    pub items: Vec<i64>,
    /// Hostile mail: no item, no model call (or a `phishing` answer).
    pub hostile: bool,
    /// A draft (label `DRAFT`): no item.
    pub draft: bool,
    /// The message is gone: nothing changed.
    pub gone: bool,
    /// Two answers were not valid: `tries` went up, `retry_at` is set.
    pub failed: bool,
    pub calls: u32,
    pub usage: Usage,
}

struct Row {
    account: i64,
    thread: i64,
    sha: Option<String>,
    internal_date: Option<i64>,
    gone: bool,
    me: String,
    labels: Vec<String>,
}

/// Run understanding on message `message` (its row ID): steps 1 to 8. Run
/// it again on the same message to understand it again: the items of the
/// message that a person did not correct are made again, not duplicated.
pub fn understand(store: &Store, message: i64, model: &dyn Model, now_ms: i64) -> Result<Outcome> {
    let row = read_row(store, message)?;
    if row.gone {
        return Ok(Outcome {
            gone: true,
            ..Outcome::default()
        });
    }
    let path = row
        .sha
        .as_deref()
        .and_then(|s| store.content_path(s))
        .ok_or(Error::NoMessage(message))?;
    // shortcut: reads the whole content file into memory (at most the Gmail
    // message limit, about 36 MB). Map the file instead if the memory budget
    // test shows a peak from large messages.
    let raw = std::fs::read(path)?;

    let sent = row.labels.iter().any(|l| l == "SENT");
    let draft = row.labels.iter().any(|l| l == "DRAFT");
    let from = iris_from(&raw);
    let kind = if sent {
        Kind::SelfOwner
    } else {
        sender_kind(store, from.as_deref())?
    };
    let known = |name: &str| known_person(store, name);
    let cx = Context {
        me: &row.me,
        kind,
        labels: &row.labels,
        internal_date: row.internal_date,
        now_ms,
        known_person: &known,
    };
    let a = analyze(&raw, &cx);
    drop(raw);
    // A draft is not understood and is not hostile mail (section 6.7).
    let marks = if draft { Vec::new() } else { a.marks.clone() };

    // Write 1: steps 1 to 3.
    store.write(|tx| {
        put_header(tx, message, &a)?;
        tx.execute(
            "UPDATE message SET hostile = ?2 WHERE id = ?1",
            params![message, marks_json(&marks, false)],
        )?;
        Ok(())
    })?;
    let sender = match (&a.head.from_name, &a.head.from_addr) {
        (Some(n), Some(ad)) => format!("{n} <{ad}>"),
        (_, Some(ad)) => ad.clone(),
        _ => String::new(),
    };
    store.put_body(
        message,
        &Body {
            subject: a.text.subject.clone(),
            sender,
            text: a.text.text.clone(),
        },
    )?;

    let d = if draft {
        Decision::default()
    } else {
        decide(&a, &cx, model).map_err(Error::Model)?
    };
    let ids = Ids {
        message,
        thread: row.thread,
        now_ms,
    };
    let mut out = Outcome {
        hostile: d.hostile,
        draft,
        failed: d.failed,
        calls: d.calls,
        usage: d.usage,
        ..Outcome::default()
    };

    // Write 3: step 8.
    let mut invalid = None;
    let written = store.write(|tx| {
        if d.failed {
            tx.execute(
                "UPDATE message SET tries = tries + 1, retry_at = ?2 WHERE id = ?1",
                params![message, now_ms + RETRY_MS],
            )?;
            return Ok(Vec::new());
        }
        if d.phishing {
            tx.execute(
                "UPDATE message SET hostile = ?2 WHERE id = ?1",
                params![message, marks_json(&marks, true)],
            )?;
        }
        drop_old_items(tx, message)?;
        let mut items = Vec::new();
        for draft in &d.drafts {
            items.push(put_item(
                tx,
                row.account,
                &ids,
                draft,
                model.name(),
                &mut invalid,
            )?);
        }
        // shortcut: call iris_talk::update(tx, row.thread, Some(message), now_ms)
        // here (step 4 and step 7) when iris-talk (W15) merges.
        tx.execute(
            "UPDATE message SET understood_at = ?2, retry_at = NULL WHERE id = ?1",
            params![message, now_ms],
        )?;
        Ok(items)
    });
    out.items = match (written, invalid) {
        (_, Some(e)) => return Err(Error::Invalid(e)),
        (r, None) => r?,
    };
    Ok(out)
}

fn read_row(store: &Store, message: i64) -> Result<Row> {
    let row = store.read(|c| {
        let r = c
            .query_row(
                "SELECT m.account_id, m.thread_id, m.content_sha256, m.internal_date,
                        m.gone_at IS NOT NULL, a.address
                 FROM message m JOIN account a ON a.id = m.account_id WHERE m.id = ?1",
                [message],
                |r| {
                    Ok(Row {
                        account: r.get(0)?,
                        thread: r.get(1)?,
                        sha: r.get(2)?,
                        internal_date: r.get(3)?,
                        gone: r.get(4)?,
                        me: r.get::<_, String>(5)?.to_ascii_lowercase(),
                        labels: Vec::new(),
                    })
                },
            )
            .optional()?;
        let Some(mut row) = r else { return Ok(None) };
        let mut st = c.prepare_cached(
            "SELECT l.provider_id FROM message_label ml JOIN label l ON l.id = ml.label_id
             WHERE ml.message_id = ?1",
        )?;
        row.labels = st
            .query_map([message], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Some(row))
    })?;
    row.ok_or(Error::NoMessage(message))
}

fn iris_from(raw: &[u8]) -> Option<String> {
    crate::Head::parse(raw).from_addr
}

/// The kind of the sender (step 3).
// shortcut: reads the kind that iris-senders wrote, a missing row or kind is
// a stranger. Call iris_senders::decide(tx, message, raw, now) here when
// iris-senders (W20) merges.
fn sender_kind(store: &Store, from: Option<&str>) -> Result<Kind> {
    let Some(from) = from else {
        return Ok(Kind::Stranger);
    };
    let kind: Option<Option<String>> = store.read(|c| {
        Ok(
            c.query_row("SELECT kind FROM sender WHERE address = ?1", [from], |r| {
                r.get(0)
            })
            .optional()?,
        )
    })?;
    Ok(kind.flatten().map_or(Kind::Stranger, |k| Kind::parse(&k)))
}

/// The address of the known person with this display name. A read error
/// gives no match.
fn known_person(store: &Store, name: &str) -> Option<String> {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT address FROM sender WHERE kind = 'person' AND name = ?1 COLLATE NOCASE
                 ORDER BY id LIMIT 1",
                [name],
                |r| r.get(0),
            )
            .optional()?)
        })
        .ok()
        .flatten()
}

fn marks_json(marks: &[Mark], phishing: bool) -> String {
    let mut names: Vec<&str> = marks.iter().map(|m| m.name()).collect();
    if phishing {
        names.push(Mark::Phishing.name());
    }
    Value::from(names).to_string()
}

fn put_header(tx: &Transaction, message: i64, a: &Analysis) -> rusqlite::Result<()> {
    let h = &a.head;
    let list = |v: &[String]| (!v.is_empty()).then(|| v.join(","));
    let subject = Some(a.text.subject.clone()).filter(|s| !s.is_empty());
    tx.prepare_cached(
        "INSERT INTO header(message_id, date, from_addr, from_name, to_addrs, cc_addrs, subject,
           message_id_hdr, in_reply_to, list_id, auth)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(message_id) DO UPDATE SET
           date = excluded.date, from_addr = excluded.from_addr, from_name = excluded.from_name,
           to_addrs = excluded.to_addrs, cc_addrs = excluded.cc_addrs, subject = excluded.subject,
           message_id_hdr = excluded.message_id_hdr, in_reply_to = excluded.in_reply_to,
           list_id = excluded.list_id, auth = excluded.auth",
    )?
    .execute(params![
        message,
        h.date,
        h.from_addr,
        h.from_name,
        list(&h.to),
        list(&h.cc),
        subject,
        h.message_id,
        h.in_reply_to,
        h.list_id,
        h.auth.as_ref().map(|a| a.json()),
    ])?;
    Ok(())
}

/// Before understanding runs again: delete the items whose origin is this
/// message, that a rule or the model made, and that are not pinned (no
/// correction, section 7.5). Conversations belong to the conversation rule.
// shortcut: pinned means "has a correction" only; add the references of
// open changesets and of the ledger when specification 2 builds them.
fn drop_old_items(tx: &Transaction, message: i64) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM item WHERE id IN (
           SELECT i.id FROM item_source s JOIN item i ON i.id = s.item_id
           WHERE s.message_id = ?1 AND s.role = 'origin' AND i.form <> 'conversation'
             AND i.derived_by IN ('rule', 'model')
             AND NOT EXISTS (SELECT 1 FROM correction c WHERE c.item_id = i.id))",
        [message],
    )?;
    Ok(())
}

fn put_item(
    tx: &Transaction,
    account: i64,
    ids: &Ids,
    d: &crate::Draft,
    model: &str,
    invalid: &mut Option<String>,
) -> iris_store::Result<i64> {
    let form = d.form().to_string();
    let by = match d.by {
        crate::By::Rule(_) => "rule",
        crate::By::Model => "model",
    };
    let id: i64 = tx.query_row(
        "INSERT INTO item(form, account_id, status, doc, created_at, updated_at, derived_by)
         VALUES (?1, ?2, '', '{}', ?3, ?3, ?4) RETURNING id",
        params![form, account, ids.now_ms, by],
        |r| r.get(0),
    )?;
    let mut failed = None;
    let mut sender = |addr: &str, name: Option<&str>| -> i64 {
        sender_row(tx, addr, name).unwrap_or_else(|e| {
            failed.get_or_insert(e);
            0
        })
    };
    let doc = d.finish(ids, id, model, &mut sender);
    if let Some(e) = failed {
        return Err(e.into());
    }
    // The draft passed the same check with placeholder IDs. A failure here
    // is a defect of the engine, so the whole step rolls back.
    if let Err(e) = validate(&doc) {
        *invalid = Some(e);
        return Err(iris_store::Error::Query("an item is not valid".into()));
    }
    // A possible duplicate of a pinned item (one with a correction): same
    // form, same origin message.
    let pinned: Option<i64> = tx
        .query_row(
            "SELECT i.id FROM item_source s JOIN item i ON i.id = s.item_id
             WHERE s.message_id = ?1 AND s.role = 'origin' AND i.form = ?2
               AND EXISTS (SELECT 1 FROM correction c WHERE c.item_id = i.id)
             ORDER BY i.id LIMIT 1",
            params![ids.message, form],
            |r| r.get(0),
        )
        .optional()?;
    tx.execute(
        "UPDATE item SET status = ?2, doc = ?3, duplicate_of = ?4 WHERE id = ?1",
        params![
            id,
            doc["status"].as_str().unwrap_or(""),
            doc.to_string(),
            pinned
        ],
    )?;
    tx.execute(
        "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
        params![id, ids.message],
    )?;
    Ok(id)
}

/// The row ID of the sender row of `addr`. A new row has no kind:
/// iris-senders decides it.
fn sender_row(tx: &Transaction, addr: &str, name: Option<&str>) -> rusqlite::Result<i64> {
    let addr = addr.to_ascii_lowercase();
    tx.execute(
        "INSERT INTO sender(address, name) VALUES (?1, ?2) ON CONFLICT(address) DO NOTHING",
        params![addr, name],
    )?;
    tx.query_row("SELECT id FROM sender WHERE address = ?1", [addr], |r| {
        r.get(0)
    })
}
