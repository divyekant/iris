//! The attention of each item (spec 2, sections 6.3 and 6.4): the `hide` or
//! `bundle` rule with the lowest ID that matches the item and its origin
//! message, written to the `attention` field of the item document. The
//! engine evaluates each item again when the active policy changes, and the
//! items of each message in `attention_due` when a fact of that message
//! changes (store migration 8).

use crate::{CommitmentKind, Form, Item, Message, Policy, RecordType, SenderKind};
use iris_store::{Result, Store};
use rusqlite::{params, Connection, OptionalExtension};

/// The longest `reason` of the schema (`docs/spec/schema/common.schema.json`).
const REASON_MAX: usize = 200;

/// Writes the attention of each item that has an origin message, from
/// `policy`, at the time `now` (seconds since 1970-01-01 UTC). A hidden item
/// stays in the store: only the brief leaves it out.
pub fn evaluate(tx: &Connection, policy: &Policy, now: i64) -> Result<()> {
    // One message at a time, so memory holds one ID for each message, not the items.
    let messages: Vec<i64> = tx
        .prepare("SELECT DISTINCT message_id FROM item_source WHERE role = 'origin'")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    evaluate_messages(tx, policy, now, &messages)?;
    // Each item is current now.
    tx.execute("DELETE FROM attention_due", [])?;
    tx.execute(
        "INSERT INTO attention_pass(id, at) VALUES (1, ?1)
         ON CONFLICT(id) DO UPDATE SET at = excluded.at",
        [now],
    )?;
    Ok(())
}

/// The most messages that one transaction of [`refresh`] evaluates, so that a
/// sync does not wait long for the write lock.
const BATCH: i64 = 1000;

/// The time between two evaluations of each item by [`refresh`], in seconds.
pub const PASS_EVERY: i64 = 3600;

/// Writes the attention of the items of each message in `attention_due`,
/// with the active policy, at the time `now` (seconds since 1970-01-01 UTC),
/// in transactions of at most [`BATCH`] messages. If the last evaluation of
/// each item is [`PASS_EVERY`] or more from `now`, or there is none, first
/// marks each message: a rule with `older_than_days` matches more items as
/// time goes on. Gives the number of messages.
pub fn refresh(store: &Store, now: i64) -> Result<u64> {
    store.write(|tx| {
        let last: Option<i64> = tx
            .query_row("SELECT at FROM attention_pass WHERE id = 1", [], |r| {
                r.get(0)
            })
            .optional()?;
        // Also a clock that went back.
        if last.is_none_or(|at| (now - at).abs() >= PASS_EVERY) {
            tx.execute(
                "INSERT OR IGNORE INTO attention_due(message_id)
                 SELECT DISTINCT message_id FROM item_source WHERE role = 'origin'",
                [],
            )?;
            tx.execute(
                "INSERT INTO attention_pass(id, at) VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET at = excluded.at",
                [now],
            )?;
        }
        Ok(())
    })?;
    let mut done = 0;
    loop {
        let n = store.write(|tx| {
            let policy = crate::active(tx)?.policy;
            let ids: Vec<i64> = tx
                .prepare("SELECT message_id FROM attention_due ORDER BY message_id LIMIT ?1")?
                .query_map([BATCH], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            evaluate_messages(tx, &policy, now, &ids)?;
            let mut clear = tx.prepare("DELETE FROM attention_due WHERE message_id = ?1")?;
            for id in &ids {
                clear.execute([id])?;
            }
            Ok(ids.len() as i64)
        })?;
        done += n as u64;
        if n < BATCH {
            return Ok(done);
        }
    }
}

/// Writes the attention of the items whose origin message is in `messages`.
fn evaluate_messages(tx: &Connection, policy: &Policy, now: i64, messages: &[i64]) -> Result<()> {
    // SQLite makes the JSON of the field. Only an item whose attention
    // changes gets a write, with the time of the change (spec 1 section 6.1).
    let mut write = tx.prepare(
        "WITH a(v) AS (SELECT CASE WHEN ?3 IS NULL THEN json_object('state', ?2)
                       ELSE json_object('state', ?2, 'rule', ?3, 'reason', ?4) END)
         UPDATE item SET updated_at = ?5 * 1000,
           doc = json_set(doc, '$.attention', json((SELECT v FROM a)),
                          '$.updated', strftime('%Y-%m-%dT%H:%M:%fZ', ?5, 'unixepoch'))
         WHERE id = ?1 AND coalesce(json_extract(doc, '$.attention'), '') <> (SELECT v FROM a)",
    )?;
    for &id in messages {
        let Some(f) = facts(tx, id, now)? else {
            continue;
        };
        let msg = f.message();
        for (item, it) in f.item_ids.iter().zip(&f.items) {
            let (state, rule, reason) = field(policy, policy.attention(it, &msg, now));
            write.execute(params![item, state, rule, reason, now])?;
        }
    }
    Ok(())
}

/// The facts of one message that a rule reads (spec 2, section 6.2), owned,
/// so that the decision of an action and the attention of an item read the
/// same facts.
#[derive(Clone, Debug)]
pub struct Facts {
    pub sender: String,
    pub sender_kind: SenderKind,
    pub hostile: bool,
    pub labels: Vec<String>,
    pub date: i64,
    /// The items whose origin is the message, with their row IDs in
    /// `item_ids`, in the same order.
    pub items: Vec<Item>,
    pub item_ids: Vec<i64>,
}

impl Facts {
    pub fn message(&self) -> Message<'_> {
        Message {
            sender: &self.sender,
            sender_kind: self.sender_kind,
            hostile: self.hostile,
            labels: &self.labels,
            date: self.date,
            items: &self.items,
        }
    }
}

/// The facts of message row `id` at the time `now` (seconds since
/// 1970-01-01 UTC). `None` if the message does not exist.
pub fn facts(tx: &Connection, id: i64, now: i64) -> Result<Option<Facts>> {
    let mut head = tx.prepare_cached(concat!(
        "SELECT h.from_addr, coalesce(h.date, m.internal_date), ",
        iris_store::may_be_hostile!(),
        " FROM message m LEFT JOIN header h ON h.message_id = m.id WHERE m.id = ?1",
    ))?;
    // The rule of the local API: no row or no kind is a stranger, and of two
    // rows that differ only in case, the most cautious kind wins.
    let mut kind = tx.prepare_cached(
        "SELECT coalesce(kind, 'stranger') AS k FROM sender WHERE address = ?1 COLLATE NOCASE
         ORDER BY CASE k WHEN 'hostile' THEN 0 WHEN 'stranger' THEN 1
                  WHEN 'service' THEN 2 WHEN 'person' THEN 3 ELSE 4 END
         LIMIT 1",
    )?;
    let mut labels = tx.prepare_cached(
        "SELECT l.name, l.provider_id FROM message_label ml JOIN label l ON l.id = ml.label_id
         WHERE ml.message_id = ?1",
    )?;
    let mut items = tx.prepare_cached(
        "SELECT i.id, i.form, json_extract(i.doc, '$.type'), json_extract(i.doc, '$.kind')
         FROM item_source s JOIN item i ON i.id = s.item_id
         WHERE s.message_id = ?1 AND s.role = 'origin'",
    )?;
    let Some((sender, date, hostile)) = head
        .query_row([id], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?
                    .unwrap_or_default()
                    .to_ascii_lowercase(), // One form with migration 39.
                // No date: age 0, so that no `older_than_days` rule hides it.
                r.get::<_, Option<i64>>(1)?
                    .map_or(now, |ms| ms.div_euclid(1000)),
                r.get::<_, bool>(2)?,
            ))
        })
        .optional()?
    else {
        return Ok(None);
    };
    let mut own = false;
    let names: Vec<String> = labels
        .query_map([id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .map(|l| {
            l.map(|(name, provider)| {
                own |= provider == "SENT" || provider == "DRAFT";
                name
            })
        })
        .collect::<rusqlite::Result<_>>()?;
    // Spec 1 section 6.7: only the SENT or DRAFT label makes mail from
    // you. A hostile mark still wins (`Message::kind`).
    let sender_kind = match sender.as_str() {
        _ if own => SenderKind::SelfOwner,
        "" => SenderKind::Stranger,
        a => kind
            .query_row([a], |r| r.get::<_, String>(0))
            .optional()?
            .and_then(|k| SenderKind::from_name(&k))
            .unwrap_or(SenderKind::Stranger),
    };
    let rows: Vec<(i64, Item)> = items
        .query_map([id], |r| {
            let form: String = r.get(1)?;
            let kind: Option<String> = r.get(2)?;
            let commitment: Option<String> = r.get(3)?;
            Ok((r.get(0)?, form, kind, commitment))
        })?
        .filter_map(|row| {
            let (item, form, kind, commitment) = match row {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            // An item of an unknown form cannot match a condition; skip it.
            let form = Form::from_name(&form)?;
            let pick = |want: Form| if form == want { Some(()) } else { None };
            Some(Ok((
                item,
                Item {
                    form,
                    record_type: pick(Form::Record)
                        .and(kind.as_deref())
                        .and_then(RecordType::from_name),
                    commitment_kind: pick(Form::Commitment)
                        .and(commitment.as_deref())
                        .and_then(CommitmentKind::from_name),
                },
            )))
        })
        .collect::<rusqlite::Result<_>>()?;
    let (item_ids, items) = rows.into_iter().unzip();
    Ok(Some(Facts {
        sender,
        sender_kind,
        hostile,
        labels: names,
        date,
        items,
        item_ids,
    }))
}

/// The `attention` field of the schema: the state, and the rule with a
/// reason for `hidden` and `bundled`.
fn field(policy: &Policy, a: crate::Attention) -> (&'static str, Option<u32>, Option<String>) {
    let (state, verb, rule) = match a {
        crate::Attention::Shown => return ("shown", None, None),
        crate::Attention::Hidden { rule } => ("hidden", "Hidden", rule),
        crate::Attention::Bundled { rule } => ("bundled", "Bundled", rule),
    };
    let name = policy
        .rules
        .iter()
        .find(|r| r.id == rule)
        .map_or("", |r| r.name.as_str());
    let mut reason = format!("{verb} by rule {rule}: {name}");
    if reason.chars().count() > REASON_MAX {
        reason = reason.chars().take(REASON_MAX - 3).collect::<String>() + "...";
    }
    (state, Some(rule), Some(reason))
}
