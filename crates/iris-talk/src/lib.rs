//! The conversation of one mail thread (specification 1, sections 6.3 and
//! 7.3).
//!
//! [`update`] computes the conversation of one thread again from the store
//! and saves it as an item of the form `conversation`. The understanding step
//! calls it in its own transaction (section 7.1, steps 4 and 8), and again
//! when a message of the thread gets a hostile mark or is gone.
//!
//! The function reads the header row, the clean text (`body`), the labels,
//! the hostile marks and the sender kind of each message. It sends nothing
//! to a model and to the network. Mail text is data: the function only cuts
//! and copies it.

use iris_store::{format_id, Kind};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};

/// The name of the rule in `derived.rule`.
pub const RULE: &str = "thread";

/// The fields of a conversation that this rule makes from the store on each
/// update. A correction cannot change them (only `summary` is left), and
/// [`update`] does not apply one.
pub const DERIVED: [&str; 7] = [
    "thread",
    "participants",
    "role",
    "latest",
    "messages",
    "new",
    "your_turn",
];
/// A conversation with no message for this time is `quiet` (section 6.3).
pub const QUIET_MS: i64 = 14 * 24 * 60 * 60 * 1000;
/// The most characters of `latest.snippet`.
pub const SNIPPET_CHARS: usize = 200;
const TITLE_CHARS: usize = 200;
const QUOTE_CHARS: usize = 500;
const NAME_CHARS: usize = 200;
const NO_SUBJECT: &str = "(no subject)";

/// One understood message of a thread, with the rule of the conversation.
pub struct Mail {
    pub id: i64,
    pub at: i64,
    /// Lowercase.
    pub from: String,
    pub from_name: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub text: String,
    pub mine: bool,
    pub service: bool,
    pub gone: bool,
    /// Not a draft and not hostile mail. A message counts when it is clean,
    /// not gone, and from you or from an address.
    pub clean: bool,
}

/// The conversation item of a thread, as the store has it.
struct Existing {
    id: i64,
    status: String,
    before_gone: Option<String>,
    created: i64,
    doc: Value,
    origin: i64,
}

/// Compute the conversation of the thread with row ID `thread` again, and
/// save it in the item table. Give the item row ID, or `None` if the thread
/// has no conversation.
///
/// The conversation counts each message of the thread that is understood
/// (`understood_at` is set), or is `current` (the message in step 4 now). A
/// message that is gone, a draft or hostile mail does not count. The first
/// message from you, a person or a stranger makes the conversation, and it
/// stays the origin. A sender with no kind yet counts as a stranger
/// (section 6.7: a sender that matches no rule is a stranger).
///
/// The function writes nothing when the conversation did not change. When no
/// message counts any more, the item gets the status `gone`. It is never
/// deleted. Run it inside the transaction that sets `understood_at`.
pub fn update(
    conn: &Connection,
    thread: i64,
    current: Option<i64>,
    now: i64,
) -> rusqlite::Result<Option<i64>> {
    let (account, me, viewed_at): (i64, String, Option<i64>) = conn.query_row(
        "SELECT a.id, a.address, t.viewed_at FROM thread t JOIN account a ON a.id = t.account_id
         WHERE t.id = ?1",
        [thread],
        |r| Ok((r.get(0)?, r.get::<_, String>(1)?.to_lowercase(), r.get(2)?)),
    )?;
    // One query and one rule decide each message, so that the live messages,
    // the gone sources and the gone origin agree.
    let all = mails(conn, thread, current)?;
    let gone_ok: Vec<(i64, i64)> = all
        .iter()
        .filter(|m| m.gone && m.clean)
        .map(|m| (m.at, m.id))
        .collect();
    let origin_only_gone = |origin: i64| all.iter().any(|m| m.id == origin && m.gone && m.clean);
    let mails: Vec<&Mail> = all
        .iter()
        .filter(|m| !m.gone && m.clean && (m.mine || !m.from.is_empty()))
        .collect();
    let existing = existing(conn, thread)?;

    let Some(old) = existing else {
        let Some(origin) = mails.iter().copied().find(|m| !m.service) else {
            return Ok(None);
        };
        let subject = subject_line(origin);
        let (title, quote) = (cut(&subject, TITLE_CHARS), cut(&subject, QUOTE_CHARS));
        let origin = origin.id;
        conn.execute(
            "INSERT INTO item(form, account_id, status, doc, created_at, updated_at, derived_by)
             VALUES ('conversation', ?1, 'active', '{}', ?2, ?2, 'rule')",
            params![account, now],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
            params![id, origin],
        )?;
        let doc = doc(conn, thread, id, &me, viewed_at, now, &mails, None)?;
        let mut doc = doc.expect("the origin message counts");
        let parts = Parts {
            origin,
            title,
            quote,
            created: now,
        };
        let updates = parts.fill(&mut doc, thread, &mails, &[], now);
        save(conn, id, &doc, None, now, &updates)?;
        return Ok(Some(id));
    };

    let parts = Parts {
        origin: old.origin,
        title: old.doc["title"].as_str().unwrap_or(NO_SUBJECT).to_string(),
        quote: old.doc["sources"]
            .as_array()
            .and_then(|s| s.iter().find(|s| s["role"] == "origin"))
            .and_then(|s| s["quote"].as_str())
            .unwrap_or(NO_SUBJECT)
            .to_string(),
        created: old.created,
    };
    let mut parts = parts;
    // The origin does not change, also when it is gone (section 7.3). If a
    // live origin becomes hostile mail or a draft, the first message that
    // counts from you, a person or a stranger becomes the origin.
    let bad_origin = all
        .iter()
        .any(|m| m.id == old.origin && !m.gone && !m.clean);
    if let Some(next) = mails.iter().find(|m| !m.service).filter(|_| bad_origin) {
        let subject = subject_line(next);
        parts.title = cut(&subject, TITLE_CHARS);
        parts.quote = cut(&subject, QUOTE_CHARS);
        parts.origin = next.id;
        conn.execute(
            "DELETE FROM item_source WHERE item_id = ?1 AND message_id = ?2",
            params![old.id, next.id],
        )?;
        conn.execute(
            "UPDATE item_source SET message_id = ?2 WHERE item_id = ?1 AND role = 'origin'",
            params![old.id, next.id],
        )?;
    }
    let mut status = match old.status.as_str() {
        "gone" => old.before_gone.clone(),
        s => Some(s.to_string()),
    };
    if status.as_deref() != Some("muted") {
        status = None;
    }
    // The conversation is gone when each of its messages is gone (section
    // 7.3). It also ends when only service mail is left, because service mail
    // alone makes no conversation. While the origin is only gone, the item
    // keeps its status (section 4.5). The origin row stays through the
    // content cleanup, so the rule does not need its header.
    let gone =
        mails.is_empty() || (mails.iter().all(|m| m.service) && !origin_only_gone(parts.origin));
    let mut doc = match doc(
        conn,
        thread,
        old.id,
        &me,
        viewed_at,
        now,
        &mails,
        status.as_deref(),
    )? {
        Some(d) => d,
        // No message counts: the fields of the last message that counted stay.
        None => old.doc.clone(),
    };
    // A gone source stays (section 4.5) while it is clean.
    let kept = update_sources(conn, old.id)?;
    let kept_gone: Vec<(i64, i64)> = gone_ok
        .into_iter()
        .filter(|g| kept.contains(&g.1))
        .collect();
    let updates = parts.fill(&mut doc, thread, &mails, &kept_gone, now);
    corrections(conn, old.id, &mut doc)?;
    let before = if gone {
        doc["status"] = json!("gone");
        // The sync can set the status column to gone first (section 4.5).
        if old.status == "gone" {
            old.before_gone.clone()
        } else {
            Some(old.status.clone())
        }
    } else {
        None
    };
    // Compare without the time of the change, so a call that changes nothing
    // writes nothing.
    doc["updated"] = old.doc["updated"].clone();
    if doc == old.doc && doc["status"] == old.status.as_str() && before == old.before_gone {
        return Ok(Some(old.id));
    }
    doc["updated"] = json!(rfc3339(now));
    save(conn, old.id, &doc, Some(before), now, &updates)?;
    Ok(Some(old.id))
}

/// The fields that the origin fixes when it makes the conversation.
struct Parts {
    origin: i64,
    title: String,
    quote: String,
    created: i64,
}

impl Parts {
    /// `gone` holds the update sources whose message is gone, as (time, row).
    /// Give the rows of the update sources.
    fn fill(
        &self,
        doc: &mut Value,
        thread: i64,
        mails: &[&Mail],
        gone: &[(i64, i64)],
        now: i64,
    ) -> Vec<i64> {
        let t = format_id(Kind::Thread, thread);
        let mut sources = vec![json!({
            "message": format_id(Kind::Message, self.origin),
            "thread": t,
            "role": "origin",
            "quote": self.quote,
        })];
        let mut updates: Vec<(i64, i64)> = mails.iter().map(|m| (m.at, m.id)).collect();
        updates.extend_from_slice(gone);
        updates.sort_unstable();
        updates.dedup_by_key(|u| u.1);
        updates.retain(|u| u.1 != self.origin);
        sources.extend(updates.iter().map(
            |u| json!({ "message": format_id(Kind::Message, u.1), "thread": t, "role": "update" }),
        ));
        doc["title"] = json!(self.title);
        doc["sources"] = Value::Array(sources);
        doc["created"] = json!(rfc3339(self.created));
        doc["updated"] = json!(rfc3339(now));
        updates.into_iter().map(|u| u.1).collect()
    }
}

fn save(
    conn: &Connection,
    id: i64,
    doc: &Value,
    // `Some(status_before_gone)` for an item that exists, `None` for a new item.
    existed: Option<Option<String>>,
    now: i64,
    updates: &[i64],
) -> rusqlite::Result<()> {
    let before = existed.clone().flatten();
    conn.execute(
        "UPDATE item SET status = ?2, status_before_gone = ?5, doc = ?3, updated_at = ?4
         WHERE id = ?1",
        params![id, doc["status"].as_str(), doc.to_string(), now, before],
    )?;
    if existed.is_some() {
        conn.execute(
            "DELETE FROM item_source WHERE item_id = ?1 AND role <> 'origin'",
            [id],
        )?;
    }
    let mut add = conn.prepare_cached(
        "INSERT OR IGNORE INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'update')",
    )?;
    for m in updates {
        add.execute(params![id, m])?;
    }
    Ok(())
}

/// The rows of the update sources of the item.
fn update_sources(conn: &Connection, item: i64) -> rusqlite::Result<Vec<i64>> {
    let mut q = conn.prepare_cached(
        "SELECT message_id FROM item_source WHERE item_id = ?1 AND role = 'update'",
    )?;
    let rows = q.query_map([item], |r| r.get(0))?;
    rows.collect()
}

/// Apply the corrections of the item (section 7.5), oldest first, so that a
/// change by a person stays. `field` is a dotted path, for example `summary`.
/// The status columns hold a status change, so `status` is not applied here.
fn corrections(conn: &Connection, item: i64, doc: &mut Value) -> rusqlite::Result<()> {
    let mut q = conn.prepare_cached(
        "SELECT field, value FROM correction WHERE item_id = ?1 AND field <> 'status' ORDER BY at, id",
    )?;
    let rows = q.query_map([item], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (field, value) = row?;
        let Ok(value) = serde_json::from_str::<Value>(&value) else {
            continue;
        };
        let top = field.split('.').next().unwrap_or("");
        if DERIVED.contains(&top) {
            continue;
        }
        // A path that does not fit this document changes nothing.
        set_field(doc, &field, value);
    }
    Ok(())
}

/// Set the field at the dotted path `field` (for example `due.date` or
/// `fields.items.0.name`) of `doc` to `value`. The one rule that applies a
/// correction, for each caller.
///
/// A step into an object names a key; a missing key before the last step
/// becomes an empty object. A step into an array is the decimal index of an
/// element (`0`, not `00`). Gives false, and leaves `doc` as it was, for an
/// empty step, an index that is not an element, or a step into a value that
/// is not an object or an array. So no array or object changes its kind.
pub fn set_field(doc: &mut Value, field: &str, value: Value) -> bool {
    let steps: Vec<&str> = field.split('.').collect();
    if steps.iter().any(|s| s.is_empty()) {
        return false;
    }
    // On a copy: a step that fails after a new empty object changes nothing.
    let mut new = doc.clone();
    let mut at = &mut new;
    for (i, key) in steps.iter().enumerate() {
        let last = i + 1 == steps.len();
        let next = match at {
            Value::Object(m) => {
                Some(
                    m.entry(*key)
                        .or_insert_with(|| if last { Value::Null } else { json!({}) }),
                )
            }
            Value::Array(a) => key
                .parse::<usize>()
                .ok()
                .filter(|n| n.to_string() == *key)
                .and_then(|n| a.get_mut(n)),
            _ => None,
        };
        let Some(next) = next else { return false };
        at = next;
    }
    *at = value;
    *doc = new;
    true
}

/// One understood message, also when it is gone, or `None`. A message is
/// for a reader when it is `clean` and not `gone`: then it is not a draft
/// and not hostile mail (specification 1, section 6.7). It reads one body,
/// so a caller can go through a large thread with bounded memory.
pub fn message(conn: &Connection, id: i64) -> rusqlite::Result<Option<Mail>> {
    let mut q = conn.prepare_cached(&format!(
        "{MAIL_SELECT} WHERE m.id = ?1 AND m.understood_at IS NOT NULL"
    ))?;
    q.query_row([id], |r| mail_of(conn, r)).optional()
}

/// The columns that [`mail_of`] reads.
const MAIL_SELECT: &str =
    "SELECT m.id, COALESCE(m.internal_date, h.date, 0) AS at, h.from_addr, h.from_name,
            h.to_addrs, h.cc_addrs, COALESCE(b.subject, h.subject), b.text, m.hostile,
            EXISTS(SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                   WHERE ml.message_id = m.id AND l.provider_id = 'SENT'),
            EXISTS(SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                   WHERE ml.message_id = m.id AND l.provider_id = 'DRAFT'),
            m.gone_at IS NOT NULL
     FROM message m LEFT JOIN header h ON h.message_id = m.id
     LEFT JOIN body b ON b.message_id = m.id";

/// Each message of the thread that is understood or `current`, also when it
/// is gone, oldest first. The content cleanup can delete the header of a gone
/// message: then only its row decides.
fn mails(conn: &Connection, thread: i64, current: Option<i64>) -> rusqlite::Result<Vec<Mail>> {
    let mut q = conn.prepare_cached(&format!(
        "{MAIL_SELECT} WHERE m.thread_id = ?1
           AND (m.understood_at IS NOT NULL OR m.id IS ?2)
         ORDER BY at, m.id"
    ))?;
    let rows = q.query_map(params![thread, current], |r| mail_of(conn, r))?;
    rows.collect()
}

/// One row of [`MAIL_SELECT`] as a [`Mail`].
fn mail_of(conn: &Connection, r: &rusqlite::Row) -> rusqlite::Result<Mail> {
    let hostile: Option<String> = r.get(8)?;
    let sent: bool = r.get(9)?;
    let draft: bool = r.get(10)?;
    let gone: bool = r.get(11)?;
    let from = r
        .get::<_, Option<String>>(2)?
        .and_then(|f| addresses(&f).into_iter().next())
        // A From that is not an address gives no party.
        .unwrap_or_default();
    // The kind of the parsed address: sender rows hold bare addresses.
    let kind: Option<String> = conn
        .prepare_cached("SELECT kind FROM sender WHERE address = ?1")?
        .query_row([&from], |r| r.get(0))
        .optional()?
        .flatten();
    // Only the label SENT makes a message yours: anyone can write the
    // mailbox address in From (spec 1 section 6.7).
    let mine = sent;
    let hostile = marked(hostile.as_deref()) || (!mine && kind.as_deref() == Some("hostile"));
    let mail = Mail {
        id: r.get(0)?,
        at: r.get(1)?,
        from_name: r
            .get::<_, Option<String>>(3)?
            .map(|n| decode_name(&n))
            .filter(|n| !n.trim().is_empty()),
        to: addresses(r.get::<_, Option<String>>(4)?.as_deref().unwrap_or("")),
        cc: addresses(r.get::<_, Option<String>>(5)?.as_deref().unwrap_or("")),
        subject: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
        text: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
        service: !mine && kind.as_deref() == Some("service"),
        mine,
        from,
        gone,
        clean: !draft && !hostile,
    };
    Ok(mail)
}

/// True if the `hostile` column holds one or more marks. A value that is not
/// a JSON list counts as marked, so a bad value never shows hostile mail.
fn marked(hostile: Option<&str>) -> bool {
    match hostile.map(str::trim) {
        None | Some("") => false,
        Some(v) => serde_json::from_str::<Vec<Value>>(v).map_or(true, |l| !l.is_empty()),
    }
}

fn existing(conn: &Connection, thread: i64) -> rusqlite::Result<Option<Existing>> {
    conn.query_row(
        "SELECT i.id, i.status, i.status_before_gone, i.created_at, i.doc, s.message_id
         FROM item i JOIN item_source s ON s.item_id = i.id AND s.role = 'origin'
         JOIN message m ON m.id = s.message_id
         WHERE i.form = 'conversation' AND m.thread_id = ?1
         ORDER BY i.id LIMIT 1",
        [thread],
        |r| {
            let doc: String = r.get(4)?;
            Ok(Existing {
                id: r.get(0)?,
                status: r.get(1)?,
                before_gone: r.get(2)?,
                created: r.get(3)?,
                doc: serde_json::from_str(&doc).unwrap_or(Value::Null),
                origin: r.get(5)?,
            })
        },
    )
    .optional()
}

/// The fields of the conversation that come from the messages. `status` is
/// the status to keep (`muted`), or `None` for `active` or `quiet`.
#[allow(clippy::too_many_arguments)]
fn doc(
    conn: &Connection,
    thread: i64,
    id: i64,
    me: &str,
    viewed_at: Option<i64>,
    now: i64,
    mails: &[&Mail],
    status: Option<&str>,
) -> rusqlite::Result<Option<Value>> {
    let Some(last) = mails.last() else {
        return Ok(None);
    };
    // The From of a sent message can be an alias: it is you, not a party.
    let aliases: Vec<&str> = mails
        .iter()
        .filter(|m| m.mine)
        .map(|m| m.from.as_str())
        .collect();
    let mut order: Vec<&str> = Vec::new();
    for m in mails {
        let from = (!m.mine).then_some(m.from.as_str());
        for a in from
            .into_iter()
            .chain(m.to.iter().map(String::as_str))
            .chain(m.cc.iter().map(String::as_str))
        {
            // An alias is you in the From of sent mail and in To or Cc. A
            // received message from the same address, or from the mailbox
            // address with no SENT label, is from a party.
            let from_party = from == Some(a);
            if (from_party || (a != me && !aliases.contains(&a))) && !order.contains(&a) {
                order.push(a);
            }
        }
    }
    let mut participants = vec![json!({ "kind": "self" })];
    let mut parties = Map::new();
    for a in &order {
        // The newest display name that the address gave in a From.
        let name = mails
            .iter()
            .rev()
            .find(|m| m.from == *a)
            .and_then(|m| m.from_name.as_deref());
        let p = party(conn, a, name)?;
        parties.insert(a.to_string(), p.clone());
        participants.push(p);
    }

    let sent = mails.iter().any(|m| m.mine);
    // You are the account address and each alias of your sent mail.
    let is_me = |a: &String| a == me || aliases.contains(&a.as_str());
    let in_to = mails.iter().any(|m| m.to.iter().any(is_me));
    let in_cc = mails.iter().any(|m| m.cc.iter().any(is_me));
    let role = if !sent && !in_to && in_cc {
        "cc"
    } else if order.len() >= 2 {
        "group"
    } else {
        "direct"
    };

    let from = if last.mine {
        json!({ "kind": "self" })
    } else {
        parties[&last.from].clone()
    };
    let earlier: Vec<&str> = mails[..mails.len() - 1]
        .iter()
        .filter(|m| m.from == last.from)
        .map(|m| m.text.as_str())
        .collect();
    let text = iris_clean::remove_repeated_end(&last.text, &earlier);
    let status = status.unwrap_or(if now - last.at >= QUIET_MS {
        "quiet"
    } else {
        "active"
    });

    Ok(Some(json!({
        "model": "iris/0",
        "id": format_id(Kind::Conversation, id),
        "form": "conversation",
        "status": status,
        "thread": format_id(Kind::Thread, thread),
        "participants": participants,
        "role": role,
        "latest": { "at": rfc3339(last.at), "from": from, "snippet": cut(&line(&text), SNIPPET_CHARS) },
        "messages": mails.len(),
        "new": mails.iter().filter(|m| viewed_at.is_none_or(|v| m.at > v)).count(),
        "your_turn": !last.mine && last.to.iter().any(is_me),
        "derived": { "by": "rule", "rule": RULE },
    })))
}

/// The party of an address. Make its sender row if it has none, so that the
/// party has a `p-` ID. The kind of a new row stays empty: the sender step
/// (section 6.7) decides it.
fn party(conn: &Connection, address: &str, name: Option<&str>) -> rusqlite::Result<Value> {
    let name = name
        .map(|n| cut(&line(n), NAME_CHARS))
        .filter(|n| !n.is_empty());
    conn.prepare_cached("INSERT OR IGNORE INTO sender(address, name) VALUES (?1, ?2)")?
        .execute(params![address, name])?;
    let (row, stored): (i64, Option<String>) = conn
        .prepare_cached("SELECT id, name FROM sender WHERE address = ?1")?
        .query_row([address], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut p =
        json!({ "kind": "sender", "sender": format_id(Kind::Sender, row), "address": address });
    if let Some(n) = name.or(stored
        .map(|n| cut(&line(&n), NAME_CHARS))
        .filter(|n| !n.is_empty()))
    {
        p["name"] = json!(n);
    }
    Ok(p)
}

/// The addresses of an address list header, lowercase, in order, each one
/// time. The RFC 5322 scanner of the people step gives the same addresses, so
/// both steps make the same sender rows.
fn addresses(list: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in iris_people::addresses(list) {
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

/// The display name with its RFC 2047 encoded words decoded, by the scanner
/// of the people step.
// shortcut: decodes through a made-up mailbox, so a name with an encoded word and a
// bare comma stays as it is; upgrade when iris-people gives its decoder.
fn decode_name(name: &str) -> String {
    if !name.contains("=?") {
        return name.to_string();
    }
    match iris_people::mailboxes(&format!("{name} <x@x.example>")).as_slice() {
        [(_, Some(decoded))] => decoded.clone(),
        _ => name.to_string(),
    }
}

/// The subject of a message for the title and the quote. The quote of a rule
/// is the line that it matched (section 7.5).
fn subject_line(m: &Mail) -> String {
    Some(line(&m.subject))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| NO_SUBJECT.to_string())
}

/// The text as one line: each run of white space becomes one space.
fn line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first `max` characters of `text`.
fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Milliseconds since the Unix epoch as an RFC 3339 date-time in UTC.
pub fn rfc3339(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, s) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days (H. Hinnant, "chrono-compatible low-level date
    // algorithms").
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    // Always 3 digits of milliseconds: the order of two messages in one
    // second stays, and the text sorts in the order of time.
    let frac = format!(".{:03}", ms.rem_euclid(1000));
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}{frac}Z",
        s / 3600,
        s % 3600 / 60,
        s % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(1_767_225_600_000), "2026-01-01T00:00:00.000Z");
        assert_eq!(
            rfc3339(951_782_400_000 + 3_723_000),
            "2000-02-29T01:02:03.000Z"
        );
        assert_eq!(rfc3339(-1000), "1969-12-31T23:59:59.000Z");
        assert_eq!(rfc3339(100), "1970-01-01T00:00:00.100Z");
        assert_eq!(rfc3339(-1), "1969-12-31T23:59:59.999Z");
    }

    #[test]
    fn address_lists() {
        assert_eq!(
            addresses("\"Ortiz, Sam\" <Sam@Fabrikam.example>, ana@x.org; ana@x.org"),
            ["sam@fabrikam.example", "ana@x.org"]
        );
        // The form that the understanding step writes.
        assert_eq!(
            addresses("a@example.com,b@example.net"),
            ["a@example.com", "b@example.net"]
        );
        assert!(addresses("undisclosed-recipients:;").is_empty());
        // An address in a display name or a comment is not a recipient.
        assert_eq!(
            addresses("\"billing@acme.example\" <sam@fabrikam.example>"),
            ["sam@fabrikam.example"]
        );
        assert_eq!(
            addresses("\"a \\\" b@c.example\" <d@e.example> (f@g.example)"),
            ["d@e.example"]
        );
        assert_eq!(
            addresses("team: a@x.example, b@y.example;"),
            ["a@x.example", "b@y.example"]
        );
        assert!(addresses("a@b@c.d, @x.org, y@").is_empty());
    }

    #[test]
    fn hostile_marks() {
        assert!(!marked(None));
        assert!(!marked(Some("[]")));
        assert!(marked(Some("[\"spam\"]")));
        assert!(marked(Some("bad")));
    }
}
