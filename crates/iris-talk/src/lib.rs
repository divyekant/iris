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
/// A conversation with no message for this time is `quiet` (section 6.3).
pub const QUIET_MS: i64 = 14 * 24 * 60 * 60 * 1000;
/// The most characters of `latest.snippet`.
pub const SNIPPET_CHARS: usize = 200;
const TITLE_CHARS: usize = 200;
const QUOTE_CHARS: usize = 500;
const NAME_CHARS: usize = 200;
const NO_SUBJECT: &str = "(no subject)";

/// One message that the conversation counts.
struct Mail {
    id: i64,
    at: i64,
    /// Lowercase.
    from: String,
    from_name: Option<String>,
    to: Vec<String>,
    cc: Vec<String>,
    subject: String,
    text: String,
    mine: bool,
    service: bool,
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
    let mails = mails(conn, thread, current, &me)?;
    let existing = existing(conn, thread)?;

    let Some(old) = existing else {
        let Some(origin) = mails.iter().find(|m| !m.service) else {
            return Ok(None);
        };
        // The quote of a rule is the line that it matched (section 7.5).
        let subject = Some(line(&origin.subject))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| NO_SUBJECT.to_string());
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
        parts.fill(&mut doc, thread, &mails, now);
        save(conn, id, &doc, None, now, &mails, origin)?;
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
    let mut status = match old.status.as_str() {
        "gone" => old.before_gone.clone(),
        s => Some(s.to_string()),
    };
    // A conversation needs a message from you, a person or a stranger
    // (section 7.3). Service mail alone does not keep it.
    if mails.iter().all(|m| m.service) {
        // The sync can set the status column to gone first (section 4.5),
        // so the document can still give the old status.
        if old.status != "gone" || old.doc["status"] != "gone" {
            let before = if old.status == "gone" {
                old.before_gone
            } else {
                Some(old.status)
            };
            let mut doc = old.doc;
            doc["status"] = json!("gone");
            doc["updated"] = json!(rfc3339(now));
            conn.execute(
                "UPDATE item SET status = 'gone', status_before_gone = ?2, doc = ?3, updated_at = ?4
                 WHERE id = ?1",
                params![old.id, before, doc.to_string(), now],
            )?;
        }
        return Ok(Some(old.id));
    }
    if status.as_deref() != Some("muted") {
        status = None;
    }
    let mut doc = doc(
        conn,
        thread,
        old.id,
        &me,
        viewed_at,
        now,
        &mails,
        status.as_deref(),
    )?
    .expect("a message counts");
    parts.fill(&mut doc, thread, &mails, now);
    corrections(conn, old.id, &mut doc)?;
    // Compare without the time of the change, so a call that changes nothing
    // writes nothing.
    doc["updated"] = old.doc["updated"].clone();
    if doc == old.doc && old.status != "gone" {
        return Ok(Some(old.id));
    }
    doc["updated"] = json!(rfc3339(now));
    save(conn, old.id, &doc, Some(()), now, &mails, old.origin)?;
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
    fn fill(&self, doc: &mut Value, thread: i64, mails: &[Mail], now: i64) {
        let t = format_id(Kind::Thread, thread);
        let mut sources = vec![json!({
            "message": format_id(Kind::Message, self.origin),
            "thread": t,
            "role": "origin",
            "quote": self.quote,
        })];
        sources.extend(mails.iter().filter(|m| m.id != self.origin).map(
            |m| json!({ "message": format_id(Kind::Message, m.id), "thread": t, "role": "update" }),
        ));
        doc["title"] = json!(self.title);
        doc["sources"] = Value::Array(sources);
        doc["created"] = json!(rfc3339(self.created));
        doc["updated"] = json!(rfc3339(now));
    }
}

fn save(
    conn: &Connection,
    id: i64,
    doc: &Value,
    existed: Option<()>,
    now: i64,
    mails: &[Mail],
    origin: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE item SET status = ?2, status_before_gone = NULL, doc = ?3, updated_at = ?4
         WHERE id = ?1",
        params![id, doc["status"].as_str(), doc.to_string(), now],
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
    for m in mails.iter().filter(|m| m.id != origin) {
        add.execute(params![id, m.id])?;
    }
    Ok(())
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
        let mut at = &mut *doc;
        for key in field.split('.') {
            if !at.is_object() {
                *at = json!({});
            }
            at = &mut at[key];
        }
        *at = value;
    }
    Ok(())
}

/// The messages that the conversation counts, oldest first.
fn mails(
    conn: &Connection,
    thread: i64,
    current: Option<i64>,
    me: &str,
) -> rusqlite::Result<Vec<Mail>> {
    let mut q = conn.prepare_cached(
        "SELECT m.id, COALESCE(m.internal_date, h.date, 0) AS at, h.from_addr, h.from_name,
                h.to_addrs, h.cc_addrs, h.subject, b.text, m.hostile,
                (SELECT kind FROM sender WHERE address = lower(h.from_addr)),
                EXISTS(SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                       WHERE ml.message_id = m.id AND l.provider_id = 'SENT'),
                EXISTS(SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                       WHERE ml.message_id = m.id AND l.provider_id = 'DRAFT')
         FROM message m JOIN header h ON h.message_id = m.id
         LEFT JOIN body b ON b.message_id = m.id
         WHERE m.thread_id = ?1 AND m.gone_at IS NULL
           AND (m.understood_at IS NOT NULL OR m.id IS ?2)
         ORDER BY at, m.id",
    )?;
    let rows = q.query_map(params![thread, current], |r| {
        let hostile: Option<String> = r.get(8)?;
        let kind: Option<String> = r.get(9)?;
        let sent: bool = r.get(10)?;
        let draft: bool = r.get(11)?;
        let from = r
            .get::<_, Option<String>>(2)?
            .and_then(|f| addresses(&f).into_iter().next())
            // A From that is not an address gives no party.
            .unwrap_or_default();
        let mine = sent || (!from.is_empty() && from == me);
        let hostile = marked(hostile.as_deref()) || (!mine && kind.as_deref() == Some("hostile"));
        let mail = Mail {
            id: r.get(0)?,
            at: r.get(1)?,
            from_name: r
                .get::<_, Option<String>>(3)?
                .filter(|n| !n.trim().is_empty()),
            to: addresses(r.get::<_, Option<String>>(4)?.as_deref().unwrap_or("")),
            cc: addresses(r.get::<_, Option<String>>(5)?.as_deref().unwrap_or("")),
            subject: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
            text: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
            service: !mine && kind.as_deref() == Some("service"),
            mine,
            from,
        };
        // A message from another party needs a From address to be a party.
        let counts = !draft && !hostile && (mail.mine || !mail.from.is_empty());
        Ok(counts.then_some(mail))
    })?;
    rows.filter_map(|r| r.transpose()).collect()
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
    mails: &[Mail],
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
            if a != me && !aliases.contains(&a) && !order.contains(&a) {
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
    let in_to = mails.iter().any(|m| m.to.iter().any(|a| a == me));
    let in_cc = mails.iter().any(|m| m.cc.iter().any(|a| a == me));
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
        "your_turn": !last.mine && last.to.iter().any(|a| a == me),
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

/// The addresses in an address list header, lowercase, in order, each one
/// time. Text in quotes and in comments is a display name or a note, never an
/// address. In each item of the list, the address is in `<>`, or it is the
/// item itself.
fn addresses(list: &str) -> Vec<String> {
    let mut bare = String::with_capacity(list.len());
    let (mut quoted, mut depth, mut escape) = (false, 0u32, false);
    for c in list.chars() {
        if escape {
            escape = false;
        } else if c == '\\' && (quoted || depth > 0) {
            escape = true;
        } else if quoted {
            quoted = c != '"';
        } else if c == '"' && depth == 0 {
            quoted = true;
        } else if c == '(' {
            depth += 1;
        } else if c == ')' && depth > 0 {
            depth -= 1;
        } else if depth == 0 {
            bare.push(c);
        }
    }
    let mut out: Vec<String> = Vec::new();
    for item in bare.split([',', ';']) {
        // A group ("team: a@x, b@y;") puts a name and a colon first.
        let item = item.rsplit(':').next().unwrap_or(item);
        let a = match item.rfind('<') {
            Some(i) => item[i + 1..].split('>').next().unwrap_or(""),
            None => item,
        };
        let a = a.trim().to_lowercase();
        let ok = match a.split_once('@') {
            Some((l, d)) => {
                !l.is_empty()
                    && d.contains('.')
                    && !d.contains('@')
                    && !a.contains(char::is_whitespace)
            }
            None => false,
        };
        if ok && !out.contains(&a) {
            out.push(a);
        }
    }
    out
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
