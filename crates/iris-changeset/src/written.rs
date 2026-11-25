//! The write view of the person (docs/specs/2026-11-17-write.md): one press
//! of Send approves a `draft` action and asks for the send of the draft
//! that the action makes. The send mark (`write_send`, migration 28) keeps
//! the ask until the draft exists. [`send_written`] then makes the send.

use std::collections::BTreeSet;

use iris_store::{format_id, parse_id, Kind, Store};
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

/// The life of a send mark: 24 hours after the press. The approval of the
/// person belongs to the press, so a mark that got no send in this time
/// never gets one (write spec, section 3, step 2).
pub const MARK_MS: i64 = 24 * 60 * 60 * 1000;

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
    /// The draft (`d-N`) that the press changes (change spec, section 5).
    /// It must be a draft of the account or the thread of `mail`. A draft
    /// of an account that is not Gmail is ignored: the press makes a new
    /// draft.
    pub draft: Option<&'a str>,
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
    // Only with a draft: the hash of each other press stays as before.
    let text = match p.draft {
        None => serde_json::json!({ "target": target, "args": p.args }),
        Some(d) => serde_json::json!({ "target": target, "args": p.args, "draft": d }),
    }
    .to_string();
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
        let mut target = match press.mail {
            Mail::New { account } => account,
            Mail::Reply { thread, .. } => thread,
        };
        if let Some(d) = press.draft {
            if change_target(c, d, &press.mail).map_err(|why| refuse(Refusal::Denied(why)))? {
                target = d;
            }
        }
        let cs = create_in(c, &author, now_ms)?;
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

/// True if the press changes the draft `draft` (a Gmail draft), false if
/// it makes a new draft (another provider). Refuses a draft that does not
/// exist, that is of an account that Iris removes, or that is not of the
/// account (a new mail) or the thread (a reply) of `mail`, and a reply
/// draft that does not answer the parent of `mail`.
fn change_target(
    c: &Connection,
    draft: &str,
    mail: &Mail<'_>,
) -> std::result::Result<bool, String> {
    let gone = || "The draft does not exist.".to_string();
    let Some((Kind::Draft, row)) = parse_id(draft) else {
        return Err(gone());
    };
    let found: Option<(i64, Option<i64>, String, String)> = c
        .query_row(
            "SELECT d.account_id, d.thread_id, a.provider, d.content FROM draft d
             JOIN account a ON a.id = d.account_id
             WHERE d.id = ?1 AND (a.state IS NULL OR a.state NOT IN ('removing', 'removed'))",
            [row],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((account, thread, provider, content)) = found else {
        return Err(gone());
    };
    let same = match (mail, thread) {
        (Mail::New { account: a }, None) => *a == format_id(Kind::Account, account),
        (Mail::Reply { thread: t, .. }, Some(th)) => *t == format_id(Kind::Thread, th),
        _ => false,
    };
    if !same {
        return Err("The draft is not of this account or thread.".into());
    }
    // A reply: the parent of the press is the message that the draft
    // answers (draft change spec, section 5).
    if let Mail::Reply { parent, .. } = mail {
        let bare = |v: &str| v.trim().trim_matches(['<', '>']).to_ascii_lowercase();
        let answers = serde_json::from_str::<Value>(&content)
            .ok()
            .and_then(|v| v["reply"]["in_reply_to"].as_str().map(bare));
        let header: Option<String> = c
            .query_row(
                "SELECT message_id_hdr FROM header WHERE message_id = ?1",
                [parent],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .flatten();
        if answers.is_none() || answers != header.as_deref().map(bare) {
            return Err("The draft does not answer this message.".into());
        }
    }
    Ok(provider == "gmail")
}

/// The newest send mark: its action row ID, or 0 if there is none. A mark
/// and its action are made in one write transaction, and the store writes
/// one transaction at a time, so a mark that commits later has a larger
/// ID. The ID does not depend on the clock.
pub fn last_mark(store: &Store) -> Result<i64> {
    Ok(store.read(|c| {
        Ok(c.query_row(
            "SELECT coalesce(max(action_id), 0) FROM write_send",
            [],
            |r| r.get(0),
        )?)
    })?)
}

/// True if a send mark is newer than `mark` (a value of [`last_mark`]).
/// The sync thread reads [`last_mark`] before each apply step, and runs
/// the apply step early for a newer mark (write spec, section 7).
pub fn written_since(store: &Store, mark: i64) -> Result<bool> {
    Ok(store.read(|c| {
        Ok(c.query_row(
            "SELECT EXISTS (SELECT 1 FROM write_send WHERE action_id > ?1)",
            [mark],
            |r| r.get(0),
        )?)
    })?)
}

/// Step 2a of the apply step (write spec, section 3): the send of each
/// marked draft of `account`, oldest mark first, each in one transaction.
/// Gives the count of marks that got a send action.
///
/// `clock` gives the time now. Each send reads it in its own transaction:
/// the age check of the mark and the approval of the send use that time,
/// so the hold of 60 seconds starts when the send exists, also after slow
/// Gmail calls earlier in the step.
pub fn send_written(store: &Store, account: i64, clock: &dyn Fn() -> i64) -> Result<usize> {
    let now_ms = clock();
    let marks: Vec<i64> = store.read(|c| {
        let mut st = c.prepare(&format!(
            "SELECT w.action_id {OPEN} AND d.account_id = ?1 ORDER BY w.created_at, w.action_id"
        ))?;
        let rows = st.query_map([account, now_ms - MARK_MS], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })?;
    let mut made = 0;
    for mark in marks {
        if transaction(store, |c| send_one(c, mark, clock()))? {
            made += 1;
        }
    }
    Ok(made)
}

/// The draft of the `draft` action `a`, as a join condition on `draft d`
/// (change spec, section 2): the row that the action made (a create), or
/// the row that its `drafts` target names once the action is `applied` (a
/// change). Each query that joins a draft to a send mark uses it.
#[macro_export]
macro_rules! draft_of_action {
    () => {
        "(d.action_id = a.id OR (a.verb = 'draft' AND a.state = 'applied' AND d.id IN
           (SELECT ad.draft_id FROM action_draft ad WHERE ad.action_id = a.id)))"
    };
}

/// The marks that wait for their send: no send yet, a draft row that
/// Gmail did not send or change, a changeset that the person did not
/// withdraw, and a press after `?2` (the time [`MARK_MS`] ago). A mark
/// that fails one of these never gets a send.
const OPEN: &str = concat!(
    "FROM write_send w
    JOIN changeset_action a ON a.id = w.action_id
    JOIN draft d ON ",
    draft_of_action!(),
    "
    JOIN changeset c ON c.id = a.changeset_id
    WHERE w.send_action_id IS NULL AND c.state <> 'withdrawn'
      AND d.foreign_at IS NULL AND d.sent_at IS NULL AND w.created_at > ?2"
);

/// The reason of a send mark whose draft has other content than the press
/// (change spec, section 2): the mark makes no send of it.
pub const DRAFT_CHANGED: &str =
    "The draft changed after you pressed Send. Look at it, then send again.";

/// One mark: read it again in the write transaction, so a second run
/// finds the send and makes nothing.
fn send_one(c: &Connection, mark: i64, now_ms: i64) -> Result<bool> {
    // `pressed`: the content hash of the press, from the ledger entry of
    // the action of the mark (a create or a change).
    let row: Option<(i64, String, bool)> = c
        .query_row(
            &format!(
                "SELECT d.id, w.flags,
                        d.hash IS (SELECT json_extract(l.ops, '$.hash') FROM ledger l
                                   WHERE l.action_id = w.action_id AND l.state = 'applied')
                 {OPEN} AND w.action_id = ?1"
            ),
            [mark, now_ms - MARK_MS],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((draft, seen, pressed)) = row else {
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
    if state.as_deref() == Some("proposed") && !pressed {
        // The draft has other content than the press: a stale mark never
        // sends the content of another press.
        let action = ActionId {
            changeset: cs,
            revision,
            n,
        };
        decide_in(
            c,
            action,
            State::Denied,
            By::Engine,
            Some(DRAFT_CHANGED),
            now_ms,
        )?;
    } else if state.as_deref() == Some("proposed") {
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
