//! The write view of the person (docs/specs/2026-11-17-write.md): one press
//! of Send approves a `draft` action and asks for the send of the draft
//! that the action makes. The send mark (`write_send`, migration 23) keeps
//! the ask until the draft exists. [`send_written`] then makes the send.

use std::collections::BTreeSet;

use iris_store::{format_id, Kind, Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    add_action_in, approve_in, create_in, decide_in, hash, submit_in, transaction, ActionId,
    Author, AuthorKind, By, Error, Result, State, Verb,
};

/// The reason of each changeset of the write view.
pub const WRITTEN: &str = "Written in the write view.";

/// The reason of a send that goes to review because its warnings are not
/// the warnings that the person saw at the press.
pub const NEW_WARNINGS: &str = "The warnings of the mail changed after the press of Send.";

fn person() -> Author {
    Author {
        kind: AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    }
}

/// A new mail from an account (`a-N`) or a reply in a thread (`t-N`) to
/// the message `parent` (a row ID) that the person saw.
#[derive(Clone, Copy)]
pub enum Mail<'a> {
    New { account: &'a str },
    Reply { thread: &'a str, parent: i64 },
}

/// One press of Send (write spec, section 2).
pub struct Press<'a> {
    pub mail: Mail<'a>,
    /// The args of the `draft` action: `to`, `cc`, `body`, and `subject`
    /// for a new mail.
    pub args: &'a Map<String, Value>,
    /// The key of the press, the same for each try of one form content.
    pub key: &'a str,
    /// The warnings (flags) that the person saw at the press.
    pub flags: &'a [String],
}

/// What [`write_mail`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Written {
    /// A new write, with its changeset row ID.
    New(i64),
    /// An earlier try of the same press made this changeset.
    Again(i64),
}

/// Why [`write_mail`] made nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The submit denied the `draft` action, with its reason.
    Denied(String),
    /// The parent that the submit resolved is not the parent that the
    /// person saw.
    NewParent,
    /// The warnings of the mail are not the warnings that the person saw.
    /// The person looks at these, then presses again.
    Flags(Vec<String>),
    /// An earlier press with this key was another mail.
    KeyTaken,
}

/// The hash of a press: the target and the args, as canonical JSON
/// (`serde_json` keeps the keys of a map in order).
fn press_hash(p: &Press<'_>) -> String {
    let target = match p.mail {
        Mail::New { account } => Value::from(account),
        Mail::Reply { thread, parent } => serde_json::json!([thread, parent]),
    };
    let text = serde_json::json!({ "target": target, "args": p.args }).to_string();
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The write of the person (write spec, section 2), in one transaction:
/// the changeset of the author `person`, the `draft` action, its submit,
/// the approval of the person and the send mark. A refusal leaves nothing.
pub fn write_mail(
    store: &Store,
    press: &Press<'_>,
    now_ms: i64,
) -> Result<std::result::Result<Written, Refusal>> {
    let refusal = std::cell::Cell::new(None);
    let refuse = |why: Refusal| {
        refusal.set(Some(why));
        Error::Refused(String::new())
    };
    let request = press_hash(press);
    let r = transaction(store, |c| {
        let earlier: Option<(i64, String)> = c
            .query_row(
                "SELECT a.changeset_id, w.request_hash FROM write_send w
                 JOIN changeset_action a ON a.id = w.action_id WHERE w.request_key = ?1",
                [press.key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((cs, hash)) = earlier {
            return if hash == request {
                Ok(Written::Again(cs))
            } else {
                Err(refuse(Refusal::KeyTaken))
            };
        }
        let author = person();
        let cs = create_in(c, &author, now_ms)?;
        let target = match press.mail {
            Mail::New { account } => account,
            Mail::Reply { thread, .. } => thread,
        };
        let n = add_action_in(c, cs, &author, Verb::Draft, &[target], press.args, None)?;
        submit_in(c, cs, &author, WRITTEN, now_ms)?;
        let (id, revision, state, reason, flags): (
            i64,
            i64,
            Option<String>,
            Option<String>,
            String,
        ) = c.query_row(
            "SELECT id, revision, state, reason, coalesce(flags, '[]') FROM changeset_action
                 WHERE changeset_id = ?1 AND n = ?2",
            params![cs, n],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        if state.as_deref() != Some("proposed") {
            return Err(refuse(Refusal::Denied(
                reason.unwrap_or_else(|| "The submit did not take the mail.".into()),
            )));
        }
        if let Mail::Reply { parent, .. } = press.mail {
            let resolved: Option<i64> = c
                .query_row(
                    "SELECT message_id FROM action_message WHERE action_id = ?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?;
            if resolved != Some(parent) {
                return Err(refuse(Refusal::NewParent));
            }
        }
        let flags: Vec<String> = serde_json::from_str(&flags).unwrap_or_default();
        let seen: BTreeSet<&str> = press.flags.iter().map(String::as_str).collect();
        if flags.iter().map(String::as_str).collect::<BTreeSet<_>>() != seen {
            return Err(refuse(Refusal::Flags(flags)));
        }
        let (_, h) = hash(c, id)?;
        approve_in(
            c,
            ActionId {
                changeset: cs,
                revision,
                n,
            },
            &h,
            now_ms,
        )?;
        c.execute(
            "INSERT INTO write_send(action_id, request_key, request_hash, flags, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                press.key,
                request,
                serde_json::to_string(&flags).expect("strings"),
                now_ms
            ],
        )?;
        Ok(Written::New(cs))
    });
    match (r, refusal.into_inner()) {
        (Err(Error::Refused(_)), Some(why)) => Ok(Err(why)),
        (r, _) => r.map(Ok),
    }
}

/// True if a send mark was made after `since_ms` (a time of the clock of
/// the engine). The sync thread runs the apply step early for a new mark
/// (write spec, section 7).
pub fn written_since(store: &Store, since_ms: i64) -> Result<bool> {
    Ok(store.read(|c| {
        Ok(c.query_row(
            "SELECT EXISTS (SELECT 1 FROM write_send WHERE created_at > ?1)",
            [since_ms],
            |r| r.get(0),
        )?)
    })?)
}

/// Step 2a of the apply step (write spec, section 3): the send of each
/// marked draft of `account`, oldest mark first, each in one transaction.
/// Gives the count of marks that got a send action.
pub fn send_written(store: &Store, account: i64, now_ms: i64) -> Result<usize> {
    let marks: Vec<i64> = store.read(|c| {
        let mut st = c.prepare(&format!(
            "SELECT w.action_id {OPEN} AND d.account_id = ?1 ORDER BY w.created_at, w.action_id"
        ))?;
        let rows = st.query_map([account], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })?;
    let mut made = 0;
    for mark in marks {
        if transaction(store, |c| send_one(c, mark, now_ms))? {
            made += 1;
        }
    }
    Ok(made)
}

/// The marks that wait for their send: no send yet, a draft row that
/// Gmail did not send or change, and a changeset that the person did not
/// withdraw. A mark that fails one of these never gets a send.
const OPEN: &str = "FROM write_send w
    JOIN draft d ON d.action_id = w.action_id
    JOIN changeset_action a ON a.id = w.action_id
    JOIN changeset c ON c.id = a.changeset_id
    WHERE w.send_action_id IS NULL AND c.state <> 'withdrawn'
      AND d.foreign_at IS NULL AND d.sent_at IS NULL";

/// One mark: read it again in the write transaction, so a second run
/// finds the send and makes nothing.
fn send_one(c: &Connection, mark: i64, now_ms: i64) -> Result<bool> {
    let row: Option<(i64, String)> = c
        .query_row(
            &format!("SELECT d.id, w.flags {OPEN} AND w.action_id = ?1"),
            [mark],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((draft, seen)) = row else {
        return Ok(false);
    };
    let author = person();
    let cs = create_in(c, &author, now_ms)?;
    let target = format_id(Kind::Draft, draft);
    let n = add_action_in(c, cs, &author, Verb::Send, &[&target], &Map::new(), None)?;
    submit_in(c, cs, &author, WRITTEN, now_ms)?;
    let (id, revision, state, flags): (i64, i64, Option<String>, String) = c.query_row(
        "SELECT id, revision, state, coalesce(flags, '[]') FROM changeset_action
         WHERE changeset_id = ?1 AND n = ?2",
        params![cs, n],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    // A denied send stays, with its reason, for the state of the write.
    if state.as_deref() == Some("proposed") {
        let action = ActionId {
            changeset: cs,
            revision,
            n,
        };
        let seen: BTreeSet<String> = serde_json::from_str(&seen).unwrap_or_default();
        let flags: BTreeSet<String> = serde_json::from_str(&flags).unwrap_or_default();
        if flags.is_subset(&seen) {
            let (_, h) = hash(c, id)?;
            approve_in(c, action, &h, now_ms)?;
        } else {
            // A warning that the person did not see: the review decides.
            decide_in(
                c,
                action,
                State::InReview,
                By::Engine,
                Some(NEW_WARNINGS),
                now_ms,
            )?;
        }
    }
    c.execute(
        "UPDATE write_send SET send_action_id = ?2 WHERE action_id = ?1 AND send_action_id IS NULL",
        params![mark, id],
    )?;
    Ok(true)
}
