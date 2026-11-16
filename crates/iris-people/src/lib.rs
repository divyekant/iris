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
//! It never writes a kind: the sender step decides the kind.

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
    /// The display name of the newest message from the person. If the person
    /// never sent you mail: the newest name that you gave it as a recipient.
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
    /// Display names in the `From` of the person, with the date of the newest message.
    names: HashMap<String, i64>,
    /// Display names that you gave the address as a recipient.
    to_names: HashMap<String, i64>,
}

impl Stats {
    /// The names of the person: the names in its `From` if there are any,
    /// else the names that you gave it.
    fn best_names(&self) -> &HashMap<String, i64> {
        if self.names.is_empty() {
            &self.to_names
        } else {
            &self.names
        }
    }

    fn latest_name(&self) -> Option<&str> {
        latest(self.best_names().iter().map(|(n, d)| (n.as_str(), *d)))
    }
}

/// The name with the newest date. On the same date, the first name in sort order.
fn latest<'a>(names: impl Iterator<Item = (&'a str, i64)>) -> Option<&'a str> {
    names
        .max_by_key(|(n, d)| (*d, std::cmp::Reverse(*n)))
        .map(|(n, _)| n)
}

/// Give the profile of each known person of the account, the person with the
/// newest message first.
pub fn people(store: &Store, account: i64) -> Result<Vec<Person>> {
    let (stats, known, tainted) = store.read(|c| {
        // One read transaction: both passes see the same snapshot, also
        // while a sync writes.
        let c = &c.unchecked_transaction()?;
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
        // For each thread: your last sent message, as (date, message ID).
        let mut your_threads: HashMap<i64, (i64, i64)> = HashMap::new();
        for row in st.query_map([account], row)? {
            let (message, thread, date, from, _, _, _, sent, draft, bad, _) = row?;
            if sent && !draft && !bad {
                let last = your_threads.entry(thread).or_insert((date, message));
                *last = (*last).max((date, message));
                if let Some(f) = from.and_then(|f| one_address(&f)) {
                    own.insert(f);
                }
            }
        }

        #[cfg(test)]
        if let Some(hook) = BETWEEN.with(|b| b.get()) {
            hook(store);
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
                    if own.contains(&a) {
                        continue;
                    }
                    let s = stats.entry(a.clone()).or_default();
                    // Each occurrence can give a name. The message counts one time.
                    if let Some(n) = name {
                        let d = s.to_names.entry(n).or_insert(date);
                        *d = (*d).max(date);
                    }
                    if done.insert(a.clone()) {
                        s.seen.push(Seen {
                            message,
                            thread,
                            date,
                            from_them: false,
                        });
                        known.insert(a);
                    }
                }
            } else if let Some(a) = from.filter(|a| !own.contains(a)) {
                let s = stats.entry(a.clone()).or_default();
                s.seen.push(Seen {
                    message,
                    thread,
                    date,
                    from_them: true,
                });
                if let Some(n) = name
                    .map(|n| decode_words(n.trim()))
                    .filter(|n| !n.is_empty())
                {
                    let d = s.names.entry(n).or_insert(date);
                    *d = (*d).max(date);
                }
                // You replied: a message of yours comes after this one.
                if your_threads
                    .get(&thread)
                    .is_some_and(|l| *l > (date, message))
                {
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
        // A new row has no kind: the sender step decides it (specification
        // 1, section 6.7). This crate never writes a kind.
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
            // The names in a `From` of the person win over the names that you gave it.
            let from_them = addrs.iter().any(|a| !stats[a].names.is_empty());
            for a in &addrs {
                let own = if from_them {
                    &stats[a].names
                } else {
                    &stats[a].to_names
                };
                for (n, d) in own {
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
                name: latest(names.iter().map(|(n, d)| (*n, *d))).map(str::to_string),
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

#[cfg(test)]
thread_local! {
    /// A test runs this between the two passes of [`people`].
    static BETWEEN: std::cell::Cell<Option<fn(&Store)>> = const { std::cell::Cell::new(None) };
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
/// display name if it has one. The scanner follows RFC 5322: quoted text
/// with `\` escapes, comments such as `(Name)` (also nested), addresses in
/// `<>`, and groups such as `team: a@example.org;`.
pub fn mailboxes(list: &str) -> Vec<(String, Option<String>)> {
    #[derive(Default)]
    struct Part {
        /// The text outside `<>` and comments, with quotes and escapes, for a bare address.
        raw: String,
        /// The same text without quotes and escapes, for the display name.
        name: String,
        /// The text inside `<>`.
        angle: Option<String>,
        /// The last comment that has text.
        comment: Option<String>,
    }
    let mut out = Vec::new();
    let mut part = Part::default();
    let (mut quoted, mut angle, mut depth) = (false, false, 0u32);
    let mut comment = String::new();
    let mut chars = list.chars();
    let finish = |part: Part, out: &mut Vec<(String, Option<String>)>| {
        let (address, name) = match part.angle {
            Some(a) => (a, Some(part.name)),
            None => (part.raw, part.comment.clone()),
        };
        if let Some(a) = valid_address(&address) {
            let name = name
                .map(|n| decode_words(n.trim()))
                .filter(|n| !n.is_empty())
                // The old form `a@example.org (Name)` puts the name in a comment.
                .or(part.comment);
            out.push((a, name));
        }
    };
    while let Some(c) = chars.next() {
        // The buffer that gets ordinary text now.
        let target = |part: &mut Part,
                      comment: &mut String,
                      depth: u32,
                      angle: bool,
                      text: &str,
                      quoted_char: bool| {
            if depth > 0 {
                comment.push_str(text);
            } else if angle {
                part.angle.get_or_insert_with(String::new).push_str(text);
            } else {
                part.raw.push_str(text);
                if !quoted_char {
                    part.name.push_str(text);
                }
            }
        };
        match c {
            '\\' if quoted || depth > 0 => {
                let next = chars.next().unwrap_or('\\');
                if depth > 0 {
                    comment.push(next);
                } else if angle {
                    let a = part.angle.get_or_insert_with(String::new);
                    a.push('\\');
                    a.push(next);
                } else {
                    part.raw.push('\\');
                    part.raw.push(next);
                    part.name.push(next);
                }
            }
            '"' if depth == 0 => {
                quoted = !quoted;
                // The quote stays in the address, not in the name.
                target(&mut part, &mut comment, 0, angle, "\"", true);
            }
            _ if quoted => target(
                &mut part,
                &mut comment,
                0,
                angle,
                c.encode_utf8(&mut [0; 4]),
                false,
            ),
            '(' => {
                if depth > 0 {
                    comment.push(c);
                }
                depth += 1;
            }
            ')' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let t = comment.trim();
                    if !t.is_empty() {
                        part.comment = Some(t.to_string());
                    }
                    comment.clear();
                    // A comment separates words of a name, but not the
                    // parts of an address such as `john(x)@example.org`.
                    if !angle {
                        part.name.push(' ');
                    }
                } else {
                    comment.push(c);
                }
            }
            _ if depth > 0 => comment.push(c),
            '<' => {
                angle = true;
                part.angle = Some(String::new());
            }
            '>' if angle => angle = false,
            // The end of a group name: the text before it is not a mailbox.
            ':' if !angle => {
                part = Part::default();
            }
            ',' | ';' if !angle => {
                finish(std::mem::take(&mut part), &mut out);
            }
            _ => target(
                &mut part,
                &mut comment,
                0,
                angle,
                c.encode_utf8(&mut [0; 4]),
                false,
            ),
        }
    }
    finish(part, &mut out);
    out
}

/// The address of one mailbox such as `"Name" <a@example.com>`, in lower case.
fn one_address(text: &str) -> Option<String> {
    mailboxes(text).into_iter().next().map(|(a, _)| a)
}

/// Check one address, as `local@domain`, and give it in lower case. A quoted
/// local part, such as `"john doe"@example.org`, can have any character, with
/// a `\` escape before a quote or a `\`.
fn valid_address(text: &str) -> Option<String> {
    let a = text.trim().to_lowercase();
    let bad = |t: &str| t.contains(|c: char| c.is_whitespace() || "\"<>(),;:@\\".contains(c));
    let (local, domain) = a.rsplit_once('@')?;
    let local_ok = match local.strip_prefix('"').and_then(|l| l.strip_suffix('"')) {
        Some(inner) => {
            let mut ok = true;
            let mut it = inner.chars();
            while let Some(c) = it.next() {
                match c {
                    '\\' => ok &= it.next().is_some(),
                    '"' | '\r' | '\n' => ok = false,
                    _ => {}
                }
            }
            ok
        }
        None => !local.is_empty() && !bad(local),
    };
    (local_ok && !domain.is_empty() && !bad(domain)).then_some(a)
}

/// Decode the RFC 2047 encoded words of a display name, such as
/// `=?UTF-8?Q?Bo_H=C3=A4rt?=`. A word with another charset than UTF-8,
/// US-ASCII or ISO-8859-1, or with a bad encoding, stays as it is.
// shortcut: own small decoder, use iris_clean's decoder when PR #15 (iris-clean) merges.
fn decode_words(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    let mut after_word = false;
    while let Some(start) = rest.find("=?") {
        let (before, word) = rest.split_at(start);
        let decoded = word.get(2..).and_then(|w| {
            let mut it = w.splitn(3, '?');
            let (charset, enc, tail) = (it.next()?, it.next()?, it.next()?);
            let end = tail.find("?=")?;
            let bytes = match enc {
                "Q" | "q" => q_decode(&tail[..end])?,
                "B" | "b" => b_decode(&tail[..end])?,
                _ => return None,
            };
            let text = match charset.to_ascii_lowercase().as_str() {
                "utf-8" | "us-ascii" => String::from_utf8(bytes).ok()?,
                "iso-8859-1" | "latin1" => bytes.iter().map(|&b| b as char).collect(),
                _ => return None,
            };
            Some((text, 2 + charset.len() + enc.len() + 2 + end + 2))
        });
        match decoded {
            Some((t, len)) => {
                // Space between two encoded words is not part of the name.
                if !(after_word && before.trim().is_empty()) {
                    out.push_str(before);
                }
                out.push_str(&t);
                rest = &word[len..];
                after_word = true;
            }
            None => {
                out.push_str(before);
                out.push_str("=?");
                rest = &word[2..];
                after_word = false;
            }
        }
    }
    out.push_str(rest);
    out
}

fn q_decode(text: &str) -> Option<Vec<u8>> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'_' => out.push(b' '),
            b'=' => {
                let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    Some(out)
}

fn b_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes().filter(|&c| c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
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
    fn both_passes_read_one_snapshot() {
        let dir = std::env::temp_dir().join(format!("iris-people-snap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let account = store.add_account("gmail", "me@example.com").unwrap();
        store
            .put_labels(
                account,
                &[("SENT", "SENT", "system"), ("INBOX", "INBOX", "system")],
            )
            .unwrap();
        let msg = |id: &str, label: &str, from: &str, to: &str| iris_store::NewMessage {
            provider_id: id.into(),
            thread_provider_id: "t1".into(),
            internal_date: Some(1000),
            labels: vec![label.into()],
            header: iris_store::Header {
                from_addr: Some(from.into()),
                to_addrs: Some(to.into()),
                ..Default::default()
            },
            ..Default::default()
        };
        store
            .put_messages(
                account,
                &[msg("in", "INBOX", "bo@example.org", "me@example.com")],
            )
            .unwrap();
        // The sync writes your reply between the two passes.
        BETWEEN.with(|b| {
            b.set(Some(|store: &Store| {
                let m = iris_store::NewMessage {
                    provider_id: "out".into(),
                    thread_provider_id: "t1".into(),
                    internal_date: Some(2000),
                    labels: vec!["SENT".into()],
                    header: iris_store::Header {
                        from_addr: Some("me@example.com".into()),
                        to_addrs: Some("ana@example.org".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                store.put_messages(1, &[m]).unwrap();
            }))
        });
        let first: Vec<String> = people(&store, account)
            .unwrap()
            .into_iter()
            .flat_map(|p| p.addresses.into_iter().map(|a| a.address))
            .collect();
        BETWEEN.with(|b| b.set(None));
        // The snapshot before the reply: nobody is known yet.
        assert_eq!(first, Vec::<String>::new());
        let mut after: Vec<String> = people(&store, account)
            .unwrap()
            .into_iter()
            .flat_map(|p| p.addresses.into_iter().map(|a| a.address))
            .collect();
        after.sort();
        assert_eq!(after, ["ana@example.org", "bo@example.org"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
    fn the_scanner_follows_rfc_5322() {
        let m = |name: &str| Some(name.to_string());
        assert_eq!(
            mailboxes(r#""a\"b"@example.org, <c@example.org (note)>, "x<y>" <d@example.org>"#),
            [
                (r#""a\"b"@example.org"#.to_string(), None),
                ("c@example.org".to_string(), m("note")),
                ("d@example.org".to_string(), m("x<y>"))
            ]
        );
        assert_eq!(
            mailboxes(r#"A (x, y: z) <e@example.org>, list:;, g: "h, i" <f@example.org>;"#),
            [
                ("e@example.org".to_string(), m("A")),
                ("f@example.org".to_string(), m("h, i"))
            ]
        );
        assert_eq!(mailboxes(r#""unclosed <z@example.org>"#), vec![]);
    }

    #[test]
    fn encoded_words() {
        assert_eq!(decode_words("=?UTF-8?Q?Bo_H=C3=A4rt?="), "Bo Härt");
        assert_eq!(decode_words("=?utf-8?B?Qm8gSMOkcnQ=?="), "Bo Härt");
        assert_eq!(
            decode_words("=?UTF-8?Q?Bo?= =?UTF-8?Q?_H=C3=A4rt?="),
            "Bo Härt"
        );
        assert_eq!(
            decode_words("Dr =?ISO-8859-1?Q?J=F6rg?= Moss"),
            "Dr Jörg Moss"
        );
        assert_eq!(decode_words("=?KOI8-R?Q?x?= =?bad"), "=?KOI8-R?Q?x?= =?bad");
        assert_eq!(
            mailboxes(r#"Team: Ana Lind <a@example.org>, "x: y" <b@example.org>;"#),
            [
                ("a@example.org".to_string(), Some("Ana Lind".to_string())),
                ("b@example.org".to_string(), Some("x: y".to_string()))
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
