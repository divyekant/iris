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
    /// Send content spec, section 6.1: can be absent.
    #[serde(default)]
    pub bcc: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<iris_compose::Attachment>,
    pub subject: Option<String>,
    pub body: String,
    pub key: String,
    pub flags: Vec<String>,
    /// The draft (`d-N`) that the press changes (draft change spec,
    /// section 5). Its account and thread must be `account` or `thread`.
    #[serde(default)]
    pub draft: Option<String>,
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
    // Only a field that is not empty: the args (and the request hash) of a
    // mail with no Bcc and no file stay as before.
    if !b.bcc.is_empty() {
        args.insert("bcc".into(), json!(b.bcc));
    }
    if !b.attachments.is_empty() {
        args.insert("attachments".into(), json!(b.attachments));
    }
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
        draft: b.draft.as_deref(),
    };
    let cs = iris_changeset::format_changeset_id;
    // A refusal of the store is a refusal of the input of the press (for
    // example a target that is not an ID of its kind): 400, nothing written.
    let written = iris_changeset::write_mail(store, &press, now_ms).map_err(|e| match e {
        iris_changeset::Error::Refused(why) => Error::Query(why),
        e => changeset_error(e),
    })?;
    match written {
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
            concat!(
                "SELECT a.state, coalesce(a.reason, l.error), w.send_action_id, d.id,
                    d.foreign_at IS NOT NULL, d.sent_at IS NOT NULL, c.state, w.created_at
             FROM write_send w
             JOIN changeset_action a ON a.id = w.action_id
             JOIN changeset c ON c.id = a.changeset_id
             LEFT JOIN draft d ON ",
                iris_changeset::draft_of_action!(),
                "
             LEFT JOIN ledger l ON l.action_id = a.id
             WHERE a.changeset_id = ?1"
            ),
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
    // A send with Bcc waits for the gate of its provider, also after the hold.
    let bcc_check = match draft {
        Some(d) => iris_apply::bcc_waits(c, d)?,
        None => None,
    };
    let change = match draft {
        Some(d) => change(c, d, now_ms)?,
        None => None,
    };
    let draft = draft.map(|d| format_id(Kind::Draft, d));
    // A stopped write with a draft says how the next press of Send writes
    // it (draft change spec, section 5).
    let stopped = |why: &str| match &draft {
        Some(_) => json!({"state": "stopped", "reason": why, "draft": draft, "change": change}),
        None => json!({"state": "stopped", "reason": why, "draft": draft}),
    };
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
                "bcc_check": bcc_check,
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

/// How the next press of Send writes the draft `draft` of a stopped write
/// (draft change spec, section 5): `update` changes it in place, `new`
/// makes a new draft (an account that is not Gmail), `None` offers no
/// "Change and send".
fn change(c: &Connection, draft: i64, now_ms: i64) -> rusqlite::Result<Option<&'static str>> {
    let row: Option<(String, bool, bool, Option<i64>, String)> = c
        .query_row(
            "SELECT a.provider, d.sent_at IS NOT NULL,
                    coalesce(a.state IN ('removing', 'removed'), 0), d.thread_id, d.content
             FROM draft d JOIN account a ON a.id = d.account_id WHERE d.id = ?1",
            [draft],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((provider, sent, removed, thread, content)) = row else {
        return Ok(None);
    };
    if provider != "gmail" {
        return Ok((!sent && !removed).then_some("new"));
    }
    let denied = iris_changeset::change_denied(c, 0, draft, now_ms).map_err(|e| match e {
        iris_changeset::Error::Store(iris_store::Error::Db(e)) => e,
        e => rusqlite::Error::ToSqlConversionFailure(e.to_string().into()),
    })?;
    if denied.is_some() {
        return Ok(None);
    }
    // A reply: the newest eligible message of the thread is still the
    // message that the draft answers.
    if let Some(thread) = thread {
        let answers: Option<String> = serde_json::from_str::<Value>(&content)
            .ok()
            .and_then(|v| v["reply"]["in_reply_to"].as_str().map(bare));
        let newest: Option<String> = match iris_changeset::reply_parent(c, thread)? {
            Some(m) => c
                .query_row(
                    "SELECT message_id_hdr FROM header WHERE message_id = ?1",
                    [m],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten()
                .map(|v| bare(&v)),
            None => None,
        };
        if answers.is_none() || answers != newest {
            return Ok(None);
        }
    }
    Ok(Some("update"))
}

/// The content of the draft `d-N` for the form of a change (draft change
/// spec, section 5): `to`, `cc`, `bcc`, `subject`, `body`, `attachments`,
/// and the account and the thread. `None` for a draft that does not exist,
/// of an account that Iris removes, or with content that Iris does not
/// write.
pub fn draft_content(store: &Store, id: &str) -> Result<Option<Value>> {
    let Some((Kind::Draft, row)) = parse_id(id) else {
        return Ok(None);
    };
    let found: Option<(i64, Option<i64>, String)> = store.read(|c| {
        Ok(c.query_row(
            "SELECT d.account_id, d.thread_id, d.content FROM draft d
             JOIN account a ON a.id = d.account_id
             WHERE d.id = ?1 AND d.foreign_at IS NULL AND (a.state IS NULL OR a.state NOT IN ('removing', 'removed'))",
            [row],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
    })?;
    let Some((account, thread, content)) = found else {
        return Ok(None);
    };
    let Some(m) = serde_json::from_str::<Value>(&content)
        .ok()
        .and_then(|v| serde_json::from_value::<iris_compose::Content>(v["content"].clone()).ok())
    else {
        return Ok(None);
    };
    Ok(Some(json!({
        "draft": format_id(Kind::Draft, row),
        "account": format_id(Kind::Account, account),
        "thread": thread.map(|t| format_id(Kind::Thread, t)),
        "to": m.to,
        "cc": m.cc,
        "bcc": m.bcc,
        "subject": m.subject,
        "body": m.body,
        "attachments": m.attachments,
    })))
}

/// A Message-ID with no angle brackets, in lower case.
fn bare(v: &str) -> String {
    v.trim().trim_matches(['<', '>']).to_ascii_lowercase()
}

/// The most characters of the parent text in the reply form.
pub const PARENT_TEXT: usize = 600;

/// The most mails of the conversation above the reply form (layout C): the
/// newest ones, up to and with the parent. The start values give the count
/// of the older mails.
pub const CONVERSATION: usize = 20;

/// The most characters of the text of one mail of the conversation. The
/// mail view shows the full text.
pub const MAIL_TEXT: usize = 4000;

/// The first `n` characters of `t`, with `…` if the text is longer.
fn cut(t: &str, n: usize) -> String {
    match t.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", &t[..i]),
        None => t.to_string(),
    }
}

/// The mails of the thread that a reply can answer (the filter of
/// `iris_changeset::reply_parent`), newest first.
const ANSWERABLE: &str = concat!(
    "FROM message m WHERE m.thread_id = ?1 AND m.gone_at IS NULL
   AND NOT EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
     WHERE ml.message_id = m.id AND l.provider_id IN ('SPAM', 'TRASH'))
   AND NOT ",
    iris_store::draft_or_copy!("m")
);

/// The conversation above the reply form: the newest [`CONVERSATION`]
/// answerable mails of the thread, oldest first, and the count of the older
/// ones. The last mail is the parent. Mail text is data: the client shows it
/// as text.
fn conversation(c: &Connection, thread: i64) -> rusqlite::Result<(Vec<Value>, i64)> {
    let total: i64 = c.query_row(&format!("SELECT count(*) {ANSWERABLE}"), [thread], |r| {
        r.get(0)
    })?;
    let mut st = c.prepare(&format!(
        "SELECT m.id, EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                              WHERE ml.message_id = m.id AND l.provider_id = 'SENT'),
                {}
         {ANSWERABLE} ORDER BY m.internal_date DESC, m.id DESC LIMIT ?2",
        iris_store::may_be_hostile!()
    ))?;
    let rows = st
        .query_map(rusqlite::params![thread, CONVERSATION as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, bool>(1)?,
                r.get::<_, bool>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::with_capacity(rows.len());
    for (row, you, marked) in rows.into_iter().rev() {
        let Some(m) = message_in(c, row)? else {
            continue;
        };
        let text = m.text.as_deref();
        out.push(json!({
            "id": m.summary.id,
            "from": m.summary.from,
            "date": m.summary.date,
            "to": m.to,
            // The label `SENT` says that the person wrote the mail (section 5).
            "you": you,
            // A hostile mark, a link check that waits, or a hostile sender.
            "hostile": marked || m.sender_kind == "hostile",
            "text": text.map(|t| cut(t, MAIL_TEXT)),
            "cut": text.is_some_and(|t| t.chars().nth(MAIL_TEXT).is_some()),
        }));
    }
    let earlier = total - out.len() as i64;
    Ok((out, earlier.max(0)))
}

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
    // `sent`: the label `SENT` is the evidence that the person wrote the
    // mail. From is not: anyone can put the address of the person there.
    let (account, subject, to, sent): (i64, Option<String>, Option<String>, bool) = c
        .query_row(
            "SELECT m.account_id, h.subject, h.to_addrs,
                    EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                            WHERE ml.message_id = m.id AND l.provider_id = 'SENT')
             FROM message m LEFT JOIN header h ON h.message_id = m.id WHERE m.id = ?1",
            [parent],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(iris_store::Error::from)?;
    let m = message_in(c, parent)
        .map_err(iris_store::Error::from)?
        .expect("reply_parent gives a message");
    // The person wrote the newest mail: the reply goes to its recipients.
    let to: Vec<String> = if sent {
        let mut out = vec![];
        iris_senders::head::each_address(to.as_deref().unwrap_or(""), |a, _| out.push(a));
        out
    } else {
        vec![m.summary.from.address.clone().unwrap_or_default()]
    };
    let to: Vec<String> = to.into_iter().filter(|a| valid_address(a)).collect();
    let text = m.text.as_deref().map(|t| cut(t, PARENT_TEXT));
    let (mails, earlier) = conversation(c, thread).map_err(iris_store::Error::from)?;
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
        "topic": subject.unwrap_or_default(),
        "conversation": mails,
        "earlier": earlier,
    })))
}

/// True if the address passes the address check of a draft (R3-01).
fn valid_address(a: &str) -> bool {
    !a.is_empty()
        && iris_compose::Content {
            to: vec![a.to_string()],
            cc: vec![],
            subject: "s".into(),
            body: "b".into(),
            ..Default::default()
        }
        .check()
        .is_ok()
}

/// A draft of the reply to the thread `t-N` in the voice of the person, from
/// `intent` (R3-04 intent path). One model call through the budget of
/// understanding (`iris_voice::draft`). `None` if the thread does not exist.
/// The person reads and edits the text before Send: the draft sends nothing.
pub fn draft(
    store: &Store,
    connect: &crate::Connect,
    thread: &str,
    parent: &str,
    intent: &str,
    to: &[String],
    now_ms: fn() -> i64,
) -> Result<Option<Value>> {
    use iris_voice::{Error as V, Failure as F};
    let Some((Kind::Thread, row)) = parse_id(thread) else {
        return Ok(None);
    };
    let (exists, now_parent): (bool, Option<i64>) = store.read(|c| {
        let exists = c.query_row(
            "SELECT EXISTS (SELECT 1 FROM thread WHERE id = ?1)",
            [row],
            |r| r.get(0),
        )?;
        Ok((exists, iris_changeset::reply_parent(c, row)?))
    })?;
    if !exists {
        return Ok(None);
    }
    // The person of the reply is the first recipient of the form (issue 535).
    if let Some(a) = to.iter().find(|a| !valid_address(a)) {
        return Err(Error::Query(format!("{a} is not a valid address.")));
    }
    // The draft answers the mail that the form shows. A newer parent costs
    // no call: the person reads the new mail first. The draft gives
    // `based_on`, so the client can check it again after the call.
    if now_parent.map(|m| format_id(Kind::Message, m)).as_deref() != Some(parent) {
        return Err(Error::Conflict(
            "A new mail came into this thread. Read it, then ask again.".into(),
        ));
    }
    let form = iris_voice::Form {
        intent: (!intent.trim().is_empty()).then_some(intent),
        // The voice crate checks the parent again against the thread that it
        // reads: a mail that arrives after the check above saves nothing (issue 576).
        parent: now_parent,
        to: to.first().map(String::as_str),
    };
    let d = iris_voice::draft(store, row, form, &|r| connect(r), now_ms(), &now_ms).map_err(
        |e| match e {
            F::Busy => Error::Unavailable(
                "Iris reads mail with the model now. Try again in a minute.".into(),
            ),
            F::NoModel => {
                Error::Unavailable("No model is set. Set one with `iris model set`.".into())
            }
            F::Connect(why) => Error::Unavailable(why),
            F::Budget(_) => Error::Unavailable(
                "The model budget is used. Approve a new budget with `iris understand --estimate`, then `--approve`.".into(),
            ),
            F::Draft(V::NoThread(_)) | F::Draft(V::NotReady) => Error::Conflict(
                "Iris did not read each mail of this thread yet. Try again in a few minutes.".into(),
            ),
            F::Draft(V::Hostile) => Error::Conflict(
                "This thread has hostile mail. Iris writes no draft for it.".into(),
            ),
            F::Draft(V::LongIntent) => Error::Query(format!(
                "Write the intent in {} characters or fewer.",
                iris_voice::MAX_INTENT
            )),
            F::Draft(V::Changed) => Error::Conflict(
                "A new mail came into this thread while Iris wrote. Try again.".into(),
            ),
            F::Draft(V::BadAnswer(why)) => Error::Model(format!("The model gave no valid draft: {why}.")),
            F::Draft(V::Model(e)) => Error::Model(e.to_string()),
            F::Draft(V::Store(e)) | F::Store(e) => Error::Store(e),
        },
    )?;
    Ok(Some(json!({
        "body": d.saved.body,
        "based_on": format_id(Kind::Message, d.saved.based_on),
        "samples": d.saved.samples,
        "unknown_cost": d.unknown_cost,
        "overrun": d.overrun,
    })))
}
