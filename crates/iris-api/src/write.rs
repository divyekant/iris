//! The write view of the web client (docs/specs/2026-11-17-write.md): the
//! press of Send (`POST /api/write`), the state of a write
//! (`GET /api/write/{cs}`) and the start values of a reply
//! (`GET /api/threads/{t}/reply`). The engine makes the draft and the send
//! (`iris_changeset::write_mail`, `iris_changeset::send_written`).

use iris_changeset::{Mail, Press, Refusal, Written};
use iris_store::{format_id, parse_id, Kind, Store};
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::read::{message_in, rfc3339};
use crate::review::changeset_error;
use crate::{Error, Result};

/// The body of `POST /api/write` (section 2).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteBody {
    pub account: Option<String>,
    pub thread: Option<String>,
    pub parent: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: Option<String>,
    pub body: String,
    pub key: String,
    pub flags: Vec<String>,
}

/// The answer of a press.
#[derive(Debug, PartialEq)]
pub enum Pressed {
    /// 201: a new write, `cs-N`.
    New(String),
    /// 200: an earlier try of the same press, `cs-N`.
    Again(String),
    /// 409: the text, and the flags to show (empty if none).
    Conflict(String, Vec<String>),
}

/// The text of a 409 for a reply whose parent is not the newest mail.
pub const NEW_PARENT: &str = "A new mail came into this thread. Read it, then send again.";
/// The text of a 409 for warnings that the person did not see.
pub const LOOK: &str = "Look at the warnings, then press Send again.";
/// The text of a 409 for a key of another mail.
pub const KEY_TAKEN: &str = "This key belongs to another mail.";

fn valid_key(k: &str) -> bool {
    (16..=64).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The press of Send (section 2). A body that is not valid, and a mail
/// that the submit denies, are [`Error::Query`] (400): nothing is written.
pub fn write(store: &Store, b: &WriteBody, now_ms: i64) -> Result<Pressed> {
    if !valid_key(&b.key) {
        return Err(Error::Query(
            "key must be 16 to 64 letters, digits or hyphens".into(),
        ));
    }
    let mut args = Map::new();
    args.insert("to".into(), json!(b.to));
    args.insert("cc".into(), json!(b.cc));
    args.insert("body".into(), json!(b.body));
    let mail = match (&b.account, &b.thread) {
        (Some(account), None) => {
            if b.parent.is_some() {
                return Err(Error::Query("a new mail has no parent".into()));
            }
            let Some(subject) = &b.subject else {
                return Err(Error::Query("a new mail needs a subject".into()));
            };
            args.insert("subject".into(), json!(subject));
            Mail::New { account }
        }
        (None, Some(thread)) => {
            if b.subject.is_some() {
                return Err(Error::Query("a reply has no subject: Iris sets it".into()));
            }
            let parent = match b.parent.as_deref().and_then(parse_id) {
                Some((Kind::Message, row)) => row,
                _ => return Err(Error::Query("a reply needs its parent, m-N".into())),
            };
            Mail::Reply { thread, parent }
        }
        _ => {
            return Err(Error::Query(
                "give one of account (a new mail) or thread (a reply)".into(),
            ))
        }
    };
    let press = Press {
        mail,
        args: &args,
        key: &b.key,
        flags: &b.flags,
    };
    let cs = iris_changeset::format_changeset_id;
    match iris_changeset::write_mail(store, &press, now_ms).map_err(changeset_error)? {
        Ok(Written::New(row)) => Ok(Pressed::New(cs(row))),
        Ok(Written::Again(row)) => Ok(Pressed::Again(cs(row))),
        Err(Refusal::Denied(why)) => Err(Error::Query(why)),
        Err(Refusal::NewParent) => Ok(Pressed::Conflict(NEW_PARENT.into(), vec![])),
        Err(Refusal::Flags(flags)) => Ok(Pressed::Conflict(LOOK.into(), flags)),
        Err(Refusal::KeyTaken) => Ok(Pressed::Conflict(KEY_TAKEN.into(), vec![])),
    }
}

/// The text of a write whose mark got no send in [`iris_changeset::MARK_MS`].
pub const TOO_OLD: &str =
    "Iris did not send this mail in 24 hours, so it stopped. Look for the draft in your Gmail drafts.";

/// The state of the write `cs` (a changeset row ID) at `now_ms`, section 4.
/// `None` if the changeset has no send mark.
pub fn state(store: &Store, cs: i64, now_ms: i64) -> Result<Option<Value>> {
    Ok(store.read(|conn| {
        // One snapshot: the engine moves the write at the same time.
        let tx = conn.unchecked_transaction()?;
        let v = state_in(&tx, cs, now_ms)?;
        tx.finish()?;
        Ok(v)
    })?)
}

type Mark = (
    String,
    Option<String>,
    Option<i64>,
    Option<i64>,
    bool,
    bool,
    String,
    i64,
);

fn state_in(c: &Connection, cs: i64, now_ms: i64) -> rusqlite::Result<Option<Value>> {
    let mark: Option<Mark> = c
        .query_row(
            "SELECT a.state, coalesce(a.reason, l.error), w.send_action_id, d.id,
                    d.foreign_at IS NOT NULL, d.sent_at IS NOT NULL, c.state, w.created_at
             FROM write_send w
             JOIN changeset_action a ON a.id = w.action_id
             JOIN changeset c ON c.id = a.changeset_id
             LEFT JOIN draft d ON d.action_id = a.id
             LEFT JOIN ledger l ON l.action_id = a.id
             WHERE a.changeset_id = ?1",
            [cs],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .optional()?;
    let Some((state, reason, send, draft, foreign, sent, changeset, pressed)) = mark else {
        return Ok(None);
    };
    let draft = draft.map(|d| format_id(Kind::Draft, d));
    let stopped = |why: &str| json!({"state": "stopped", "reason": why, "draft": draft});
    if let Some(send) = send {
        let (s, why, decided_at): (String, Option<String>, Option<i64>) = c.query_row(
            "SELECT a.state, coalesce(a.reason, l.error), a.decided_at FROM changeset_action a
             LEFT JOIN ledger l ON l.action_id = a.id WHERE a.id = ?1",
            [send],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let on: Option<i64> =
            c.query_row("SELECT on_at FROM send_switch WHERE id = 1", [], |r| {
                r.get(0)
            })?;
        return Ok(Some(match s.as_str() {
            "approved" | "held" => json!({
                "state": "waiting",
                "draft": draft,
                "sending": on.is_some(),
                "leaves_at": on.zip(decided_at)
                    .map(|(on, at)| rfc3339(at.max(on) + iris_apply::HOLD_MS)),
            }),
            "in_review" => json!({"state": "review", "draft": draft, "reason": why}),
            "applied" => json!({"state": "sent", "draft": draft}),
            "cancelled" if changeset == "withdrawn" => {
                stopped("You stopped this mail. The draft stays in your Gmail drafts.")
            }
            _ => stopped(why.as_deref().unwrap_or("The send stopped.")),
        }));
    }
    // Step 2a makes no send for this mark any more, whatever the state of
    // its draft action (spec section 4).
    if pressed <= now_ms - iris_changeset::MARK_MS {
        return Ok(Some(stopped(TOO_OLD)));
    }
    Ok(Some(match state.as_str() {
        "cancelled" => stopped("You stopped this mail."),
        "applied" if changeset == "withdrawn" => {
            stopped("You stopped this mail. The draft stays in your Gmail drafts.")
        }
        "applied" if sent => stopped("The mail went out from Gmail."),
        "applied" if foreign => stopped("The draft changed in Gmail. Send it from Gmail."),
        "approved" | "applied" => json!({"state": "saving"}),
        _ => stopped(reason.as_deref().unwrap_or("Iris could not save the mail.")),
    }))
}

/// The most characters of the parent text in the reply form.
pub const PARENT_TEXT: usize = 600;

/// The start values of a reply in the thread `t-N` (section 5). `None` if
/// the thread does not exist. [`Error::Conflict`] if it has no parent.
pub fn reply(store: &Store, thread: &str) -> Result<Option<Value>> {
    let Some((Kind::Thread, row)) = parse_id(thread) else {
        return Ok(None);
    };
    store
        .read(|conn| {
            let tx = conn.unchecked_transaction()?;
            let v = reply_in(&tx, row);
            tx.finish()?;
            Ok(v)
        })
        .map_err(Error::from)?
}

fn reply_in(c: &Connection, thread: i64) -> Result<Option<Value>> {
    let exists: bool = c
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM thread WHERE id = ?1)",
            [thread],
            |r| r.get(0),
        )
        .map_err(iris_store::Error::from)?;
    if !exists {
        return Ok(None);
    }
    let Some(parent) = iris_changeset::reply_parent(c, thread).map_err(iris_store::Error::from)?
    else {
        return Err(Error::Conflict(
            "This thread has no mail to reply to.".into(),
        ));
    };
    let (account, address, subject, to): (i64, String, Option<String>, Option<String>) = c
        .query_row(
            "SELECT m.account_id, a.address, h.subject, h.to_addrs FROM message m
             JOIN account a ON a.id = m.account_id
             LEFT JOIN header h ON h.message_id = m.id WHERE m.id = ?1",
            [parent],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(iris_store::Error::from)?;
    let m = message_in(c, parent)
        .map_err(iris_store::Error::from)?
        .expect("reply_parent gives a message");
    let from = m.summary.from.address.clone().unwrap_or_default();
    // The person wrote the newest mail: the reply goes to its recipients.
    let to: Vec<String> = if from.eq_ignore_ascii_case(&address) {
        to.unwrap_or_default()
            .split(',')
            .map(|a| a.trim().to_string())
            .collect()
    } else {
        vec![from]
    };
    let to: Vec<String> = to.into_iter().filter(|a| valid_address(a)).collect();
    let text = m
        .text
        .as_deref()
        .map(|t| match t.char_indices().nth(PARENT_TEXT) {
            Some((i, _)) => format!("{}…", &t[..i]),
            None => t.to_string(),
        });
    Ok(Some(json!({
        "account": format_id(Kind::Account, account),
        "parent": {
            "id": m.summary.id,
            "from": m.summary.from,
            "date": m.summary.date,
            "text": text,
        },
        "to": to,
        "cc": [],
        "subject": iris_compose::reply_subject(subject.as_deref().unwrap_or("")),
    })))
}

/// True if the address passes the address check of a draft (R3-01).
fn valid_address(a: &str) -> bool {
    !a.is_empty()
        && iris_compose::Content {
            to: vec![a.to_string()],
            cc: vec![],
            subject: "s".into(),
            body: String::new(),
        }
        .check()
        .is_ok()
}
