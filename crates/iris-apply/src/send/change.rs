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
/// Recovery cannot show that the call applied (section 4, step 3).
pub const UNKNOWN: &str = "Iris does not know if the draft changed. Look at it in Gmail.";

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
/// (section 2, steps 1 to 8).
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
    // A reply keeps the subject of its draft (section 1), as at submit.
    if d.thread.is_some() {
        content.subject = old.content.subject.clone();
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
        return deny("an attachment file is missing or changed");
    };
    let hash = iris_compose::hash(&content, old.reply.as_ref());
    let post = Stored {
        content,
        reply: old.reply.clone(),
        name: owner_name(store, run.account, &run.address)?,
    };
    let date = now() / 1000;
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
        .is_some_and(|r| iris_compose::hash(&r.content, r.reply.as_ref()) == d.hash);
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
        pre_hash: d.hash.clone(),
        hash: hash.clone(),
        thread: d.thread.clone(),
    };
    // The same content again: no call, a final entry (section 2, step 3).
    let noop = hash == d.hash;
    let ledger = match claim(run, action, d.id, &ops, &post, &pre, noop)? {
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
                Ok(back) => settle(run, ledger, &new.id, &back.raw.unwrap_or_default()),
                Err(_) => Ok(Outcome::Pending),
            }
        }
        Err(e) if e.write_result_unknown() => recover(run, ledger, now()),
        // Step 7: a definite rejection. The draft row does not change.
        Err(e) => end(run, ledger, "failed", Some(e.to_string()), false),
    }
}

/// The claim of a change (section 2, step 4): in one transaction, the
/// gate, the approval, the checks of section 1 again and the ledger row.
/// The row is in `writing` with `call` 1, or for a no-op change
/// `applied`, with `call` 0 and no undo.
fn claim(
    run: &Run<'_, '_>,
    action: i64,
    draft: i64,
    ops: &Ops,
    post: &Stored,
    pre: &Pre,
    noop: bool,
) -> Result<Option<std::result::Result<i64, Outcome>>> {
    let account = run.account;
    let ops = serde_json::to_string(ops).expect("plain data");
    let post = serde_json::to_string(post).expect("plain data");
    let pre = (!noop).then(|| serde_json::to_string(pre).expect("plain data"));
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
        let undo = if noop { "none" } else { "inverse" };
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
/// 6). Else the row is `failed` with [`UNKNOWN`], and the draft row
/// refreshes from Gmail (section 4, step 3).
fn settle(run: &mut Run<'_, '_>, ledger: i64, id: &str, raw: &[u8]) -> Result<Outcome> {
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
                params![ledger, UNKNOWN],
            )?;
            if n == 1 {
                refresh(tx, draft, &message_id, read, raw, new_ids, t)?;
            }
            Ok(())
        })?;
        return Ok(Outcome::Failed {
            counts: Default::default(),
            error: UNKNOWN.into(),
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
    match run.call(|g, t| g.draft_raw(t, &provider_id)) {
        Ok(d) => settle(run, ledger, &provider_id, &d.raw.unwrap_or_default()),
        // Gmail can give the changed draft a new ID: look for it by the
        // Message-ID. Exactly one draft with the new hash is the change.
        Err(GmailError::NotFound) => {
            let found = run
                .call(|g, t| g.drafts_with_id(t, &message_id))
                .map_err(Error::Provider)?;
            let mut hits = Vec::new();
            for f in found {
                let back = run
                    .call(|g, t| g.draft_raw(t, &f.id))
                    .map_err(Error::Provider)?;
                let raw = back.raw.unwrap_or_default();
                let same = iris_compose::read(&raw, &run.address)
                    .is_some_and(|r| iris_compose::hash(&r.content, r.reply.as_ref()) == hash);
                if same {
                    hits.push((f.id, raw));
                }
            }
            match hits.as_slice() {
                [(id, raw)] => settle(run, ledger, id, raw),
                _ => end(run, ledger, "failed", Some(UNKNOWN.into()), false),
            }
        }
        Err(_) if now() - at >= UNKNOWN_MS => {
            end(run, ledger, "failed", Some(UNKNOWN.into()), false)
        }
        Err(_) => Ok(Outcome::Pending),
    }
}
