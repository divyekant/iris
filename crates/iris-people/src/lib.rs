//! People of Iris: one profile for each known person, from the mail history
//! in the store (specification 1, sections 5.5 and 6.7).
//!
//! A known person is an address that matches the `person` rule of section
//! 6.7: you sent a message to the address, or you sent a message in a thread
//! where the address sent one. A draft does not count. A message with a
//! hostile mark or the label `SPAM` does not count, because its `From` can be
//! false. A kind in the `sender` table changes the result: `hostile` always
//! removes the address, and a kind that a person set always wins.
//!
//! One profile can have more than one address. Two known addresses go into
//! one profile when their latest display names are the same, with two words
//! or more, and no message from the address has a hostile mark.
//!
//! The profile reads the `header` table, which the understanding step fills
//! (section 7.1, step 1). The profile ID is the `p-` ID of the first sender
//! row of its addresses. This crate adds a sender row (address and name, no
//! kind) for each address that has no row, so that each address has its ID.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use iris_store::{format_id, parse_id, Kind, Result, Store};
use rusqlite::{params, OptionalExtension};

/// One address of a person, with its own `p-` ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub id: String,
    pub address: String,
}

/// The profile of one known person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    /// The `p-` ID of the first sender row of the addresses.
    pub id: String,
    /// The display name of the newest message from the person.
    pub name: Option<String>,
    /// Each display name that the person used, sorted.
    pub names: Vec<String>,
    /// Each address, in the order of the sender row IDs.
    pub addresses: Vec<Address>,
    /// The first and the last message with the person: milliseconds since
    /// the Unix epoch, UTC.
    pub first_at: i64,
    pub last_at: i64,
    /// Messages from the person to you.
    pub from_them: u32,
    /// Messages from you to the person.
    pub from_you: u32,
    /// The `t-` IDs of the threads with the person, newest first.
    pub threads: Vec<String>,
}

/// One message that names an address.
#[derive(Clone, Copy)]
struct Seen {
    message: i64,
    thread: i64,
    date: i64,
    from_them: bool,
}

#[derive(Default)]
struct Stats {
    seen: Vec<Seen>,
    /// Display name, with the date of its newest message.
    names: HashMap<String, i64>,
}

impl Stats {
    fn latest_name(&self) -> Option<&str> {
        self.names
            .iter()
            .max_by_key(|(n, d)| (**d, std::cmp::Reverse(n.as_str())))
            .map(|(n, _)| n.as_str())
    }
}

/// Give the profile of each known person of the account, the person with the
/// newest message first.
pub fn people(store: &Store, account: i64) -> Result<Vec<Person>> {
    let (stats, known, tainted) = store.read(|c| {
        let mut own: HashSet<String> = HashSet::new();
        if let Some(a) = c
            .query_row(
                "SELECT address FROM account WHERE id = ?1",
                [account],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            own.insert(a.to_lowercase());
        }

        let flag = |label: &str| {
            format!(
                "EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                  WHERE ml.message_id = m.id AND l.provider_id = '{label}')"
            )
        };
        let mut st = c.prepare(&format!(
            "SELECT m.id, m.thread_id, coalesce(m.internal_date, h.date, 0), h.from_addr,
               h.from_name, h.to_addrs, h.cc_addrs, {sent}, {draft},
               (m.hostile IS NOT NULL AND m.hostile NOT IN ('', '[]')) OR {spam},
               m.content_sha256
             FROM message m JOIN header h ON h.message_id = m.id
             WHERE m.account_id = ?1 AND m.gone_at IS NULL",
            sent = flag("SENT"),
            draft = flag("DRAFT"),
            spam = flag("SPAM"),
        ))?;
        // Pass 1: your addresses (with aliases) and the threads where you sent mail.
        let mut your_threads: HashSet<i64> = HashSet::new();
        for row in st.query_map([account], row)? {
            let (_, thread, _, from, _, _, _, sent, draft, bad, _) = row?;
            if sent && !draft && !bad {
                your_threads.insert(thread);
                if let Some(f) = from.and_then(|f| one_address(&f)) {
                    own.insert(f);
                }
            }
        }

        // Pass 2: the history of each address.
        let mut stats: HashMap<String, Stats> = HashMap::new();
        let mut known: HashSet<String> = HashSet::new();
        let mut tainted: HashSet<String> = HashSet::new();
        for row in st.query_map([account], row)? {
            let (message, thread, date, from, name, to, cc, sent, draft, bad, sha) = row?;
            let from = from.and_then(|f| one_address(&f));
            if draft {
                continue;
            }
            if bad {
                tainted.extend(from);
                continue;
            }
            if sent {
                let to = to.as_deref().map(mailboxes).unwrap_or_default();
                let cc = cc.as_deref().map(mailboxes).unwrap_or_default();
                // The header table has no Bcc. Gmail keeps it in the raw sent message.
                let bcc = sha
                    .and_then(|sha| store.content_path(&sha))
                    .and_then(|p| head_field(&p, "bcc"))
                    .map(|b| mailboxes(&b))
                    .unwrap_or_default();
                let mut done = HashSet::new();
                for (a, name) in to.into_iter().chain(cc).chain(bcc) {
                    if own.contains(&a) || !done.insert(a.clone()) {
                        continue;
                    }
                    let s = stats.entry(a.clone()).or_default();
                    s.seen.push(Seen {
                        message,
                        thread,
                        date,
                        from_them: false,
                    });
                    if let Some(n) = name {
                        let d = s.names.entry(n).or_insert(date);
                        *d = (*d).max(date);
                    }
                    known.insert(a);
                }
            } else if let Some(a) = from.filter(|a| !own.contains(a)) {
                let s = stats.entry(a.clone()).or_default();
                s.seen.push(Seen {
                    message,
                    thread,
                    date,
                    from_them: true,
                });
                if let Some(n) = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
                    let d = s.names.entry(n).or_insert(date);
                    *d = (*d).max(date);
                }
                if your_threads.contains(&thread) {
                    known.insert(a);
                }
            }
        }
        Ok((stats, known, tainted))
    })?;

    // The sender rows: read the kind, and add a row for each new address.
    let mut ids: HashMap<String, i64> = HashMap::new();
    let mut kept: Vec<String> = Vec::new();
    store.write(|tx| {
        let mut kind =
            tx.prepare_cached("SELECT id, kind, decided_by FROM sender WHERE address = ?1")?;
        let mut add = tx.prepare_cached(
            "INSERT INTO sender(address, name) VALUES (?1, ?2)
             ON CONFLICT(address) DO NOTHING RETURNING id",
        )?;
        let mut addrs: Vec<&String> = stats.keys().collect();
        addrs.sort();
        for a in addrs {
            let row: Option<(i64, Option<String>, Option<String>)> = kind
                .query_row([a], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .optional()?;
            let (k, by) = row
                .as_ref()
                .map(|(_, k, b)| (k.as_deref(), b.as_deref()))
                .unwrap_or((None, None));
            let include = match (k, by) {
                (Some("hostile"), _) => false,
                (k, Some("person")) => k == Some("person"),
                _ => known.contains(a),
            };
            if !include {
                continue;
            }
            let id = match row {
                Some((id, _, _)) => id,
                None => add.query_row(params![a, stats[a].latest_name()], |r| r.get(0))?,
            };
            ids.insert(a.clone(), id);
            kept.push(a.clone());
        }
        Ok(())
    })?;

    // Group the addresses: one group for each shared full name, else one
    // group for each address.
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for a in kept {
        let key = match stats[&a].latest_name().and_then(name_key) {
            Some(k) if !tainted.contains(&a) => format!("n:{k}"),
            _ => format!("a:{a}"),
        };
        groups.entry(key).or_default().push(a);
    }

    let mut out: Vec<Person> = groups
        .into_values()
        .map(|mut addrs| {
            addrs.sort_by_key(|a| ids[a]);
            let mut seen: Vec<Seen> = addrs.iter().flat_map(|a| stats[a].seen.clone()).collect();
            seen.sort_by_key(|s| s.message);
            seen.dedup_by_key(|s| s.message);
            let mut names: HashMap<&str, i64> = HashMap::new();
            for a in &addrs {
                for (n, d) in &stats[a].names {
                    let e = names.entry(n.as_str()).or_insert(*d);
                    *e = (*e).max(*d);
                }
            }
            let mut threads: HashMap<i64, i64> = HashMap::new();
            for s in &seen {
                let e = threads.entry(s.thread).or_insert(s.date);
                *e = (*e).max(s.date);
            }
            let mut threads: Vec<(i64, i64)> = threads.into_iter().collect();
            threads.sort_by_key(|(t, d)| (std::cmp::Reverse(*d), std::cmp::Reverse(*t)));
            Person {
                id: format_id(Kind::Sender, ids[&addrs[0]]),
                name: names
                    .iter()
                    .max_by_key(|(n, d)| (**d, std::cmp::Reverse(**n)))
                    .map(|(n, _)| n.to_string()),
                names: names
                    .keys()
                    .map(|n| n.to_string())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                addresses: addrs
                    .iter()
                    .map(|a| Address {
                        id: format_id(Kind::Sender, ids[a]),
                        address: a.clone(),
                    })
                    .collect(),
                first_at: seen.iter().map(|s| s.date).min().unwrap_or(0),
                last_at: seen.iter().map(|s| s.date).max().unwrap_or(0),
                from_them: seen.iter().filter(|s| s.from_them).count() as u32,
                from_you: seen.iter().filter(|s| !s.from_them).count() as u32,
                threads: threads
                    .into_iter()
                    .map(|(t, _)| format_id(Kind::Thread, t))
                    .collect(),
            }
        })
        .collect();
    out.sort_by(|a, b| b.last_at.cmp(&a.last_at).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

/// One message row: ID, thread, date, `From` address and name, `To`, `Cc`,
/// the flags sent, draft and hostile-or-spam, and the content file.
type Row = (
    i64,
    i64,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
    bool,
    bool,
    Option<String>,
);

/// The largest header block that [`head_field`] reads.
const HEAD_LIMIT: u64 = 1 << 20;

/// The value of the first header `name` (lower case) of a raw message file,
/// unfolded. Reads only the header block, up to [`HEAD_LIMIT`] bytes.
fn head_field(path: &std::path::Path, name: &str) -> Option<String> {
    use std::io::{BufRead, Read};
    let file = std::fs::File::open(path).ok()?;
    let mut out: Option<String> = None;
    let mut inside = false;
    for line in std::io::BufReader::new(file.take(HEAD_LIMIT)).split(b'\n') {
        let line = line.ok()?;
        let line = String::from_utf8_lossy(&line);
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if inside {
                out.as_mut()?.push_str(line);
            }
            continue;
        }
        if out.is_some() {
            break;
        }
        if let Some((n, v)) = line.split_once(':') {
            inside = n.trim().eq_ignore_ascii_case(name);
            if inside {
                out = Some(v.trim().to_string());
            }
        }
    }
    out
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Row> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
    ))
}

/// Give the profile that has the `p-` ID `id`, as the profile ID or as the ID
/// of one of its addresses. `None` if no known person has the ID.
pub fn person(store: &Store, account: i64, id: &str) -> Result<Option<Person>> {
    // shortcut: builds every profile to find one, upgrade to a query for one address set if the local API needs it fast.
    if !matches!(parse_id(id), Some((Kind::Sender, _))) {
        return Ok(None);
    }
    Ok(people(store, account)?
        .into_iter()
        .find(|p| p.id == id || p.addresses.iter().any(|a| a.id == id)))
}

/// The addresses of an address list such as `A <a@example.com>, b@example.com`,
/// in lower case. A part that has no address gives nothing.
pub fn addresses(list: &str) -> Vec<String> {
    mailboxes(list).into_iter().map(|(a, _)| a).collect()
}

/// The mailboxes of an address list: each address in lower case, with its
/// display name if it has one.
pub fn mailboxes(list: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let (mut quoted, mut angle, mut depth, mut start) = (false, false, 0u32, 0);
    for (i, c) in list.char_indices() {
        match c {
            '"' if !angle && depth == 0 => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            '<' if !quoted && depth == 0 => angle = true,
            '>' if !quoted && depth == 0 => angle = false,
            ',' | ';' if !quoted && !angle && depth == 0 => {
                out.extend(mailbox(&list[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    out.extend(mailbox(&list[start..]));
    out
}

/// One mailbox such as `"Lind, Ana" <ana@example.org>`: the address and the
/// display name.
fn mailbox(part: &str) -> Option<(String, Option<String>)> {
    let (part, comment) = strip_comments(part);
    let address = one_address(&part)?;
    let name = part
        .rfind('<')
        .map(|i| part[..i].trim().trim_matches('"').trim().to_string())
        .filter(|n| !n.is_empty())
        // The old form `a@example.org (Name)` puts the name in a comment.
        .or(comment);
    Some((address, name))
}

/// Remove the RFC 5322 comments, such as `(Name)`, outside quoted text.
/// Gives the text and the last comment that has text.
fn strip_comments(part: &str) -> (String, Option<String>) {
    let (mut out, mut comment, mut last) = (String::new(), String::new(), None);
    let (mut quoted, mut depth) = (false, 0u32);
    for c in part.chars() {
        match c {
            '"' if depth == 0 => {
                quoted = !quoted;
                out.push(c);
            }
            '(' if !quoted => {
                if depth > 0 {
                    comment.push(c);
                }
                depth += 1;
            }
            ')' if !quoted && depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let t = comment.trim();
                    if !t.is_empty() {
                        last = Some(t.to_string());
                    }
                    comment.clear();
                    out.push(' ');
                } else {
                    comment.push(c);
                }
            }
            _ if depth > 0 => comment.push(c),
            _ => out.push(c),
        }
    }
    (out, last)
}

/// The address of one mailbox such as `"Name" <a@example.com>`, in lower case.
fn one_address(part: &str) -> Option<String> {
    let a = match (part.rfind('<'), part.rfind('>')) {
        (Some(s), Some(e)) if s < e => &part[s + 1..e],
        // A group such as `team: a@example.com` starts with its name.
        _ => part.rsplit(':').next().unwrap_or(part),
    };
    let a = a.trim().to_lowercase();
    let bad = |t: &str| t.contains(|c: char| c.is_whitespace() || "\"<>(),;:@".contains(c));
    // The domain is after the last `@`. A quoted local part, such as
    // `"john doe"@example.org`, can have any character but a quote.
    let (local, domain) = a.rsplit_once('@')?;
    let local_ok = match local.strip_prefix('"').and_then(|l| l.strip_suffix('"')) {
        Some(inner) => !inner.contains(['"', '\\', '\r', '\n']),
        None => !local.is_empty() && !bad(local),
    };
    (local_ok && !domain.is_empty() && !bad(domain)).then_some(a)
}

/// The key of a display name for the merge of addresses: lower case, one
/// space between words. Only a name of two words or more, with no address in
/// it, gives a key.
fn name_key(name: &str) -> Option<String> {
    let words: Vec<String> = name
        .split_whitespace()
        .map(|w| w.trim_matches(|c| c == '"' || c == '\'').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    (words.len() >= 2 && !name.contains('@')).then(|| words.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_lists() {
        assert_eq!(
            addresses(r#""Torun, Avel" <Avel@Example.com>, b@example.org; team: c@example.net"#),
            ["avel@example.com", "b@example.org", "c@example.net"]
        );
        assert_eq!(addresses("undisclosed-recipients:;"), Vec::<String>::new());
        assert_eq!(addresses("not an address, @x, x@"), Vec::<String>::new());
        assert_eq!(
            addresses(r#""john.doe"@Example.org, "a, b"@example.org, Jo <"j o"@example.org>"#),
            [
                r#""john.doe"@example.org"#,
                r#""a, b"@example.org"#,
                r#""j o"@example.org"#
            ]
        );
        assert_eq!(
            mailboxes(
                "bob@example.org (Bob Smith), cy@example.org (x, (y)), \"(no)\" <d@example.org>"
            ),
            [
                ("bob@example.org".to_string(), Some("Bob Smith".to_string())),
                ("cy@example.org".to_string(), Some("x, (y)".to_string())),
                ("d@example.org".to_string(), Some("(no)".to_string()))
            ]
        );
        assert_eq!(
            mailboxes(r#""Lind, Ana" <ana@example.org>, bo@example.org"#),
            [
                ("ana@example.org".to_string(), Some("Lind, Ana".to_string())),
                ("bo@example.org".to_string(), None)
            ]
        );
    }

    #[test]
    fn name_keys() {
        assert_eq!(name_key("  Mira   Kolen "), Some("mira kolen".into()));
        assert_eq!(name_key("Mira"), None);
        assert_eq!(name_key("mira@example.com Kolen"), None);
    }
}
