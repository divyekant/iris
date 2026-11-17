//! The verbs `draft` and `send` (spec `docs/specs/2026-11-16-send.md`):
//! the apply of a draft create, the release of a send, the completion of
//! a sent message and the recovery of both calls.
//!
//! Iris never makes `drafts.create` or `drafts.send` two times for one
//! action. After an unknown result, the recovery searches at Gmail by the
//! `Message-ID` that the claim saved, and never calls again.

use iris_compose::{Content, Reply};
use iris_gmail::Error as GmailError;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{
    check, decide, gate, lock, now, sync_lock, writing_rows, Error, Outcome, Result, Run, Token,
};
use iris_gmail::Gmail;
use iris_store::Store;

/// The hold delay of a send after its approval (spec 2, section 7.5).
pub const HOLD_MS: i64 = 60_000;
/// A send claim with no sent message at Gmail after this time ends as
/// `failed`. Its `ledger_send` row stays: only a definite rejection proves
/// that Gmail did not run the request (section 6, step 2).
const UNKNOWN_MS: i64 = 24 * 60 * 60 * 1000;

/// The error of a send whose sent copy does not have the approved content.
/// A ledger row with it stops each later release (section 6).
pub const SENT_OTHER: &str = "Gmail sent other content than the approved content";

/// The content of a `draft` row and the `post` of a create: the content
/// with its reply headers.
#[derive(Serialize, Deserialize)]
struct Stored {
    content: Content,
    reply: Option<Reply>,
}

/// The `ops` of a ledger row of a write verb.
#[derive(Serialize, Deserialize)]
#[serde(tag = "call")]
enum Ops {
    #[serde(rename = "drafts.create")]
    Create {
        message_id: String,
        /// Seconds since 1970: compose makes the same bytes again.
        date: i64,
        hash: String,
        /// The Gmail thread ID of a reply.
        thread: Option<String>,
    },
    #[serde(rename = "drafts.send")]
    Send {
        draft: i64,
        provider_id: String,
        message_id: String,
        hash: String,
    },
}

/// The `ops` of the ledger row, if it is a row of a write verb.
fn ops_of(store: &Store, ledger: i64) -> Result<Option<Ops>> {
    let ops: String = store.read(|c| {
        Ok(
            c.query_row("SELECT ops FROM ledger WHERE id = ?1", [ledger], |r| {
                r.get(0)
            })?,
        )
    })?;
    Ok(serde_json::from_str(&ops).ok())
}

/// True if the ledger row is a row of a write verb.
pub(crate) fn is_write(store: &Store, ledger: i64) -> Result<bool> {
    Ok(ops_of(store, ledger)?.is_some())
}

fn random_id(address: &str) -> std::result::Result<String, Error> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|e| Error::Store(iris_store::Error::Query(format!("no random bytes: {e}"))))?;
    Ok(iris_compose::message_id(bytes, address))
}

const GROWN: &str = "The warnings of the draft changed after the approval.";

/// True if `content` has a draft flag that the approval of `action` did
/// not see. Then the action is denied with [`GROWN`].
fn grown(
    store: &Store,
    action: i64,
    address: &str,
    content: &iris_compose::Content,
) -> Result<bool> {
    let new = store.read(|c| {
        iris_changeset::new_flags(c, action, address, content).map_err(changeset_error)
    })?;
    if !new.is_empty() {
        decide(store, action, "denied", GROWN)?;
    }
    Ok(!new.is_empty())
}

/// The apply of an approved `draft` action (section 4), after the checks
/// of [`super::apply`]: the parent of a reply, the claim, one
/// `drafts.create` call, and the `draft` row.
pub(crate) fn draft(run: &mut Run<'_, '_>, action: i64) -> Result<Outcome> {
    let store = run.store;
    let (args, reply_target): (String, bool) = store.read(|c| {
        Ok(c.query_row(
            "SELECT args, json_type(targets, '$.threads') IS NOT NULL
             FROM changeset_action WHERE id = ?1",
            [action],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    })?;
    let deny = |why: &str| -> Result<Outcome> {
        decide(store, action, "denied", why)?;
        Ok(Outcome::Denied(why.into()))
    };
    let Ok(mut content) = serde_json::from_str::<Content>(&args) else {
        return deny("the arguments are not a draft content");
    };
    let (mut reply, mut thread) = (None, None);
    if reply_target {
        let (parent, hostile) = store.read(|c| {
            let p = iris_changeset::parent(c, action).map_err(changeset_error)?;
            let h = iris_changeset::hostile(c, action).map_err(changeset_error)?;
            Ok((p, h))
        })?;
        if hostile {
            return deny("the thread has hostile mail: only a person can draft in it");
        }
        let Some(p) = parent.filter(|p| !p.gone) else {
            return deny("the parent message is gone");
        };
        let (provider, thread_provider): (String, String) = store.read(|c| {
            Ok(c.query_row(
                "SELECT m.provider_id, t.provider_id FROM message m
                 JOIN thread t ON t.id = m.thread_id WHERE m.id = ?1",
                [p.message],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })?;
        // The headers of the parent at Gmail: the mirror can lack the
        // Message-ID before the understanding of the message.
        let names = ["Message-ID", "References", "In-Reply-To", "Subject"];
        let head = match run.call(|g, t| g.metadata(t, &provider, &names)) {
            Ok(h) => h,
            Err(GmailError::NotFound) => return deny("the parent message is gone"),
            Err(e) => return Err(Error::Gmail(e)),
        };
        let header = |name: &str| {
            head.headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .map(|h| h.value.clone())
        };
        let in_reply_to = header("In-Reply-To").or(p.in_reply_to.clone());
        let Some(r) = header("Message-ID").and_then(|id| {
            iris_compose::references(&id, header("References").as_deref(), in_reply_to.as_deref())
        }) else {
            return deny("the parent message has no Message-ID: Gmail cannot thread a reply");
        };
        reply = Some(r);
        content.subject = iris_compose::reply_subject(&header("Subject").unwrap_or(p.subject));
        thread = Some(thread_provider);
    }
    let content = match content.check() {
        Ok(c) => c,
        Err(why) => return deny(&why),
    };
    // The subject of a reply comes from Gmail here, not from the mirror:
    // check the flags of the final content (send-checks spec, section 4).
    if grown(store, action, &run.address, &content)? {
        return Ok(Outcome::Denied(GROWN.into()));
    }
    let hash = iris_compose::hash(&content, reply.as_ref());
    let message_id = random_id(&run.address)?;
    let ops = Ops::Create {
        message_id: message_id.clone(),
        date: now() / 1000,
        hash,
        thread: thread.clone(),
    };
    let post = Stored { content, reply };
    let raw = compose(&run.address, &ops, &post);
    let Some(ledger) = claim(run, action, &ops, Some(&post), None)? else {
        return Err(Error::NotApproved(action));
    };
    let ledger = match ledger {
        Ok(l) => l,
        Err(o) => return Ok(o),
    };
    match run.call(|g, t| g.create_draft(t, &raw, thread.as_deref())) {
        Ok(d) => created(run, ledger, action, &d.id, &d.message.thread_id),
        Err(e) if e.write_result_unknown() => recover(run, ledger),
        Err(e) => end(run, ledger, "failed", Some(e.to_string()), false),
    }
}

/// The message of a create, from its ops and its post: the same input
/// gives the same bytes (section 4, step 2).
fn compose(address: &str, ops: &Ops, post: &Stored) -> Vec<u8> {
    let Ops::Create {
        message_id, date, ..
    } = ops
    else {
        unreachable!("compose of a create")
    };
    iris_compose::compose(
        address,
        message_id,
        &post.content,
        post.reply.as_ref(),
        *date,
    )
}

/// The claim: in one transaction, the ledger row in `writing` with its
/// ops and post. For a send, also the `ledger_send` row of the draft, and
/// `call` 1. Gives `None` if the action is not approved, or the outcome of
/// the gate.
fn claim(
    run: &Run<'_, '_>,
    action: i64,
    ops: &Ops,
    post: Option<&Stored>,
    draft: Option<i64>,
) -> Result<Option<std::result::Result<i64, Outcome>>> {
    let account = run.account;
    let ops = serde_json::to_string(ops).expect("plain data");
    let post = post.map(|p| serde_json::to_string(p).expect("plain data"));
    Ok(run.store.write(|tx| {
        if let Some(o) = gate(tx, action)? {
            return Ok(Some(Err(o)));
        }
        // A send leaves only from `held`, and only while the switch is on.
        // The switch is read here, in the transaction of the claim; the
        // apply lock that the caller holds keeps `iris sending off` out
        // until the call ends.
        if draft.is_some() && switch_on(tx)?.is_none() {
            return Ok(Some(Err(Outcome::SendingOff)));
        }
        let state = if draft.is_some() { "held" } else { "approved" };
        let free: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM changeset_action WHERE id = ?1 AND state = ?3)
               AND NOT EXISTS (SELECT 1 FROM ledger_send WHERE draft_id = ?2)",
            params![action, draft, state],
            |r| r.get(0),
        )?;
        if !free {
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO ledger(action_id, account_id, ops, post, state, undo, at)
             VALUES (?1, ?2, ?3, ?4, 'writing', 'none', ?5)",
            params![action, account, ops, post, now()],
        )?;
        let ledger = tx.last_insert_rowid();
        // The call starts now: the claim records it before the call.
        tx.execute("UPDATE ledger SET call = 1 WHERE id = ?1", [ledger])?;
        if let Some(d) = draft {
            tx.execute(
                "INSERT INTO ledger_send(draft_id, ledger_id) VALUES (?1, ?2)",
                params![d, ledger],
            )?;
        }
        Ok(Some(Ok(ledger)))
    })?)
}

/// Gmail made the draft `provider_id`: one transaction writes the `draft`
/// row from the ledger post and ops, and sets the ledger row `applied`.
fn created(
    run: &mut Run<'_, '_>,
    ledger: i64,
    action: i64,
    provider_id: &str,
    _gmail_thread: &str,
) -> Result<Outcome> {
    let (ops, post) = row(run.store, ledger)?;
    let post: Stored = serde_json::from_str(&post.expect("a create has a post")).expect("ours");
    let raw = compose(&run.address, &ops, &post);
    let Ops::Create {
        message_id, hash, ..
    } = &ops
    else {
        unreachable!("a create row")
    };
    let (account, address) = (run.account, run.address.clone());
    let _ = address;
    let content = serde_json::to_string(&post).expect("plain data");
    run.store.write(|tx| {
        let thread: Option<i64> = tx
            .query_row(
                "SELECT m.thread_id FROM action_message am JOIN message m ON m.id = am.message_id
                 WHERE am.action_id = ?1",
                [action],
                |r| r.get(0),
            )
            .optional()?;
        let t = now();
        tx.execute(
            "INSERT INTO draft(account_id, thread_id, provider_id, message_id_hdr, content, hash,
                               raw, action_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                account,
                thread,
                provider_id,
                message_id,
                content,
                hash,
                raw,
                action,
                t
            ],
        )?;
        tx.execute(
            "UPDATE ledger SET state = 'applied' WHERE id = ?1 AND state = 'writing'",
            [ledger],
        )?;
        Ok(())
    })?;
    Ok(Outcome::Applied(super::Counts {
        changed: 1,
        ..Default::default()
    }))
}

/// The ops and the post of a ledger row of a write verb.
fn row(store: &Store, ledger: i64) -> Result<(Ops, Option<String>)> {
    let (ops, post): (String, Option<String>) = store.read(|c| {
        Ok(c.query_row(
            "SELECT ops, post FROM ledger WHERE id = ?1",
            [ledger],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    })?;
    Ok((serde_json::from_str(&ops).expect("a write row"), post))
}

/// End a ledger row in `writing` as `failed` or `applied`, with an error.
/// `free`: the transaction also deletes the `ledger_send` row, because
/// Gmail did not send (section 5, step 4, and section 6, step 2).
fn end(
    run: &Run<'_, '_>,
    ledger: i64,
    state: &str,
    error: Option<String>,
    free: bool,
) -> Result<Outcome> {
    run.store.write(|tx| {
        tx.execute(
            "UPDATE ledger SET state = ?2, error = ?3 WHERE id = ?1 AND state = 'writing'",
            params![ledger, state, error],
        )?;
        if free {
            tx.execute("DELETE FROM ledger_send WHERE ledger_id = ?1", [ledger])?;
        }
        Ok(())
    })?;
    Ok(match error {
        Some(error) if state == "failed" => Outcome::Failed {
            counts: Default::default(),
            error,
        },
        _ => Outcome::Applied(Default::default()),
    })
}

/// The recovery of a ledger row of a write verb in `writing` (section 6).
/// It never calls `drafts.create` or `drafts.send`.
pub(crate) fn recover(run: &mut Run<'_, '_>, ledger: i64) -> Result<Outcome> {
    let (ops, _) = row(run.store, ledger)?;
    let (action, at): (i64, i64) = run.store.read(|c| {
        Ok(c.query_row(
            "SELECT action_id, at FROM ledger WHERE id = ?1",
            [ledger],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    })?;
    match ops {
        Ops::Create { message_id, .. } => {
            let q = format!("rfc822msgid:{message_id}");
            let found = run.call(|g, t| g.drafts(t, &q)).map_err(Error::Gmail)?;
            match found.first() {
                Some(d) => {
                    let (id, thread) = (d.id.clone(), d.message.thread_id.clone());
                    created(run, ledger, action, &id, &thread)
                }
                None => end(
                    run,
                    ledger,
                    "failed",
                    Some("Gmail has no draft of the create".into()),
                    false,
                ),
            }
        }
        Ops::Send {
            draft,
            provider_id: _,
            message_id,
            hash,
        } => {
            let q = format!("rfc822msgid:{message_id}");
            let page = run
                .call(|g, t| g.search(t, &q, None))
                .map_err(Error::Gmail)?;
            for m in &page.messages {
                let labels = run.call(|g, t| g.minimal(t, &m.id));
                match labels {
                    Ok(l) if l.label_ids.iter().any(|l| l == "SENT") => {
                        return complete(run, ledger, draft, &m.id, &hash);
                    }
                    Ok(_) | Err(GmailError::NotFound) => {}
                    Err(e) => return Err(Error::Gmail(e)),
                }
            }
            if now() - at >= UNKNOWN_MS {
                end(
                    run,
                    ledger,
                    "failed",
                    Some("unknown: Gmail shows no sent message".into()),
                    false,
                )
            } else {
                Ok(Outcome::Pending)
            }
        }
    }
}

/// The completion of a sent message (section 6): read the sent copy, then
/// in one transaction the `draft` row gets `sent_at` and `sent_message`,
/// and the ledger row becomes `applied`, with [`SENT_OTHER`] if the sent
/// content is not the approved content. If the read fails, the row stays
/// in `writing`, and recovery completes it.
fn complete(
    run: &mut Run<'_, '_>,
    ledger: i64,
    draft: i64,
    sent_id: &str,
    approved: &str,
) -> Result<Outcome> {
    let raw = run.call(|g, t| {
        let mut v = Vec::new();
        g.raw(t, sent_id, &mut v).map(|_| v)
    });
    let raw = match raw {
        Ok(r) => r,
        Err(_) => return Ok(Outcome::Pending),
    };
    let same = iris_compose::read_sent(&raw, &run.address)
        .is_some_and(|r| iris_compose::hash(&r.content, r.reply.as_ref()) == approved);
    let error = (!same).then(|| SENT_OTHER.to_string());
    run.store.write(|tx| {
        let t = now();
        tx.execute(
            "UPDATE draft SET sent_at = ?2, sent_message = ?3, updated_at = ?2 WHERE id = ?1",
            params![draft, t, sent_id],
        )?;
        tx.execute(
            "UPDATE ledger SET state = 'applied', error = ?2 WHERE id = ?1 AND state = 'writing'",
            params![ledger, error],
        )?;
        Ok(())
    })?;
    Ok(Outcome::Applied(super::Counts {
        changed: 1,
        ..Default::default()
    }))
}

/// The draft of a send action.
struct Draft {
    id: i64,
    account: i64,
    provider_id: String,
    message_id: String,
    hash: String,
    raw: Vec<u8>,
    thread: Option<String>,
    sent: bool,
    foreign: bool,
}

/// The release of an approved `send` action (section 5): the hold, the
/// checks, a read of the draft at Gmail, the claim, one `drafts.send`
/// call with the bytes of the `draft` row, and the completion.
///
/// No engine path calls it before R3-02 and R3-03 (spec, Not in scope).
/// It takes the locks of [`super::apply`].
pub fn release(
    store: &Store,
    gmail: &Gmail,
    token: &mut Token<'_>,
    account: i64,
    action: i64,
) -> Result<Outcome> {
    let _lock = lock(store)?;
    let _sync = sync_lock(store, account)?;
    #[allow(clippy::type_complexity)]
    let head: Option<(String, Option<String>, String, i64, Option<i64>)> = store.read(|c| {
        Ok(c.query_row(
            "SELECT a.verb, a.hash, a.decided_by, a.decided_at,
                    (SELECT id FROM ledger WHERE action_id = a.id AND state = 'writing')
             FROM changeset_action a WHERE a.id = ?1 AND a.state = 'held'",
            [action],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?)
    })?;
    let (verb, approved_hash, decided_by, decided_at, writing) =
        head.ok_or(Error::NotApproved(action))?;
    if verb != "send" {
        return Err(Error::Verb(verb));
    }
    let d: Draft = store.read(|c| {
        Ok(c.query_row(
            "SELECT d.id, d.account_id, d.provider_id, d.message_id_hdr, d.hash, d.raw,
                    t.provider_id, d.sent_at IS NOT NULL, d.foreign_at IS NOT NULL
             FROM action_draft ad JOIN draft d ON d.id = ad.draft_id
             LEFT JOIN thread t ON t.id = d.thread_id WHERE ad.action_id = ?1",
            [action],
            |r| {
                Ok(Draft {
                    id: r.get(0)?,
                    account: r.get(1)?,
                    provider_id: r.get(2)?,
                    message_id: r.get(3)?,
                    hash: r.get(4)?,
                    raw: r.get(5)?,
                    thread: r.get(6)?,
                    sent: r.get(7)?,
                    foreign: r.get(8)?,
                })
            },
        )?)
    })?;
    if d.account != account {
        return Err(Error::OtherMailbox);
    }
    let mut run = Run::new(store, gmail, token, account)?;
    run.mailbox()?;
    if let Some(ledger) = writing {
        return recover(&mut run, ledger);
    }
    if writing_rows(store, account)? {
        run.recover_account()?;
    }
    // A send whose sent copy differed stops each later release until the
    // owner checks assumption 6 (section 6).
    let stopped: bool = store.read(|c| {
        Ok(c.query_row(
            "SELECT EXISTS (SELECT 1 FROM ledger WHERE error = ?1)",
            [SENT_OTHER],
            |r| r.get(0),
        )?)
    })?;
    if stopped {
        return Err(Error::SendStopped);
    }
    // The hold starts at the later of the approval and the time the switch
    // went on (docs/specs/2026-11-17-held-outbox.md, section 1).
    let Some(on_at) = store.read(switch_on)? else {
        return Ok(Outcome::SendingOff);
    };
    let until = decided_at.max(on_at) + HOLD_MS;
    if now() < until {
        return Ok(Outcome::Hold(until));
    }
    if let Some(o) = check(store, account, action, &decided_by, approved_hash)? {
        return Ok(o);
    }
    if d.sent || d.foreign {
        let why = "the draft was sent, or has a form that Iris does not write";
        decide(store, action, "denied", why)?;
        return Ok(Outcome::Denied(why.into()));
    }
    // Step 2: read the draft at Gmail.
    let at_gmail = match run.call(|g, t| g.draft_raw(t, &d.provider_id)) {
        Ok(g) => g,
        Err(GmailError::NotFound) => {
            let why = "the draft is gone at Gmail";
            decide(store, action, "denied", why)?;
            return Ok(Outcome::Denied(why.into()));
        }
        Err(e) => return Err(Error::Gmail(e)),
    };
    // A draft over DRAFT_LIMIT has no raw: it is not in the form of Iris.
    let raw = at_gmail.raw.unwrap_or_default();
    let read = iris_compose::read(&raw, &run.address);
    let same = read.as_ref().is_some_and(|r| {
        r.message_id == d.message_id && iris_compose::hash(&r.content, r.reply.as_ref()) == d.hash
    });
    if !same {
        return changed_at_gmail(store, action, &d, read, &raw);
    }
    // The flags of the mail that Gmail holds, the same bytes as the draft
    // row (docs/specs/2026-11-17-send-checks.md, section 4).
    let content = &read.as_ref().expect("same").content;
    if grown(store, action, &run.address, content)? {
        return Ok(Outcome::Denied(GROWN.into()));
    }
    // Step 3: the claim.
    let ops = Ops::Send {
        draft: d.id,
        provider_id: d.provider_id.clone(),
        message_id: d.message_id.clone(),
        hash: d.hash.clone(),
    };
    let ledger = match claim(&run, action, &ops, None, Some(d.id))? {
        None => return Err(Error::NotApproved(action)),
        Some(Err(o)) => return Ok(o),
        Some(Ok(l)) => l,
    };
    // Step 4: one call, with the bytes of the draft row.
    match run.call(|g, t| g.send_draft(t, &d.provider_id, &d.raw, d.thread.as_deref())) {
        Ok(sent) => complete(&mut run, ledger, d.id, &sent.id, &d.hash),
        Err(e) if e.write_result_unknown() => recover(&mut run, ledger),
        Err(e) => end(&run, ledger, "failed", Some(e.to_string()), true),
    }
}

/// The draft changed at Gmail (section 4.1): in one transaction, the
/// engine denies the send and refreshes the `draft` row from Gmail, or
/// marks it `foreign_at` if Iris does not write its form.
fn changed_at_gmail(
    store: &Store,
    action: i64,
    d: &Draft,
    read: Option<iris_compose::Read>,
    raw: &[u8],
) -> Result<Outcome> {
    let why = "the draft changed at Gmail after the approval";
    let t = now();
    let refresh = read.filter(|r| r.message_id == d.message_id).map(|r| {
        let hash = iris_compose::hash(&r.content, r.reply.as_ref());
        let stored = Stored {
            content: r.content,
            reply: r.reply,
        };
        (serde_json::to_string(&stored).expect("plain data"), hash)
    });
    let n = store.write(|tx| {
        let n = tx.execute(
            "UPDATE changeset_action SET state = 'denied', decided_by = 'engine', rule = NULL,
               policy = NULL, reason = ?2, decided_at = ?3
             WHERE id = ?1 AND state IN ('approved', 'held')",
            params![action, why, t],
        )?;
        if n == 1 {
            match &refresh {
                Some((content, hash)) => tx.execute(
                    "UPDATE draft SET content = ?2, hash = ?3, raw = ?4, updated_at = ?5
                     WHERE id = ?1",
                    params![d.id, content, hash, raw, t],
                )?,
                None => tx.execute(
                    "UPDATE draft SET foreign_at = ?2, updated_at = ?2 WHERE id = ?1",
                    params![d.id, t],
                )?,
            };
        }
        Ok(n)
    })?;
    if n == 0 {
        return Err(Error::NotApproved(action));
    }
    Ok(Outcome::Denied(why.into()))
}

pub(crate) fn changeset_error(e: iris_changeset::Error) -> iris_store::Error {
    match e {
        iris_changeset::Error::Store(e) => e,
        other => iris_store::Error::Query(other.to_string()),
    }
}

/// The time the send switch went on, or `None` while it is off.
fn switch_on(c: &rusqlite::Connection) -> iris_store::Result<Option<i64>> {
    Ok(
        c.query_row("SELECT on_at FROM send_switch WHERE id = 1", [], |r| {
            r.get(0)
        })?,
    )
}

/// The state of the send switch: the time it went on, or `None` while off.
pub fn sending(store: &Store) -> Result<Option<i64>> {
    Ok(store.read(switch_on)?)
}

/// The longest wait of `iris sending off` for the apply lock. It is
/// shorter than the answer wait of the socket command (30 seconds).
pub const OFF_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Turn the send switch on or off (`iris sending on|off`) at the time of
/// the write. Off takes the
/// apply lock first, which the release holds from its claim to the end of
/// `drafts.send`: when off returns, no send call is in progress, and each
/// later claim reads off. If the lock stays busy for [`OFF_WAIT`], nothing
/// changes and the answer is [`Error::Busy`].
pub fn set_sending(store: &Store, on: bool) -> Result<()> {
    let _lock = if on {
        None
    } else {
        let start = std::time::Instant::now();
        loop {
            match super::lock(store) {
                Ok(l) => break Some(l),
                Err(Error::Busy) if start.elapsed() < OFF_WAIT => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => return Err(e),
            }
        }
    };
    // The clock is read here, after the lock and the confirmation of the
    // caller, so a hold always counts from the moment the switch changed.
    let now_ms = super::now();
    store.write(|tx| {
        tx.execute(
            "UPDATE send_switch SET on_at = CASE WHEN ?1 THEN coalesce(on_at, ?2) END,
               changed_at = ?2 WHERE id = 1",
            params![on, now_ms],
        )?;
        Ok(())
    })?;
    Ok(())
}
