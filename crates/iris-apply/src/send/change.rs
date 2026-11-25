//! The apply and the recovery of a change of a draft
//! (`docs/specs/2026-11-25-draft-change.md`, sections 2 and 4).
//!
//! Iris calls `drafts.update` at most one time for one action. After an
//! unknown result, recovery reads the draft at Gmail and never calls
//! again.

use iris_compose::Content;
use iris_provider::Error as GmailError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{
    changed_at_gmail, changeset_error, compose_as, decide, end, files, grown, now, owner_name,
    refresh, row, Error, Ops, Outcome, Result, Run, Stored, GROWN, UNKNOWN_MS,
};

/// The draft of the change is gone at Gmail.
pub const GONE: &str = "The draft is gone at Gmail.";
/// The draft at Gmail is not the content of the draft row.
pub const CHANGED: &str = "The draft changed at Gmail. Iris did not change it.";
/// A reply change keeps its reply headers, so it cannot answer a newer
/// message (section 1).
pub const NEW_MAIL: &str = "A new mail came into this thread. Write a new reply.";
/// An attachment of the stored draft has no file (section 1): the undo
/// could not compose the old draft again.
pub const OLD_FILE: &str = "A file of the draft is not in Iris. Change it in Gmail.";
/// The read back after a call that applied has another hash (section 7).
pub const DURING: &str = "The draft changed at Gmail during the change. Look at it in Gmail.";
/// Recovery cannot show that the call applied (section 4, step 3).
pub const UNKNOWN: &str = "Iris does not know if the draft changed. Look at it in Gmail.";
/// An attachment of the content before a change has no file: the undo
/// cannot compose the old draft again (section 3).
pub const UNDO_FILE: &str = "A file of the old draft is gone.";

/// The change entry that an undo undoes: the `Date` of its `pre`, and the
/// hash after the change, which must be the hash at Gmail (section 3).
struct Undoes {
    date: Option<i64>,
    hash: String,
}

/// The `pre` of the ledger entry of a change: the stored content of the
/// draft row before the change, and its `Date` (the undo composes the
/// same message again).
#[derive(Serialize, Deserialize)]
struct Pre {
    draft: Stored,
    date: Option<i64>,
}

/// The draft row that the action changes.
struct Row {
    id: i64,
    provider_id: String,
    message_id: String,
    stored: String,
    hash: String,
    /// The Gmail thread ID of a reply draft.
    thread: Option<String>,
}

/// The apply of an approved `draft` action with a `drafts` target
/// (section 2, steps 1 to 8), or of the undo of a change, with the
/// differences of section 3.
pub(super) fn apply(run: &mut Run<'_, '_>, action: i64, args: &str) -> Result<Outcome> {
    let store = run.store;
    let deny = |why: &str| -> Result<Outcome> {
        decide(store, action, "denied", why)?;
        Ok(Outcome::Denied(why.into()))
    };
    let row: Option<Row> = store.read(|c| {
        Ok(c.query_row(
            "SELECT d.id, d.provider_id, d.message_id_hdr, d.content, d.hash, t.provider_id
             FROM action_draft ad JOIN draft d ON d.id = ad.draft_id
             LEFT JOIN thread t ON t.id = d.thread_id WHERE ad.action_id = ?1",
            [action],
            |r| {
                Ok(Row {
                    id: r.get(0)?,
                    provider_id: r.get(1)?,
                    message_id: r.get(2)?,
                    stored: r.get(3)?,
                    hash: r.get(4)?,
                    thread: r.get(5)?,
                })
            },
        )
        .optional()?)
    })?;
    let Some(d) = row else {
        return deny("The draft does not exist.");
    };
    let undoes = undoes(store, action)?;
    // Step 1: the checks of section 1 again.
    let denied = store
        .read(|c| iris_changeset::change_denied(c, action, d.id, now()).map_err(changeset_error))?;
    if let Some(why) = denied {
        return deny(&why);
    }
    if !run.mail.updates_drafts() {
        return deny("Iris cannot change a draft of this account.");
    }
    let Ok(old) = serde_json::from_str::<Stored>(&d.stored) else {
        return deny("The draft has a form that Iris does not write. Change it in Gmail.");
    };
    let Ok(mut content) = serde_json::from_str::<Content>(args) else {
        return deny("the arguments are not a draft content");
    };
    // The files of the stored draft: the undo composes it again. The undo
    // itself has no undo.
    if undoes.is_none() && files(store, &old.content)?.is_none() {
        return deny(OLD_FILE);
    }
    // A reply keeps the subject of its draft (section 1), as at submit,
    // and answers the parent that the approval binds.
    if d.thread.is_some() {
        content.subject = old.content.subject.clone();
        if !same_parent(run, action, old.reply.as_ref())? {
            return deny(NEW_MAIL);
        }
    }
    let content = match content.check() {
        Ok(c) => c,
        Err(why) => return deny(&why),
    };
    // Step 2: the new message, with the reply headers and the Message-ID
    // of the draft row.
    if grown(store, action, &run.address, &content)? {
        return Ok(Outcome::Denied(GROWN.into()));
    }
    let Some(files) = files(store, &content)? else {
        return deny(match undoes {
            Some(_) => UNDO_FILE,
            None => "an attachment file is missing or changed",
        });
    };
    let hash = iris_compose::hash(&content, old.reply.as_ref());
    let post = Stored {
        content,
        reply: old.reply.clone(),
        name: owner_name(run)?,
    };
    // The undo composes the old message again, with its `Date`.
    let date = undoes.as_ref().and_then(|u| u.date).unwrap_or(now() / 1000);
    // The content at Gmail that the action replaces: the draft row, or
    // after the change that an undo undoes.
    let expect = undoes.as_ref().map_or(&d.hash, |u| &u.hash).clone();
    let raw = compose_as(&run.address, &d.message_id, date, &post, &files);
    let provider: String = store.read(|c| {
        Ok(c.query_row(
            "SELECT provider FROM account WHERE id = ?1",
            [run.account],
            |r| r.get(0),
        )?)
    })?;
    if raw.len() as u64 > iris_compose::max_message(&provider) {
        return deny("the mail is too large: remove a file");
    }
    // Step 3: the draft at Gmail must be the content of the draft row.
    let at = match run.call(|g, t| g.draft_raw(t, &d.provider_id)) {
        Ok(a) => a,
        Err(GmailError::NotFound) => return deny(GONE),
        Err(e) => return Err(Error::Provider(e)),
    };
    let at_raw = at.raw.unwrap_or_default();
    let read = iris_compose::read(&at_raw, &run.address);
    let same = read
        .as_ref()
        .is_some_and(|r| iris_compose::hash(&r.content, r.reply.as_ref()) == expect);
    if !same {
        let new_ids = run.mail.sets_draft_message_id();
        return changed_at_gmail(
            store,
            action,
            d.id,
            &d.message_id,
            read,
            &at_raw,
            CHANGED.into(),
            new_ids,
        );
    }
    let pre = Pre {
        draft: old,
        date: read.and_then(|r| r.date),
    };
    let ops = Ops::Update {
        draft: d.id,
        provider_id: d.provider_id.clone(),
        message_id: d.message_id.clone(),
        date,
        pre_hash: expect.clone(),
        hash: hash.clone(),
        thread: d.thread.clone(),
    };
    // The same content again: no call, a final entry (section 2, step 3).
    let noop = hash == expect;
    let pre = (!noop && undoes.is_none()).then_some(pre);
    let ledger = match claim(run, action, d.id, &ops, &post, pre.as_ref(), noop)? {
        None => return Err(Error::NotApproved(action)),
        Some(Err(o)) => return Ok(o),
        Some(Ok(l)) => l,
    };
    if noop {
        return Ok(Outcome::Applied(Default::default()));
    }
    // Step 5: one call.
    match run.call(|g, t| g.update_draft(t, &d.provider_id, &raw, d.thread.as_deref())) {
        Ok(new) => {
            // Step 6: read the draft back. A failed read leaves the row in
            // `writing`, and recovery completes it.
            match run.call(|g, t| g.draft_raw(t, &new.id)) {
                Ok(back) => settle(run, ledger, &new.id, &back.raw.unwrap_or_default(), DURING),
                Err(_) => Ok(Outcome::Pending),
            }
        }
        Err(e) if e.write_result_unknown() => recover(run, ledger, now()),
        // Step 7: a definite rejection. The draft row does not change.
        Err(e) => end(run, ledger, "failed", Some(e.to_string()), false),
    }
}

/// The change entry that the action undoes, or `None` for a change.
fn undoes(store: &iris_store::Store, action: i64) -> Result<Option<Undoes>> {
    Ok(store.read(|c| {
        Ok(c.query_row(
            "SELECT json_extract(l.pre, '$.date'), json_extract(l.ops, '$.hash') FROM ledger_undo u
             JOIN ledger l ON l.id = u.ledger_id WHERE u.action_id = ?1",
            [action],
            |r| {
                Ok(Undoes {
                    date: r.get(0)?,
                    hash: r.get(1)?,
                })
            },
        )
        .optional()?)
    })?)
}

/// True if the parent of the action (the newest eligible message of the
/// thread at submit, bound by the approval) is the message that the reply
/// headers of the draft answer. The Message-ID comes from Gmail: the
/// mirror can lack it before the understanding of the message.
fn same_parent(
    run: &mut Run<'_, '_>,
    action: i64,
    reply: Option<&iris_compose::Reply>,
) -> Result<bool> {
    let Some(reply) = reply else {
        return Ok(false);
    };
    let parent = run.store.read(|c| {
        let p = iris_changeset::parent(c, action).map_err(changeset_error)?;
        Ok(match p.filter(|p| !p.gone) {
            Some(p) => Some(c.query_row(
                "SELECT provider_id FROM message WHERE id = ?1",
                [p.message],
                |r| r.get::<_, String>(0),
            )?),
            None => None,
        })
    })?;
    let Some(provider) = parent else {
        return Ok(false);
    };
    let head = match run.call(|g, t| g.metadata(t, &provider, &["Message-ID"])) {
        Ok(h) => h,
        Err(GmailError::NotFound) => return Ok(false),
        Err(e) => return Err(Error::Provider(e)),
    };
    let bare = |v: &str| v.trim().trim_matches(['<', '>']).to_ascii_lowercase();
    Ok(head
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("Message-ID"))
        .is_some_and(|h| bare(&h.value) == bare(&reply.in_reply_to)))
}

/// The claim of a change (section 2, step 4): in one transaction, the
/// gate, the approval, the checks of section 1 again and the ledger row.
/// The row is in `writing` with `call` 1, or for a no-op change
/// `applied`, with `call` 0. A no-op and an undo (no `pre`) have no undo.
fn claim(
    run: &Run<'_, '_>,
    action: i64,
    draft: i64,
    ops: &Ops,
    post: &Stored,
    pre: Option<&Pre>,
    noop: bool,
) -> Result<Option<std::result::Result<i64, Outcome>>> {
    let account = run.account;
    let ops = serde_json::to_string(ops).expect("plain data");
    let post = serde_json::to_string(post).expect("plain data");
    let pre = pre.map(|p| serde_json::to_string(p).expect("plain data"));
    Ok(run.store.write(|tx| {
        if let Some(o) = super::super::gate(tx, action)? {
            return Ok(Some(Err(o)));
        }
        let approved: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM changeset_action WHERE id = ?1 AND state = 'approved')",
            [action],
            |r| r.get(0),
        )?;
        if !approved {
            return Ok(None);
        }
        let t = now();
        if let Some(why) =
            iris_changeset::change_denied(tx, action, draft, t).map_err(changeset_error)?
        {
            deny_in(tx, action, &why, t)?;
            return Ok(Some(Err(Outcome::Denied(why))));
        }
        let undo = if pre.is_some() { "inverse" } else { "none" };
        tx.execute(
            "INSERT INTO ledger(action_id, account_id, ops, pre, post, state, undo, at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'writing', ?6, ?7)",
            params![action, account, ops, pre, post, undo, t],
        )?;
        let ledger = tx.last_insert_rowid();
        // The call starts now, so the claim records it before the call.
        // A no-op change makes no call and ends here.
        let set = if noop {
            "state = 'applied'"
        } else {
            "call = 1"
        };
        tx.execute(&format!("UPDATE ledger SET {set} WHERE id = ?1"), [ledger])?;
        Ok(Some(Ok(ledger)))
    })?)
}

fn deny_in(tx: &Connection, action: i64, why: &str, t: i64) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE changeset_action SET state = 'denied', decided_by = 'engine', rule = NULL,
           policy = NULL, reason = ?2, decided_at = ?3
         WHERE id = ?1 AND state = 'approved'",
        params![action, why, t],
    )?;
    Ok(())
}

/// The draft `id` at Gmail, with the message `raw`, after the call or in
/// recovery. Its hash is the new hash of the ops: the call applied (step
/// 6). Else the row is `failed` with `why` ([`DURING`] after a call that
/// applied, [`UNKNOWN`] in recovery), and the draft row refreshes from
/// Gmail and keeps the draft ID `id` (sections 4 and 7).
fn settle(run: &mut Run<'_, '_>, ledger: i64, id: &str, raw: &[u8], why: &str) -> Result<Outcome> {
    let (ops, post) = row(run.store, ledger)?;
    let Ops::Update {
        draft,
        message_id,
        date,
        hash,
        ..
    } = ops
    else {
        unreachable!("a change row")
    };
    let read = iris_compose::read(raw, &run.address);
    let applied = read
        .as_ref()
        .is_some_and(|r| iris_compose::hash(&r.content, r.reply.as_ref()) == hash);
    let new_ids = run.mail.sets_draft_message_id();
    let t = now();
    if !applied {
        run.store.write(|tx| {
            let n = tx.execute(
                "UPDATE ledger SET state = 'failed', error = ?2 WHERE id = ?1 AND state = 'writing'",
                params![ledger, why],
            )?;
            if n == 1 {
                // Each later read and send uses the ID that Gmail gave last.
                tx.execute(
                    "UPDATE draft SET provider_id = ?2 WHERE id = ?1",
                    params![draft, id],
                )?;
                refresh(tx, draft, &message_id, read, raw, new_ids, t)?;
            }
            Ok(())
        })?;
        return Ok(Outcome::Failed {
            counts: Default::default(),
            error: why.into(),
        });
    }
    let read = read.expect("applied");
    let post: Stored = serde_json::from_str(&post.expect("a change has a post")).expect("ours");
    // The Message-ID follows the read back where Gmail sets its own ID,
    // so the release sends and the send recovery searches the same ID.
    let message_id = if new_ids { read.message_id } else { message_id };
    let files = files(run.store, &post.content)?.ok_or_else(|| {
        Error::Store(iris_store::Error::Query(
            "an attachment file of a changed draft is missing or changed".into(),
        ))
    })?;
    let raw = compose_as(&run.address, &message_id, date, &post, &files);
    let content = serde_json::to_string(&post).expect("plain data");
    run.store.write(|tx| {
        let n = tx.execute(
            "UPDATE ledger SET state = 'applied' WHERE id = ?1 AND state = 'writing'",
            [ledger],
        )?;
        if n == 1 {
            tx.execute(
                "UPDATE draft SET provider_id = ?2, message_id_hdr = ?3, content = ?4, hash = ?5,
                   raw = ?6, updated_at = ?7
                 WHERE id = ?1",
                params![draft, id, message_id, content, hash, raw, t],
            )?;
            // An undo that applied: the change that it undoes is `undone`.
            tx.execute(
                "UPDATE ledger SET state = 'undone', undone_by = ?1
                 WHERE id = (SELECT u.ledger_id FROM ledger_undo u
                             JOIN ledger n ON n.action_id = u.action_id WHERE n.id = ?1)
                   AND state = 'applied'",
                [ledger],
            )?;
        }
        Ok(())
    })?;
    Ok(Outcome::Applied(super::super::Counts {
        changed: 1,
        ..Default::default()
    }))
}

/// Recovery of a change in `writing` (section 4). `at` is the time of the
/// ledger row. It never calls `drafts.update`.
pub(super) fn recover(run: &mut Run<'_, '_>, ledger: i64, at: i64) -> Result<Outcome> {
    let (ops, _) = row(run.store, ledger)?;
    let Ops::Update {
        provider_id,
        message_id,
        hash,
        ..
    } = ops
    else {
        unreachable!("a change row")
    };
    // A read that fails: wait for a later run, and fail after 24 hours.
    let read_failed = |run: &Run<'_, '_>| -> Result<Outcome> {
        if now() - at >= UNKNOWN_MS {
            end(run, ledger, "failed", Some(UNKNOWN.into()), false)
        } else {
            Ok(Outcome::Pending)
        }
    };
    match run.call(|g, t| g.draft_raw(t, &provider_id)) {
        Ok(d) => settle(
            run,
            ledger,
            &provider_id,
            &d.raw.unwrap_or_default(),
            UNKNOWN,
        ),
        // Gmail can give the changed draft a new ID: look for it by the
        // Message-ID. Exactly one draft with the new hash is the change. One
        // draft only is the draft of the row in each case: `settle` stores
        // its ID and content, also when the person changed it (issue #1001).
        Err(GmailError::NotFound) => {
            let Ok(found) = run.call(|g, t| g.drafts_with_id(t, &message_id)) else {
                return read_failed(run);
            };
            // Keep at most two messages in memory: the first draft, and the
            // first draft with the new hash.
            let (mut count, mut first, mut hits, mut hit) = (0, None, 0, None);
            for f in found {
                let Ok(back) = run.call(|g, t| g.draft_raw(t, &f.id)) else {
                    return read_failed(run);
                };
                let raw = back.raw.unwrap_or_default();
                count += 1;
                if iris_compose::read(&raw, &run.address)
                    .is_some_and(|r| iris_compose::hash(&r.content, r.reply.as_ref()) == hash)
                {
                    hits += 1;
                    if hit.is_none() {
                        hit = Some((f.id.clone(), raw.clone()));
                    }
                }
                if first.is_none() {
                    first = Some((f.id, raw));
                }
            }
            let one = match (count, hits) {
                (1, _) => first,
                (_, 1) => hit,
                _ => None,
            };
            match one {
                Some((id, raw)) => settle(run, ledger, &id, &raw, UNKNOWN),
                None => end(run, ledger, "failed", Some(UNKNOWN.into()), false),
            }
        }
        Err(_) => read_failed(run),
    }
}
