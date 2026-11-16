//! The attention of each item (spec 2, sections 6.3 and 6.4): the `hide` or
//! `bundle` rule with the lowest ID that matches the item and its origin
//! message, written to the `attention` field of the item document. The
//! engine evaluates each item again when the active policy changes.

use crate::{CommitmentKind, Form, Item, Message, Policy, RecordType, SenderKind};
use iris_store::Result;
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
    let mut head = tx.prepare(
        "SELECT h.from_addr, coalesce(h.date, m.internal_date),
                m.hostile IS NOT NULL AND m.hostile NOT IN ('', '[]')
         FROM message m LEFT JOIN header h ON h.message_id = m.id WHERE m.id = ?1",
    )?;
    // The rule of the local API: no row or no kind is a stranger, and of two
    // rows that differ only in case, the most cautious kind wins.
    let mut kind = tx.prepare(
        "SELECT coalesce(kind, 'stranger') AS k FROM sender WHERE address = ?1 COLLATE NOCASE
         ORDER BY CASE k WHEN 'hostile' THEN 0 WHEN 'stranger' THEN 1
                  WHEN 'service' THEN 2 WHEN 'person' THEN 3 ELSE 4 END
         LIMIT 1",
    )?;
    let mut labels = tx.prepare(
        "SELECT l.name FROM message_label ml JOIN label l ON l.id = ml.label_id
         WHERE ml.message_id = ?1",
    )?;
    let mut items = tx.prepare(
        "SELECT i.id, i.form, json_extract(i.doc, '$.type'), json_extract(i.doc, '$.kind')
         FROM item_source s JOIN item i ON i.id = s.item_id
         WHERE s.message_id = ?1 AND s.role = 'origin'",
    )?;
    // Only an item whose attention changes gets a write.
    let mut write = tx.prepare(
        "UPDATE item SET doc = json_set(doc, '$.attention', json(?2))
         WHERE id = ?1 AND coalesce(json_extract(doc, '$.attention'), '') <> json(?2)",
    )?;
    for id in messages {
        let Some((sender, date, hostile)) = head
            .query_row([id], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?
                        .unwrap_or_default()
                        .to_lowercase(),
                    // No date: age 0, so that no `older_than_days` rule hides it.
                    r.get::<_, Option<i64>>(1)?
                        .map_or(now, |ms| ms.div_euclid(1000)),
                    r.get::<_, bool>(2)?,
                ))
            })
            .optional()?
        else {
            continue;
        };
        let sender_kind = match sender.as_str() {
            "" => SenderKind::Stranger,
            a => kind
                .query_row([a], |r| r.get::<_, String>(0))
                .optional()?
                .and_then(|k| SenderKind::from_name(&k))
                .unwrap_or(SenderKind::Stranger),
        };
        let names: Vec<String> = labels
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
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
        let all: Vec<Item> = rows.iter().map(|(_, i)| *i).collect();
        let msg = Message {
            sender: &sender,
            sender_kind,
            hostile,
            labels: &names,
            date,
            items: &all,
        };
        for (item, it) in &rows {
            let json = attention_json(policy, policy.attention(it, &msg, now));
            write.execute(params![item, json])?;
        }
    }
    Ok(())
}

/// The `attention` field of the schema, as JSON text.
fn attention_json(policy: &Policy, a: crate::Attention) -> String {
    let (state, rule) = match a {
        crate::Attention::Shown => return r#"{"state":"shown"}"#.into(),
        crate::Attention::Hidden { rule } => ("hidden", rule),
        crate::Attention::Bundled { rule } => ("bundled", rule),
    };
    let name = policy
        .rules
        .iter()
        .find(|r| r.id == rule)
        .map_or("", |r| r.name.as_str());
    let verb = if state == "hidden" {
        "Hidden"
    } else {
        "Bundled"
    };
    let mut reason = format!("{verb} by rule {rule}: {name}");
    if reason.chars().count() > REASON_MAX {
        reason = reason.chars().take(REASON_MAX - 3).collect::<String>() + "...";
    }
    format!(
        r#"{{"state":"{state}","rule":{rule},"reason":{}}}"#,
        json_string(&reason)
    )
}

/// A JSON string literal of `s`.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
