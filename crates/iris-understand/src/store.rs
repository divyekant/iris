//! Understanding of one message of the store (specification 1, section 7.1).
//!
//! Three writes:
//! 1. Steps 1 to 3: the header row, the clean text, the sender kind
//!    (`iris_senders::decide`) and the hostile marks, after a check that the
//!    message did not change since it was read. It also sets
//!    `understood_at` to NULL.
//! 2. No write while the model works, so the model never holds the write lock.
//! 3. Step 8: the items, their sources, `understood_at` and the
//!    conversation of the thread (`iris_talk::update`), in one transaction. A crash before the commit leaves the message not
//!    understood, and the next run starts again from step 3.

use std::fmt;

use iris_store::{Body, Store};
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::Value;

use crate::{analyze_cut, decide, validate, Analysis, Context, Ids, Kind, Mark, Model, Usage};

/// The wait after a failed try of the model (section 7.4).
pub const RETRY_MS: i64 = 86_400_000;
/// After this many failed tries, the model gets the message no more.
pub const MAX_TRIES: i64 = 3;

#[derive(Debug)]
pub enum Error {
    Store(iris_store::Error),
    /// The model call failed. The message stays not understood, with no
    /// added try. If `is_retryable()`, try again later. The `Vec` holds the
    /// sender rows of [`Outcome::rerun`]: write 1 committed their change.
    /// [`Seen`] is what the call read: the state of the message, the
    /// sender kind and the other inputs.
    Model(iris_model::Error, Vec<i64>, Box<Seen>),
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
            Error::Model(e, _, _) => write!(f, "model: {e}"),
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
    /// A draft or a sent copy (`iris_store::DRAFT_OR_COPY`): no item.
    pub draft: bool,
    /// The message is gone: nothing changed.
    pub gone: bool,
    /// Two answers were not valid: `tries` went up, `retry_at` is set.
    pub failed: bool,
    /// No fixed rule matched, and the message has 3 failed tries or a later
    /// `retry_at`: no model call, and step 8 wrote nothing.
    pub waiting: bool,
    /// The sender is a stranger that the person declined at the gate
    /// (decision D29): no model call. The message is understood with no
    /// items, and its row in `model_declined` makes it pending again when
    /// the decline stops applying (store migration 53).
    pub declined: bool,
    /// The message changed in the store while the model worked (content,
    /// labels, sender kind or gone). Step 8 wrote nothing, and the message
    /// stays not understood for the next run.
    pub changed: bool,
    /// The `sender` rows whose kind became `hostile` or stopped being
    /// `hostile` (`iris_senders::Decision::rerun`). The caller runs
    /// understanding again on each message from these rows.
    pub rerun: Vec<i64>,
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
    /// The authserv-id that the account provider adds (`trusted_authserv`).
    authserv: Option<&'static str>,
    labels: Vec<String>,
    tries: i64,
    retry_at: Option<i64>,
    phishing: bool,
    /// The hostile marks that the person released for the current content
    /// (`message.released`, migration 62).
    released: Vec<String>,
}

/// Rows of `link_check` in one read of [`recheck_links`].
pub const LINK_CHECKS: i64 = 200;

/// The link check of each message in `link_check` (migration 20): new
/// content, and mail that the engine understood before the mark `link`. It
/// reads each row, in pages of [`LINK_CHECKS`], with no model. A message
/// with a deceptive link is understood at once: the mark makes it hostile,
/// so the run needs no model, and its items go. A row goes only when the
/// store has the result of the check for the content that it read: no
/// deceptive link, or the mark `link`. Other rows stay for the next run (a
/// file that Iris cannot read, a change during the check). Returns the
/// count of messages that got the mark. With `max_marks`, it stops before
/// one more message would get the mark: the run limit counts them.
pub fn recheck_links(store: &Store, now_ms: i64, max_marks: Option<u64>) -> Result<u64> {
    let mut marked = 0;
    let mut after = 0;
    loop {
        let rows: Vec<(i64, i64)> = store.read(|c| {
            let mut st = c.prepare(
                "SELECT id, message_id FROM link_check WHERE id > ?1 ORDER BY id LIMIT ?2",
            )?;
            let rows = st
                .query_map([after, LINK_CHECKS], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(rows)
        })?;
        let Some(&(last, _)) = rows.last() else {
            return Ok(marked);
        };
        after = last;
        for (row, id) in rows {
            // The row ID first: a trigger that queues the message again
            // after this read gives it a new row, which this check keeps.
            let Some((mark, sha)) = link_mark_of(store, id) else {
                continue;
            };
            if mark && max_marks.is_some_and(|n| marked >= n) {
                // The message limit of the run: the rest waits.
                return Ok(marked);
            }
            if mark {
                // Hostile mail never reaches the model (section 6.7). An
                // error leaves the row for the next run.
                let _ = understand(store, id, &crate::budget::NoModel, now_ms);
            }
            // The row goes only for the state that the check read (the same
            // row, the same content), and, for a deceptive link, only when
            // the message has the mark.
            let done = store.write(|tx| {
                let n = tx.execute(
                    "DELETE FROM link_check WHERE id = ?1 AND EXISTS (
                       SELECT 1 FROM message m WHERE m.id = ?2
                         AND (m.content_sha256 IS ?3 OR m.gone_at IS NOT NULL)
                         AND (NOT ?4 OR EXISTS (
                           SELECT 1 FROM json_each(coalesce(m.hostile, '[]'))
                           WHERE value = 'link')))",
                    params![row, id, sha, mark],
                )?;
                if n > 0 && !mark {
                    // A mark of earlier content goes: the check of this
                    // content found no deceptive link.
                    tx.execute(
                        "UPDATE message SET hostile = (
                           SELECT json_group_array(value) FROM json_each(hostile)
                           WHERE value <> 'link')
                         WHERE id = ?1 AND content_sha256 IS ?2 AND EXISTS (
                           SELECT 1 FROM json_each(coalesce(hostile, '[]'))
                           WHERE value = 'link')",
                        params![id, sha],
                    )?;
                }
                Ok(n > 0)
            })?;
            marked += u64::from(mark && done);
        }
    }
}

/// Rows of `fixed_due` in one read of [`recheck_fixed`].
pub const FIXED_CHECKS: i64 = 200;

/// The fixed check of each message in `fixed_due` (store migration 54,
/// decision D31): mail whose sender became or stopped being a `service`, or
/// whose display name is the old or the new name of a known person. It runs
/// the fixed steps again with no model: the hostile marks (the spoof check
/// reads the known names) and the fixed rules (the code-signal rule reads
/// the kind `service`). Only when the result differs from the stored one,
/// it understands the message again with no model:
/// - a new mark makes the message hostile: its items go;
/// - a rule that matches now gives its items;
/// - mail that no fixed rule covers now, and that the model never read (it
///   was hostile, or a rule made its items), waits for the model as new mail
///   does.
///
/// The check never calls the model, and mail whose model items still hold
/// keeps them. A row goes only when the check read the state of now: a
/// change during the check gives the message a new row. Gives the count of
/// messages that it understood again. With `max`, it stops before one more
/// message would be understood again: the run limit counts them, and the
/// rest of the queue waits for the next run.
pub fn recheck_fixed(store: &Store, now_ms: i64, max: Option<u64>) -> Result<u64> {
    let mut done = 0;
    let mut after = 0;
    loop {
        let rows: Vec<(i64, i64)> = store.read(|c| {
            let mut st = c.prepare(
                "SELECT id, message_id FROM fixed_due WHERE id > ?1 ORDER BY id LIMIT ?2",
            )?;
            let rows = st
                .query_map([after, FIXED_CHECKS], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(rows)
        })?;
        let Some(&(last, _)) = rows.last() else {
            return Ok(done);
        };
        after = last;
        for (row, id) in rows {
            let start = store.read(|c| Ok(state(c, id, true)?))?;
            // A file that Iris cannot read now keeps the row for the next run.
            let Ok(differs) = fixed_differs(store, id, now_ms) else {
                continue;
            };
            if differs && max.is_some_and(|n| done >= n) {
                return Ok(done);
            }
            if differs {
                // No model: the run ends with a refusal when the message
                // needs the model. Write 1 has made it pending then.
                match understand(store, id, &crate::budget::NoModel, now_ms) {
                    Ok(_) | Err(Error::Model(..)) => done += 1,
                    Err(_) => continue,
                }
            }
            store.write(|tx| {
                // With no change, the row goes only for the state that the
                // check read; after a new run, for any state.
                let same = differs || state(tx, id, true)? == start;
                if same {
                    tx.execute("DELETE FROM fixed_due WHERE id = ?1", [row])?;
                }
                Ok(())
            })?;
        }
    }
}

/// Steps 1 to 3 for a message read from the store: one rule for
/// [`understand`] and the fixed check. Mail past the read limit counts as
/// cut text, the mark `phishing` stays until a rebuild, and a mark that the
/// person released is left out (section 6.7).
fn analysis(raw: &[u8], raw_cut: bool, html_after: bool, cx: &Context, row: &Row) -> Analysis {
    let mut a = analyze_cut(raw, raw_cut, html_after, cx);
    a.text.cut |= raw_cut;
    a.marks
        .retain(|m| !row.released.iter().any(|r| r == m.name()));
    if row.phishing {
        a.marks.push(Mark::Phishing);
    }
    a
}

/// The record of `message.fixed_rules` that the backfill of migration 58
/// writes when it cannot tell how the last run completed the message.
const UNKNOWN_RULES: &str = "unknown";

/// True when the fixed result of the understood received message `id` now
/// (its hostile marks, or the rules of its rule items) is not the stored
/// one. False for mail that is gone or your own. For mail that is not
/// understood: true when the fixed steps alone complete it now.
fn fixed_differs(store: &Store, id: i64, now_ms: i64) -> Result<bool> {
    let row = read_row(store, id)?;
    let own = row
        .labels
        .iter()
        .any(|l| l == "SENT" || iris_store::DRAFT_OR_COPY.contains(&l.as_str()));
    let understood: bool = store.read(|c| {
        Ok(c.query_row(
            "SELECT understood_at IS NOT NULL FROM message WHERE id = ?1",
            [id],
            |r| r.get(0),
        )?)
    })?;
    if row.gone || own || row.sha.is_none() {
        return Ok(false);
    }
    let path = row
        .sha
        .as_deref()
        .and_then(|s| store.content_path(s))
        .ok_or(Error::NoMessage(id))?;
    let (raw, raw_cut, html_after) = read_raw(&path)?;
    let head = crate::Head::parse(&raw, row.authserv);
    let (now, stored_marks, (stored_rules, unknown), model_items) = store.read(|c| {
        let now = inputs(
            c,
            id,
            row.thread,
            row.internal_date,
            head.from_name.as_deref(),
            head.from_addr.as_deref(),
        )?;
        let kind = match head.from_addr.as_deref() {
            Some(a) => sender_kind_in(c, a)?,
            None => Kind::Stranger,
        };
        let marks: String = c.query_row(
            "SELECT coalesce(hostile, '[]') FROM message WHERE id = ?1",
            [id],
            |r| r.get(0),
        )?;
        // The rules that completed the last understanding (migration 58),
        // also of pinned items; `None` when the model made an item.
        let rules: Option<String> =
            c.query_row("SELECT fixed_rules FROM message WHERE id = ?1", [id], |r| {
                r.get(0)
            })?;
        // `unknown`: the backfill of migration 58 could not tell how the
        // last run completed the message; the check runs it again.
        let unknown = rules.as_deref() == Some(UNKNOWN_RULES);
        let rules: Option<Vec<String>> =
            rules.map(|r| serde_json::from_str(&r).unwrap_or_default());
        let model = rules.is_none();
        let rules = rules.unwrap_or_default();
        Ok(((now, kind), marks, (rules, unknown), model))
    })?;
    let (inputs_now, kind) = now;
    let known = |_: &str| inputs_now.known.clone();
    let cx = Context {
        me: &row.me,
        kind,
        labels: &row.labels,
        authserv: row.authserv,
        internal_date: row.internal_date,
        now_ms,
        known_person: &known,
        existing: &inputs_now.pinned,
        thread: &inputs_now.context,
        same_sender: &inputs_now.same,
        known: &inputs_now.parties,
    };
    let a = analysis(&raw, raw_cut, html_after, &cx, &row);
    if !understood {
        // Pending mail, or mail that a model try limit or a refusal holds:
        // understood again only when the fixed steps alone complete it now.
        return Ok(decide(&a, &cx, None).is_ok_and(|d| d.hostile || !d.waiting));
    }
    if marks_json(&a.marks, false) != stored_marks {
        return Ok(true);
    }
    if unknown {
        return Ok(true);
    }
    // With no model, `decide` makes no call and gives no error.
    let Ok(d) = decide(&a, &cx, None) else {
        return Ok(true);
    };
    if d.hostile {
        return Ok(false);
    }
    let mut stored_rules = stored_rules;
    stored_rules.sort();
    Ok(if d.waiting {
        // The model reads the mail: a rule item does not hold now.
        !stored_rules.is_empty()
    } else {
        let mut now: Vec<String> = d.rules.iter().map(|r| r.to_string()).collect();
        now.sort();
        model_items || now != stored_rules
    })
}

/// The record of `message.fixed_rules` for one decision: `None` when the
/// model made a draft, else the rules that the fixed rules matched
/// ([`crate::Decision::rules`]), also of a draft that a pinned item holds.
fn fixed_rules(d: &crate::Decision) -> Option<String> {
    if d.drafts.iter().any(|x| matches!(x.by, crate::By::Model)) {
        return None;
    }
    let mut rules = d.rules.clone();
    rules.sort();
    Some(serde_json::to_string(&rules).expect("strings serialize"))
}

/// For the received message `id`: true when it has a deceptive link, with
/// the content hash that the check read. False when it has none, is gone,
/// is your own mail or has no content. `None` when Iris cannot read the
/// row or the file now.
fn link_mark_of(store: &Store, id: i64) -> Option<(bool, Option<String>)> {
    let row = read_row(store, id).ok()?;
    let own = row.labels.iter().any(|l| l == "SENT" || l == "DRAFT");
    if row.gone || own || row.sha.is_none() {
        return Some((false, row.sha));
    }
    let path = row.sha.as_deref().and_then(|s| store.content_path(s))?;
    let (raw, raw_cut, html_after) = read_raw(&path).ok()?;
    let c = iris_clean::clean_cut(&raw, raw_cut);
    let head = crate::Head::parse(&raw, row.authserv);
    // A released `link` mark is no mark (section 6.7).
    let mark = crate::link_mark(&head, &c.links, c.links_cut || html_after)
        && !row.released.iter().any(|r| r == "link");
    Some((mark, row.sha))
}

/// Run understanding on message `message` (its row ID): steps 1 to 8. Run
/// it again on the same message to understand it again: the items of the
/// message that a person did not correct are made again, not duplicated.
pub fn understand(store: &Store, message: i64, model: &dyn Model, now_ms: i64) -> Result<Outcome> {
    // The state that this run reads. Write 1 checks it again.
    // The full state, also the marks and `understood_at`: a run that ends
    // between this read and write 1 makes this run save nothing.
    let start = store.read(|c| Ok(state(c, message, true)?))?;
    let row = read_row(store, message)?;
    if row.gone {
        // A gone message leaves the conversation of its thread (section 4.5),
        // if a sync did not restore it after the first read.
        // Sync gives its items the status `gone` when it marks the message
        // gone, and the saved status again when the message returns
        // (`iris-sync`, section 4.5). This step does not change items.
        // Its sender evidence goes too: a sent message can be the only
        // evidence that its recipient is a person (section 6.7).
        let done = store.write(|tx| {
            if state(tx, message, true)? != start {
                return Ok(None);
            }
            // The kinds first: the conversation reads them.
            let rerun = iris_senders::refresh(tx, now_ms)?;
            rerun_senders(tx, &rerun, now_ms)?;
            iris_talk::update(tx, row.thread, None, now_ms)?;
            Ok(Some(rerun))
        })?;
        return Ok(Outcome {
            gone: done.is_some(),
            changed: done.is_none(),
            rerun: done.unwrap_or_default(),
            ..Outcome::default()
        });
    }
    let path = row
        .sha
        .as_deref()
        .and_then(|s| store.content_path(s))
        .ok_or(Error::NoMessage(message))?;
    let (raw, raw_cut, html_after) = read_raw(&path)?;

    // A draft or a copy in a Sent folder is not received mail (IMAP spec section 4).
    let draft = row
        .labels
        .iter()
        .any(|l| iris_store::DRAFT_OR_COPY.contains(&l.as_str()));
    let head = crate::Head::parse(&raw, row.authserv);
    // The spoof check needs this lookup: a read error stops understanding
    // before any model call.
    let (from_name, from_addr) = (head.from_name.clone(), head.from_addr.clone());
    // All store inputs of the run, read with one function. Write 1 and
    // write 3 read them again and compare.
    let inputs_now = |c: &rusqlite::Connection| {
        inputs(
            c,
            message,
            row.thread,
            row.internal_date,
            from_name.as_deref(),
            from_addr.as_deref(),
        )
    };
    let first = store.read(|c| Ok(inputs_now(c)?))?;
    let known = |_: &str| first.known.clone();
    // Steps 1 and 2 do not use the sender kind or the known parties. Write
    // 1 decides them (step 3).
    let cx = Context {
        me: &row.me,
        kind: Kind::Stranger,
        labels: &row.labels,
        authserv: row.authserv,
        internal_date: row.internal_date,
        now_ms,
        known_person: &known,
        existing: &first.pinned,
        thread: &first.context,
        same_sender: &first.same,
        known: &[],
    };
    let a = analysis(&raw, raw_cut, html_after, &cx, &row);
    // A draft is not understood and is not hostile mail (section 6.7).
    let marks = if draft { Vec::new() } else { a.marks.clone() };

    // Write 1: steps 1 to 3.
    // After write 1, `seen` is the full state of the message. Each later
    // write checks it first, so a sync or a second run between the writes
    // makes this run save nothing.
    let sender = match (&a.head.from_name, &a.head.from_addr) {
        (Some(n), Some(ad)) => format!("{n} <{ad}>"),
        (_, Some(ad)) => ad.clone(),
        _ => String::new(),
    };
    let body = Body {
        subject: a.text.subject.clone(),
        sender,
        text: a.text.text.clone(),
    };
    let mut decided = None;
    let mut declined = false;
    let mut gate_read = (false, None);
    let mut after = Inputs::default();
    let seen = store.write(|tx| {
        // Steps 1 and 2 used the state of the message and `first`.
        if state(tx, message, true)? != start || inputs_now(tx)? != first {
            return Ok(None);
        }
        put_header(tx, message, &a)?;
        // Step 3: the sender rules read the header row of this message.
        let d = iris_senders::decide(tx, message, &raw, now_ms)?;
        // In the transaction of the kind change, so no error loses a rerun.
        rerun_senders(tx, &d.rerun, now_ms)?;
        decided = Some(d);
        // After the kind of this message: a decline applies only to a stranger.
        gate_read = gate(tx, from_addr.as_deref())?;
        declined = gate_read.0;
        tx.execute(
            "DELETE FROM model_declined WHERE message_id = ?1",
            [message],
        )?;
        // The clean text goes in the same checked write, so a sync between
        // two writes cannot leave the text of old content.
        iris_store::put_body(tx, message, &body)?;
        // The message is not understood until write 3 commits, so a stop
        // between the writes leaves it for the next run (section 7.1).
        tx.execute(
            "UPDATE message SET hostile = ?2, understood_at = NULL WHERE id = ?1",
            params![message, marks_json(&marks, false)],
        )?;
        // A new run first deletes the items of the last run that are not
        // pinned, and computes the conversation without the message (section
        // 7.1), so a model error after this write leaves no stale item. A
        // draft keeps no item.
        drop_old_items(tx, message)?;
        iris_talk::update(tx, row.thread, None, now_ms)?;
        // After the header row and the sender kinds of this message: the
        // inputs of the model call, which write 3 checks.
        after = inputs_now(tx)?;
        Ok(Some(state(tx, message, true)?))
    })?;
    drop(raw);
    let decided = decided.filter(|_| seen.is_some());
    let rerun = decided
        .as_ref()
        .map(|d| d.rerun.clone())
        .unwrap_or_default();
    let kind_read = decided.map_or(Kind::Stranger, |d| Kind::parse(d.kind.name()));
    let from = a.head.from_addr.clone();
    let sent = kind_read == Kind::SelfOwner;
    let known = |_: &str| after.known.clone();
    let cx = Context {
        kind: kind_read,
        known_person: &known,
        existing: &after.pinned,
        thread: &after.context,
        known: &after.parties,
        ..cx
    };
    let Some(seen) = seen else {
        return Ok(Outcome {
            changed: true,
            ..Outcome::default()
        });
    };
    // What the call reads.
    let read = || Seen {
        message,
        state: seen.clone(),
        kind: kind_read,
        from: from.clone().filter(|_| !sent),
        inputs: after.clone(),
        thread: row.thread,
        date: row.internal_date,
        from_name: from_name.clone(),
        from_addr: from_addr.clone(),
        gate: gate_read,
    };
    // The engine does not understand a draft. It stays not understood, so
    // it runs when it is sent and loses the label DRAFT.
    if draft {
        return Ok(Outcome {
            draft: true,
            rerun,
            ..Outcome::default()
        });
    }
    // One rule for the retry state (section 7.4): the hostile check and the
    // fixed rules always run. Only the model call waits for `retry_at` and
    // stops after MAX_TRIES failed tries.
    let held = row.tries >= MAX_TRIES || row.retry_at.is_some_and(|t| t > now_ms);
    // The mail of a declined sender gets no model call (decision D29). The
    // fixed rules and the hostile check still run.
    let d = decide(&a, &cx, (!held && !declined).then_some(model))
        .map_err(|e| Error::Model(e, rerun.clone(), Box::new(read())))?;
    // A declined sender with no rule that applies: understood, no items.
    let skip = declined && d.waiting;
    if d.waiting && !skip {
        // No rule supports the old items now, and the model cannot run.
        let same = store.write(|tx| {
            let same = state(tx, message, true)? == seen;
            if same {
                drop_old_items(tx, message)?;
                iris_talk::update(tx, row.thread, None, now_ms)?;
            }
            Ok(same)
        })?;
        return Ok(Outcome {
            waiting: same,
            changed: !same,
            rerun,
            ..Outcome::default()
        });
    }
    let ids = Ids {
        message,
        thread: row.thread,
        now_ms,
    };
    let mut out = Outcome {
        hostile: d.hostile,
        declined: skip,
        failed: d.failed,
        calls: d.calls,
        usage: d.usage,
        rerun,
        ..Outcome::default()
    };

    // Write 3: step 8.
    let mut invalid = None;
    let mut changed = false;
    let read = read();
    let written = store.write(|tx| {
        // The model saw `after`: each input must still be the same now, for
        // example with no later SPAM label on an earlier message.
        // A decline, or the end of a decline, after write 1 changes the
        // model step: save nothing, so the next run decides again.
        if !read.same(tx)? {
            changed = true;
            return Ok(Vec::new());
        }
        if d.failed {
            // The old items are not supported by this run: remove them. The
            // message is not understood until a run succeeds.
            drop_old_items(tx, message)?;
            tx.execute(
                "UPDATE message SET tries = tries + 1, retry_at = ?2, understood_at = NULL
                 WHERE id = ?1",
                params![message, now_ms + RETRY_MS],
            )?;
            iris_talk::update(tx, row.thread, None, now_ms)?;
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
        keep_choices(tx, message)?;
        tx.execute(
            "UPDATE message SET understood_at = ?2, retry_at = NULL, fixed_rules = ?3
             WHERE id = ?1",
            params![message, now_ms, fixed_rules(&d)],
        )?;
        if skip {
            tx.execute(
                "INSERT INTO model_declined(message_id) VALUES (?1)",
                [message],
            )?;
        }
        // Steps 4 and 7: the conversation counts this message unless it is
        // hostile mail, also after a phishing answer.
        iris_talk::update(tx, row.thread, Some(message), now_ms)?;
        Ok(items)
    });
    out.items = match (written, invalid) {
        (_, Some(e)) => return Err(Error::Invalid(e)),
        (r, None) => r?,
    };
    if changed {
        return Ok(Outcome {
            changed: true,
            calls: out.calls,
            usage: out.usage,
            rerun: out.rerun,
            ..Outcome::default()
        });
    }
    Ok(out)
}

/// The state of a message that understanding depends on: the content, the
/// gone state, the thread, the date and the labels. With `full`, also the hostile marks and
/// `understood_at`, which a second run of the same message changes. The
/// sender kind is checked apart.
/// What one model call read (after write 1): the state of the message,
/// the sender kind and the other store inputs. Each checked write after the
/// call compares all of it with [`Seen::same`].
#[derive(Debug)]
pub struct Seen {
    message: i64,
    state: Option<String>,
    kind: Kind,
    /// The sender address whose kind the call read; `None` for your own mail.
    from: Option<String>,
    inputs: Inputs,
    thread: i64,
    date: Option<i64>,
    from_name: Option<String>,
    from_addr: Option<String>,
    /// The gate of the sender (decision D29): its decline, and the row ID
    /// and `gate_rev` of its row (migration 58). Each decline or reopen
    /// during the call is a changed input (#934).
    gate: (bool, Option<(i64, i64)>),
}

impl Seen {
    /// True if each thing that the call read is the same now.
    fn same(&self, c: &rusqlite::Connection) -> rusqlite::Result<bool> {
        let kind_now = match &self.from {
            Some(f) => sender_kind_in(c, f)?,
            None => self.kind,
        };
        Ok(state(c, self.message, true)? == self.state
            && kind_now == self.kind
            && gate(c, self.from_addr.as_deref())? == self.gate
            && inputs(
                c,
                self.message,
                self.thread,
                self.date,
                self.from_name.as_deref(),
                self.from_addr.as_deref(),
            )? == self.inputs)
    }
}

/// Mark `message` as waiting for the model with `mark`, only if it is still
/// in the state that the refused call read (issue #153). A write of another
/// writer between the refusal and the mark changed an input: the message
/// then gets no mark, and the next run reads it again.
pub fn mark_waiting(
    tx: &rusqlite::Transaction,
    message: i64,
    seen: &Seen,
    mark: i64,
) -> rusqlite::Result<bool> {
    if seen.message != message || !seen.same(tx)? {
        return Ok(false);
    }
    tx.execute(
        "UPDATE message SET waits_model = ?2 WHERE id = ?1 AND understood_at IS NULL",
        params![message, mark],
    )?;
    Ok(true)
}

fn state(c: &rusqlite::Connection, message: i64, full: bool) -> rusqlite::Result<Option<String>> {
    c.query_row(
        "SELECT coalesce(m.content_sha256, '') || '|' || (m.gone_at IS NOT NULL) || '|' ||
           m.thread_id || '|' || coalesce(m.internal_date, '') || '|' ||
           coalesce((SELECT group_concat(provider_id, ',') FROM (
              SELECT l.provider_id FROM message_label ml JOIN label l ON l.id = ml.label_id
              WHERE ml.message_id = m.id ORDER BY l.provider_id)), '') ||
           CASE WHEN ?2 THEN '|' || coalesce(m.hostile, '') || '|' ||
                coalesce(m.released, '') || '|' ||
                coalesce(m.understood_at, '') || '|' || m.tries || '|' ||
                coalesce(m.retry_at, '') ELSE '' END
         FROM message m WHERE m.id = ?1",
        params![message, full],
        |r| r.get(0),
    )
    .optional()
}

/// The most bytes of one message that understanding reads, so that one
/// message stays inside the memory budget (spec 1, M3). A longer message
/// counts as cut text: the fixed rules defer to the model (section 7.3).
pub const RAW_LIMIT: u64 = 8 * 1024 * 1024;

// shortcut: parts after RAW_LIMIT are lost, for example a text part after a
// large attachment (an HTML part there gives the mark `link`). Read only the parts that iris-clean decodes, with no
// limit on the file, when a real mailbox shows the need.
/// The first `RAW_LIMIT` bytes of the content file, true if the file is
/// longer, and true if the rest can hold an HTML part
/// (`iris_clean::html_part_in`, which reads the first bytes to know if the
/// cut is in a part header). The rest is read in chunks: the memory stays bounded.
fn read_raw(path: &std::path::Path) -> Result<(Vec<u8>, bool, bool)> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut raw = Vec::new();
    (&mut file).take(RAW_LIMIT + 1).read_to_end(&mut raw)?;
    let cut = raw.len() as u64 > RAW_LIMIT;
    let html_after = cut && {
        let limit = RAW_LIMIT as usize;
        iris_clean::html_part_in(&raw[..limit], (&raw[limit..]).chain(file))?
    };
    raw.truncate(RAW_LIMIT as usize);
    Ok((raw, cut, html_after))
}

fn read_row(store: &Store, message: i64) -> Result<Row> {
    let row = store.read(|c| {
        let r = c
            .query_row(
                "SELECT m.account_id, m.thread_id, m.content_sha256, m.internal_date,
                        m.gone_at IS NOT NULL, a.address, m.tries, m.retry_at,
                        EXISTS (SELECT 1 FROM json_each(coalesce(m.hostile, '[]')) WHERE value = 'phishing'),
                        a.provider, m.released
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
                        authserv: crate::head::trusted_authserv(&r.get::<_, String>(9)?),
                        labels: Vec::new(),
                        tries: r.get(6)?,
                        retry_at: r.get(7)?,
                        phishing: r.get(8)?,
                        released: r
                            .get::<_, Option<String>>(10)?
                            .and_then(|j| serde_json::from_str(&j).ok())
                            .unwrap_or_default(),
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

/// The documents of the pinned items (the view `item_pin`) whose origin is
/// the message. A document that is not JSON is left out.
fn pinned(c: &rusqlite::Connection, message: i64) -> rusqlite::Result<Vec<Value>> {
    let mut st = c.prepare_cached(
        "SELECT i.doc FROM item_source s JOIN item i ON i.id = s.item_id
         WHERE s.message_id = ?1 AND s.role = 'origin' AND i.form <> 'conversation'
           AND EXISTS (SELECT 1 FROM item_pin p WHERE p.item_id = i.id)
         ORDER BY i.id",
    )?;
    let docs = st
        .query_map([message], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(docs
        .iter()
        .filter_map(|d| serde_json::from_str(d).ok())
        .collect())
}

/// True when `from` is a stranger that the person declined at the gate:
/// `declined_at` is set and the kind is `stranger` or not set yet (the rule
/// of `kind_sql` in `iris-api`).
/// The gate state of `from` for [`Seen`]: [`declined_now`], and the row ID
/// and `gate_rev` of the sender row.
fn gate(
    c: &rusqlite::Connection,
    from: Option<&str>,
) -> rusqlite::Result<(bool, Option<(i64, i64)>)> {
    let row = match from {
        Some(f) => c
            .query_row(
                "SELECT id, gate_rev FROM sender WHERE address = ?1",
                [f],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?,
        None => None,
    };
    Ok((declined_now(c, from)?, row))
}

fn declined_now(c: &rusqlite::Connection, from: Option<&str>) -> rusqlite::Result<bool> {
    let Some(from) = from else {
        return Ok(false);
    };
    c.query_row(
        "SELECT EXISTS (SELECT 1 FROM sender WHERE address = ?1 AND declined_at IS NOT NULL
                          AND coalesce(kind, 'stranger') = 'stranger')",
        [from],
        |r| r.get(0),
    )
}

fn sender_kind_in(c: &rusqlite::Connection, from: &str) -> rusqlite::Result<Kind> {
    let kind: Option<Option<String>> = c
        .query_row("SELECT kind FROM sender WHERE address = ?1", [from], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(kind.flatten().map_or(Kind::Stranger, |k| Kind::parse(&k)))
}

/// The store inputs of one run besides the state of the message. A run
/// reads them with `inputs` only, and each checked write compares them.
#[derive(Clone, Debug, Default, PartialEq)]
struct Inputs {
    /// The known persons with the display name of the sender (spoof check).
    known: Vec<String>,
    /// The clean texts of the earlier messages of the sender (section 7.2).
    same: Vec<String>,
    /// The earlier messages of the thread for the model (section 7.4).
    context: Vec<String>,
    /// The known persons in the headers of the thread (section 7.4).
    parties: Vec<String>,
    /// The pinned items of the message, which the model gets.
    pinned: Vec<Value>,
}

fn inputs(
    c: &rusqlite::Connection,
    message: i64,
    thread: i64,
    date: Option<i64>,
    from_name: Option<&str>,
    from_addr: Option<&str>,
) -> rusqlite::Result<Inputs> {
    Ok(Inputs {
        known: from_name
            .map(|n| known_person(c, n))
            .transpose()?
            .unwrap_or_default(),
        same: same_sender(c, message, thread, date, from_addr)?,
        context: thread_context(c, message, thread, date)?,
        parties: known_parties(c, thread)?,
        pinned: pinned(c, message)?,
    })
}

/// The SQL condition on an earlier message `m` that may shape this run:
/// one rule for the thread context and the texts of the same sender.
/// Hostile mail never shapes a run. Write 1 saves the clean text and the
/// hostile marks together, so a message with a clean text has its marks. A
/// SPAM label counts now, also one that came later. A draft is not mail of
/// the thread yet.
macro_rules! earlier_ok {
    () => {
        concat!(
            "m.thread_id = ?1 AND m.id <> ?2 AND m.gone_at IS NULL
         AND m.internal_date < ?3 AND NOT ",
            iris_store::may_be_hostile!(),
            " AND NOT EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                         WHERE ml.message_id = m.id AND l.provider_id = 'SPAM')
         AND NOT ",
            iris_store::draft_or_copy!("m"),
            "
         AND NOT EXISTS (SELECT 1 FROM header hh JOIN sender s ON s.address = hh.from_addr
                         WHERE hh.message_id = m.id AND s.kind = 'hostile')"
        )
    };
}

/// The most known parties that the model gets.
const KNOWN_PARTIES: i64 = crate::prompt::MAX_KNOWN as i64;

/// The known persons in the headers (From, To, Cc) of the messages of the
/// thread, also the current one, as `Name <address>`, for the model
/// (section 7.4).
fn known_parties(c: &rusqlite::Connection, thread: i64) -> rusqlite::Result<Vec<String>> {
    {
        let mut st = c.prepare_cached(
            "SELECT DISTINCT s.name, s.address FROM message m
             JOIN header h ON h.message_id = m.id
             JOIN sender s ON s.kind = 'person' AND (s.address = h.from_addr
               OR instr(',' || coalesce(h.to_addrs, '') || ',', ',' || s.address || ',') > 0
               OR instr(',' || coalesce(h.cc_addrs, '') || ',', ',' || s.address || ',') > 0)
             WHERE m.thread_id = ?1 AND m.gone_at IS NULL
             ORDER BY s.address LIMIT ?2",
        )?;
        let v = st
            .query_map(params![thread, KNOWN_PARTIES], |r| {
                let (name, addr): (Option<String>, String) = (r.get(0)?, r.get(1)?);
                Ok(match name.filter(|n| !n.is_empty()) {
                    Some(n) => format!("{n} <{addr}>"),
                    None => format!("<{addr}>"),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }
}

/// The addresses of the known persons with this display name.
fn known_person(c: &rusqlite::Connection, name: &str) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare_cached(
        "SELECT address FROM sender WHERE kind = 'person' AND name = ?1 COLLATE NOCASE
         ORDER BY address",
    )?;
    let v = st
        .query_map([name], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(v)
}

/// The most earlier messages of the same sender for the repeated end block.
const SAME_SENDER_MESSAGES: i64 = 5;

/// The clean texts of the earlier messages of the same sender in the thread.
fn same_sender(
    c: &rusqlite::Connection,
    message: i64,
    thread: i64,
    internal_date: Option<i64>,
    from: Option<&str>,
) -> rusqlite::Result<Vec<String>> {
    let (Some(date), Some(from)) = (internal_date, from) else {
        return Ok(Vec::new());
    };
    {
        let mut st = c.prepare_cached(concat!(
            "SELECT b.text FROM message m JOIN body b ON b.message_id = m.id
             JOIN header h ON h.message_id = m.id WHERE h.from_addr = ?4 AND ",
            earlier_ok!(),
            " ORDER BY m.internal_date DESC, m.id DESC LIMIT ?5"
        ))?;
        let v = st
            .query_map(
                params![thread, message, date, from, SAME_SENDER_MESSAGES],
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }
}

/// The most earlier messages of the thread that the model gets.
const THREAD_MESSAGES: i64 = 3;
/// The most characters of the text of one earlier message.
const THREAD_CHARS: usize = 2_000;

/// The clean text of the earlier messages of the thread (before
/// `internal_date`), oldest first. Only a message that understanding read
/// once has its clean text in the store, so the run order is oldest first.
/// Hostile mail and drafts are not context.
fn thread_context(
    c: &rusqlite::Connection,
    message: i64,
    thread: i64,
    internal_date: Option<i64>,
) -> rusqlite::Result<Vec<String>> {
    let Some(date) = internal_date else {
        return Ok(Vec::new());
    };
    let mut v: Vec<String> = {
        let mut st = c.prepare_cached(concat!(
            // The model gets only a message that a run understood, so the
            // model step judged it for phishing too.
            "SELECT b.sender, b.subject, b.text FROM message m JOIN body b ON b.message_id = m.id
             WHERE m.understood_at IS NOT NULL AND ",
            earlier_ok!(),
            " ORDER BY m.internal_date DESC, m.id DESC LIMIT ?4"
        ))?;
        let v = st
            .query_map(params![thread, message, date, THREAD_MESSAGES], |r| {
                let (from, subject, text): (String, String, String) =
                    (r.get(0)?, r.get(1)?, r.get(2)?);
                Ok(format!(
                    "From: {from}\nSubject: {subject}\n{}",
                    crate::cut(&text, THREAD_CHARS)
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        v
    };
    v.reverse();
    Ok(v)
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
/// message, that a rule or the model made, and that are not pinned
/// (`item_pin`, section 7.5). Conversations belong to the conversation rule.
// shortcut: `item_pin` has no ledger references yet; the ledger (R2-03)
// adds them to the view.
/// Makes the mail of the `sender` rows `rows` pending again: a sender that
/// became or stopped being hostile (section 6.7). For each message from
/// these addresses that is not gone: deletes its items that are not pinned
/// (not the conversation), and clears `understood_at`, `tries` and
/// `retry_at`, because the new hostile state is new input. Then computes the
/// conversation of each thread again, so no conversation shows reset mail.
/// Call it in the transaction that changed the kinds.
///
/// The hostile marks that the person can release: the marks of the fixed
/// checks. `spam` (a label of the provider), `phishing` (the answer of the
/// model) and a `hostile` sender (it has the trust of the gate) stay.
pub const RELEASABLE: [&str; 3] = ["dmarc", "spoof", "link"];

/// The result of [`release`].
#[derive(Debug, PartialEq, Eq)]
pub enum Release {
    /// The marks that the person released now, and the marks that stay.
    Done {
        released: Vec<String>,
        kept: Vec<String>,
    },
    /// No message has the row ID, or it is gone.
    NoMessage,
    /// The message has no mark that the person can release, or (for
    /// [`unrelease`]) no release.
    Nothing,
    /// Iris did not understand the current content yet, so its marks are
    /// not known. A release names only marks of the current content.
    Reading,
}

/// The marks and the release of message `message`, when it is not gone.
/// `understood` is false while the stored marks can be from old content.
fn release_row(
    tx: &Transaction,
    message: i64,
) -> iris_store::Result<Option<(Vec<String>, Vec<String>, bool)>> {
    let row: Option<(String, Option<String>, bool)> = tx
        .query_row(
            "SELECT coalesce(hostile, '[]'), released, understood_at IS NOT NULL
               FROM message WHERE id = ?1 AND gone_at IS NULL",
            [message],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(row.map(|(hostile, released, understood)| {
        let list = |j: &str| serde_json::from_str::<Vec<String>>(j).unwrap_or_default();
        (
            list(&hostile),
            released.as_deref().map(list).unwrap_or_default(),
            understood,
        )
    }))
}

/// The person releases received message `message` from quarantine (issues
/// 369, 373 and 374): each mark of [`RELEASABLE`] that it has now goes to
/// `message.released`, with the marks that the person released before. The
/// trigger of migration 62 queues the fixed check, which understands the
/// message again without these marks: with the model, as new mail, if no
/// other mark stays. New content of the message ends the release.
pub fn release(tx: &Transaction, message: i64) -> iris_store::Result<Release> {
    let Some((hostile, mut all, understood)) = release_row(tx, message)? else {
        return Ok(Release::NoMessage);
    };
    // The sync clears `understood_at` when the content changes and keeps
    // the old marks until the next understanding: never release those.
    if !understood {
        return Ok(Release::Reading);
    }
    let now: Vec<String> = hostile
        .iter()
        .filter(|m| RELEASABLE.contains(&m.as_str()) && !all.contains(m))
        .cloned()
        .collect();
    if now.is_empty() {
        return Ok(Release::Nothing);
    }
    all.extend(now.iter().cloned());
    all.sort();
    tx.execute(
        "UPDATE message SET released = ?2 WHERE id = ?1",
        params![
            message,
            serde_json::to_string(&all).expect("strings serialize")
        ],
    )?;
    let kept = hostile.into_iter().filter(|m| !now.contains(m)).collect();
    Ok(Release::Done {
        released: now,
        kept,
    })
}

/// The person ends the release of message `message`. The fixed check gives
/// the marks back. A gone message keeps its row as it is.
pub fn unrelease(tx: &Transaction, message: i64) -> iris_store::Result<Release> {
    let Some((hostile, released, _)) = release_row(tx, message)? else {
        return Ok(Release::NoMessage);
    };
    if released.is_empty() {
        return Ok(Release::Nothing);
    }
    tx.execute(
        "UPDATE message SET released = NULL WHERE id = ?1",
        [message],
    )?;
    Ok(Release::Done {
        released: Vec::new(),
        kept: hostile,
    })
}

/// Set-based writes and thread pages of [`RERUN_PAGE`], so the work is
/// linear in the mail of the sender and the memory is bounded. Gives the
/// count of reset messages.
pub fn rerun_senders(tx: &Transaction, rows: &[i64], now_ms: i64) -> iris_store::Result<u64> {
    let mut reset = 0;
    for &row in rows {
        tx.execute(
            "DELETE FROM item WHERE id IN (
               SELECT d.item_id FROM sender s
               JOIN header h ON h.from_addr = s.address
               JOIN message m ON m.id = h.message_id
               JOIN item_droppable d ON d.message_id = m.id
               WHERE s.id = ?1 AND m.gone_at IS NULL)",
            [row],
        )?;
        reset += tx.execute(
            "UPDATE message SET understood_at = NULL, tries = 0, retry_at = NULL, waits_model = 0
             WHERE gone_at IS NULL AND id IN (
               SELECT h.message_id FROM sender s JOIN header h ON h.from_addr = s.address
               WHERE s.id = ?1)",
            [row],
        )? as u64;
        let mut after = i64::MIN;
        loop {
            let page: Vec<i64> = tx
                .prepare_cached(
                    "SELECT DISTINCT m.thread_id FROM sender s
                     JOIN header h ON h.from_addr = s.address
                     JOIN message m ON m.id = h.message_id
                     WHERE s.id = ?1 AND m.gone_at IS NULL AND m.thread_id > ?2
                     ORDER BY m.thread_id LIMIT ?3",
                )?
                .query_map(params![row, after, RERUN_PAGE], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let Some(&last) = page.last() else { break };
            for t in page {
                iris_talk::update(tx, t, None, now_ms)?;
            }
            after = last;
        }
    }
    Ok(reset)
}

/// Threads in one page of [`rerun_senders`].
pub const RERUN_PAGE: i64 = 500;

/// Computes again the conversation of each thread that a change of a sender
/// kind marked (`talk_due`, store migration 40, issue 106), in writes of
/// [`RERUN_PAGE`] threads. Gives the count of threads. The understanding
/// runner calls it after its sender refresh, and the engine at each turn of
/// its attention thread, so a kind change of the stranger gate counts too.
pub fn update_due_talks(store: &Store, now_ms: i64) -> iris_store::Result<usize> {
    let mut done = 0;
    loop {
        let n = store.write(|tx| Ok(iris_talk::update_due(tx, now_ms, RERUN_PAGE)?))?;
        if n == 0 {
            return Ok(done);
        }
        done += n;
    }
}

/// Deletes the items of `message` that a new run replaces: the rows of the
/// view `item_droppable` (store migration 42). They are origin items of a
/// message that is not gone, not
/// the conversation, made by a rule or the model, and not pinned
/// (`item_pin`: a correction, or an open changeset action that names the
/// item). The label triggers of a draft or a copy read the same view.
fn drop_old_items(tx: &Transaction, message: i64) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM item WHERE id IN
           (SELECT item_id FROM item_droppable WHERE message_id = ?1)",
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
    // A possible duplicate of a pinned item (`item_pin`): same form, same
    // origin message.
    let pinned: Option<i64> = tx
        .query_row(
            "SELECT i.id FROM item_source s JOIN item i ON i.id = s.item_id
             WHERE s.message_id = ?1 AND s.role = 'origin' AND i.form = ?2
               AND EXISTS (SELECT 1 FROM item_pin p WHERE p.item_id = i.id)
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

/// A feed item that the person skipped, or a signal that the person
/// dismissed, stays out of the brief after a new run
/// (docs/specs/2026-11-17-preference-rules.md). The new item takes its
/// status only on a one-to-one match: exactly one pinned item and exactly
/// one new item of the message with the same form and the same content
/// (the document without the fields that each run or the person changes),
/// and the pinned item has the chosen status. Each other new item stays
/// visible, also one that differs in one field, such as a code. Call after the run writes its items: each item of the message
/// that is not pinned is new.
fn keep_choices(tx: &Transaction, message: i64) -> rusqlite::Result<()> {
    tx.execute(
        "WITH mine AS (
           SELECT i.id, i.form, i.status,
                  json_remove(i.doc, '$.id', '$.status', '$.created', '$.updated',
                              '$.derived', '$.model', '$.attention') AS body,
                  EXISTS (SELECT 1 FROM item_pin p WHERE p.item_id = i.id) AS pinned
           FROM item_source s JOIN item i ON i.id = s.item_id
           WHERE s.message_id = ?1 AND s.role = 'origin' AND i.form IN ('feed', 'signal')
         ),
         pair AS (
           SELECT max(CASE WHEN pinned THEN status END) AS status,
                  max(CASE WHEN NOT pinned THEN id END) AS new
           FROM mine WHERE body IS NOT NULL
           GROUP BY form, body
           HAVING sum(pinned) = 1 AND sum(NOT pinned) = 1
         )
         UPDATE item SET status = pair.status,
                         doc = json_set(item.doc, '$.status', pair.status)
         FROM pair
         WHERE item.id = pair.new AND pair.status IN ('skipped', 'dismissed')",
        [message],
    )?;
    Ok(())
}

/// The row ID of the sender row of `addr`. A new row has no kind:
/// iris-senders decides it.
pub fn sender_row(tx: &Transaction, addr: &str, name: Option<&str>) -> rusqlite::Result<i64> {
    let addr = addr.to_ascii_lowercase();
    tx.execute(
        "INSERT INTO sender(address, name) VALUES (?1, ?2) ON CONFLICT(address) DO NOTHING",
        params![addr, name],
    )?;
    tx.query_row("SELECT id FROM sender WHERE address = ?1", [addr], |r| {
        r.get(0)
    })
}
