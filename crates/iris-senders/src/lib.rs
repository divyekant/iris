//! The sender axis of specification 1 section 6.7. [`decide`] gives the kind
//! of the sender of one message, and keeps the `sender` row of the address
//! current.
//!
//! Fixed rules decide each kind. The specification makes `stranger` the
//! result when no rule matches, so this crate calls no model. The model part
//! of the sender axis is the `phishing` mark, which the understanding step
//! sets. The hostile marks of one message (`dmarc`, `spoof`, `spam`,
//! `phishing`) are the work of the understanding step, not of this crate.
//!
//! Mail text is data: this crate only compares header bytes.

use rusqlite::{params, Connection, OptionalExtension};

mod head;
use head::Head;

/// The kind of a sender. The kind is a property of the address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// You: the message has the label `SENT` or `DRAFT`. A `From` with the
    /// address of the account is not enough, because anyone can write it.
    /// No `sender` row.
    SelfOwner,
    Person,
    Service,
    Stranger,
    Hostile,
}

impl Kind {
    /// The name in the `sender.kind` column. `SelfOwner` gives `self`.
    pub fn name(self) -> &'static str {
        match self {
            Kind::SelfOwner => "self",
            Kind::Person => "person",
            Kind::Service => "service",
            Kind::Stranger => "stranger",
            Kind::Hostile => "hostile",
        }
    }

    fn parse(s: &str) -> Option<Kind> {
        [Kind::Person, Kind::Service, Kind::Stranger, Kind::Hostile]
            .into_iter()
            .find(|k| k.name() == s)
    }
}

/// The result of [`decide`] for one message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub kind: Kind,
    /// The ID of the `sender` row (the `p-` ID). `None` for `SelfOwner`, and
    /// for a message with no `From` address.
    pub sender: Option<i64>,
    /// The rule that matched, as in the `basis` column.
    pub basis: String,
    /// The kind of the `From` row before this call.
    pub previous: Option<Kind>,
    /// The `sender` rows whose kind became `hostile` or stopped being
    /// `hostile` in this call: the `From` row, or a recipient of a sent
    /// message. The caller runs understanding again on each message from
    /// these addresses (spec 1 section 6.7).
    pub rerun: Vec<i64>,
}

/// Decide the sender kind of the stored message `message`, with its raw
/// bytes `raw`, and write the kind to the `sender` row of the address.
///
/// Call it for each message, also for a message with the label `SENT`: a
/// sent message makes each of its `To` and `Cc` addresses a `person`.
///
/// Rows are keyed on the lowercase address. A row with `decided_by` =
/// `person` never changes. The function reads the `header`, `message`,
/// `message_label` and `sender` tables, so call it after the `header` row
/// of each earlier message is saved. In the `header` row, `from_addr` is
/// the bare lowercase address, and `to_addrs` and `cc_addrs` are bare
/// lowercase addresses joined by `,`. `now_ms` goes to `decided_at`.
pub fn decide(
    conn: &Connection,
    message: i64,
    raw: &[u8],
    now_ms: i64,
) -> rusqlite::Result<Decision> {
    let head = Head::parse(raw);
    let (date, sha, account): (Option<i64>, Option<String>, String) = conn.query_row(
        "SELECT m.internal_date, m.content_sha256, a.address FROM message m
           JOIN account a ON a.id = m.account_id WHERE m.id = ?1",
        [message],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let labels: Vec<String> = conn
        .prepare_cached(
            "SELECT l.provider_id FROM message_label ml JOIN label l ON l.id = ml.label_id
              WHERE ml.message_id = ?1",
        )?
        .query_map([message], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let has = |l: &str| labels.iter().any(|x| x == l);
    let from = head.from();
    // This decision writes the evidence of this message again.
    conn.prepare_cached("DELETE FROM sender_evidence WHERE message_id = ?1")?
        .execute([message])?;

    if has("DRAFT") {
        // A draft does not count for the `person` rule.
        return Ok(own(if has("SENT") { "sent" } else { "draft" }));
    }
    // Only the SENT label makes mail from you. Anyone can write the account
    // address in `From`, so such mail goes through the rules for received
    // mail.
    if has("SENT") {
        let recipients: Vec<String> = head
            .recipients()
            .into_iter()
            .map(|(a, _)| a)
            .filter(|a| !a.eq_ignore_ascii_case(&account))
            .collect();
        // The `hostile` rule comes before the `person` rule. It also compares
        // each recipient with the other recipients of this message: when two
        // domains of one message are near copies of each other, the message
        // proves neither one.
        let mut known = known_domains(conn, "")?;
        known.extend(recipients.iter().map(|a| domain_of(a).to_string()));
        let mut rerun = Vec::new();
        for to in &recipients {
            conn.prepare_cached(
                "INSERT OR IGNORE INTO sender_evidence(message_id, address, rule, content_sha256)
                 VALUES (?1, ?2, 'sent', ?3)",
            )?
            .execute(params![message, to, sha])?;
            match near(to, &known) {
                Some(k) => upsert(
                    &mut rerun,
                    conn,
                    to,
                    None,
                    Kind::Hostile,
                    &format!("near copy of {k}"),
                    now_ms,
                )?,
                None => upsert(&mut rerun, conn, to, None, Kind::Person, "sent", now_ms)?,
            };
        }
        return Ok(Decision {
            rerun,
            ..own("sent")
        });
    }

    let Some((addr, name)) = from else {
        return Ok(Decision {
            kind: Kind::Stranger,
            sender: None,
            basis: "no from address".into(),
            previous: None,
            rerun: Vec::new(),
        });
    };
    // (id, kind, decided_by)
    type Row = (i64, Option<String>, Option<String>);
    let row: Option<Row> = conn
        .query_row(
            "SELECT id, kind, decided_by FROM sender WHERE address = ?1",
            [&addr],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let previous = row
        .as_ref()
        .and_then(|r| r.1.as_deref())
        .and_then(Kind::parse);
    if let Some((id, Some(kind), Some(by))) = &row {
        if by == "person" {
            if let Some(kind) = Kind::parse(kind) {
                return Ok(Decision {
                    kind,
                    sender: Some(*id),
                    basis: "set by a person".into(),
                    previous,
                    rerun: Vec::new(),
                });
            }
        }
    }

    let (kind, basis) = rules(conn, &head, &addr, message, sha.as_deref(), date)?;
    let mut rerun = Vec::new();
    let id = upsert(
        &mut rerun,
        conn,
        &addr,
        name.as_deref(),
        kind,
        &basis,
        now_ms,
    )?;
    Ok(Decision {
        rerun,
        kind,
        sender: Some(id),
        basis,
        previous,
    })
}

/// True when the message `s` is not gone and has the label `SENT` and not
/// `DRAFT`.
const SENT_NOT_DRAFT: &str =
    "(s.gone_at IS NULL AND EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                  WHERE ml.message_id = s.id AND l.provider_id = 'SENT')
      AND NOT EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                  WHERE ml.message_id = s.id AND l.provider_id = 'DRAFT'))";

/// SQL that is true when the header `h` names the address of parameter
/// `?n` in `To` or `Cc`.
fn to_addr(n: u8) -> String {
    format!(
        "instr(',' || coalesce(h.to_addrs, '') || ',' || coalesce(h.cc_addrs, '') || ',',
               ',' || ?{n} || ',') > 0"
    )
}

fn own(basis: &str) -> Decision {
    Decision {
        kind: Kind::SelfOwner,
        sender: None,
        basis: basis.into(),
        previous: None,
        rerun: Vec::new(),
    }
}

/// The engine rules in the order of spec 1 section 6.7: `hostile`, then
/// `person`, then `service`, else `stranger`.
fn rules(
    conn: &Connection,
    head: &Head,
    addr: &str,
    message: i64,
    sha: Option<&str>,
    date: Option<i64>,
) -> rusqlite::Result<(Kind, String)> {
    let domain = domain_of(addr);
    if let Some(k) = near(addr, &known_domains(conn, addr)?) {
        return Ok((Kind::Hostile, format!("near copy of {k}")));
    }

    // The `sent` rule: the decision of each sent message writes a `sent`
    // evidence row for each recipient. A row counts while its message is a
    // sent message with the same bytes.
    // A sent message that waits for its own decision has no row yet. Its
    // decision makes the recipients `person`. Spec 1 section 6.7 accepts
    // this: "The first sync fetches newest first, so a known person can be a
    // stranger for some time."
    let sent: bool = conn.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM sender_evidence e JOIN message s ON s.id = e.message_id
              WHERE e.address = ?1 AND e.rule = 'sent' AND s.content_sha256 IS e.content_sha256
                AND {SENT_NOT_DRAFT})"
        ),
        [addr],
        |r| r.get(0),
    )?;
    if sent {
        return Ok((Kind::Person, "sent".into()));
    }
    // shortcut: this query reads each message from the address, so its cost
    // grows with the history of the address (about 1.7 ms for each message on the
    // 100,000-message synthetic mailbox, release build). Keep a "replied"
    // flag in the store if a large mailbox makes it slow.
    // The `replied` rule: a sent message in a thread after a message from
    // the address. A sent message before the address joined the thread does
    // not count. A sent message of this thread to the address also counts
    // (the `sent` rule), because a sent message that is older than this one
    // can still wait for its own decision (the engine reads newest first).
    let replied: bool = conn.query_row(
        &format!(
            "SELECT EXISTS (
               SELECT 1 FROM header hm JOIN message m ON m.id = hm.message_id
                 JOIN message s ON s.thread_id = m.thread_id AND s.id != m.id
                 JOIN header h ON h.message_id = s.id
                WHERE hm.from_addr = ?1 AND m.gone_at IS NULL AND {SENT_NOT_DRAFT}
                  AND (s.internal_date >= m.internal_date OR {}))",
            to_addr(1)
        ),
        [addr],
        |r| r.get(0),
    )?;
    if replied {
        return Ok((Kind::Person, "replied".into()));
    }

    if head.automated() && head.dkim_pass(domain) {
        let earlier: i64 = conn.query_row(
            "SELECT count(*) FROM (SELECT 1 FROM header h JOIN message m ON m.id = h.message_id
              WHERE h.from_addr = ?1 AND m.internal_date < ?2 AND m.gone_at IS NULL LIMIT 2)",
            params![addr, date.unwrap_or(i64::MIN)],
            |r| r.get(0),
        )?;
        if earlier >= 2 {
            conn.prepare_cached(
                "INSERT OR IGNORE INTO sender_evidence(message_id, address, rule, content_sha256)
                 VALUES (?1, ?2, 'service', ?3)",
            )?
            .execute(params![message, addr, sha])?;
            return Ok((
                Kind::Service,
                "automated, dkim pass, 2 earlier messages".into(),
            ));
        }
    }
    // The kind belongs to the address. Another message from the address can
    // qualify (an older message is read later, newest first). It counts
    // while it is there with the same bytes (so the same automated and DKIM
    // headers), and still has 2 earlier messages from the address.
    let other: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sender_evidence ev JOIN message q ON q.id = ev.message_id
          WHERE ev.address = ?1 AND ev.rule = 'service' AND q.content_sha256 IS ev.content_sha256
            AND q.gone_at IS NULL
            AND (SELECT count(*) FROM (SELECT 1 FROM header h JOIN message e ON e.id = h.message_id
                  WHERE h.from_addr = ?1 AND e.internal_date < q.internal_date
                    AND e.gone_at IS NULL LIMIT 2)) >= 2)",
        [addr],
        |r| r.get(0),
    )?;
    if other {
        return Ok((Kind::Service, "another message qualifies".into()));
    }
    Ok((Kind::Stranger, "no rule matched".into()))
}

/// Write the engine kind of `addr`, and give the row ID. A row that a person
/// decided keeps its kind. A row that another crate made (kind NULL) gets
/// the kind, and keeps its name.
#[allow(clippy::too_many_arguments)]
fn upsert(
    rerun: &mut Vec<i64>,
    conn: &Connection,
    addr: &str,
    name: Option<&str>,
    kind: Kind,
    basis: &str,
    now_ms: i64,
) -> rusqlite::Result<i64> {
    let before: Option<Option<String>> = conn
        .query_row("SELECT kind FROM sender WHERE address = ?1", [addr], |r| {
            r.get(0)
        })
        .optional()?;
    conn.prepare_cached(
        "INSERT INTO sender(address, name, kind, basis, decided_by, decided_at)
              VALUES (?1, ?2, ?3, ?4, 'engine', ?5)
         ON CONFLICT(address) DO UPDATE SET
              name = coalesce(sender.name, excluded.name), kind = excluded.kind,
              basis = excluded.basis, decided_by = 'engine', decided_at = excluded.decided_at
         WHERE sender.decided_by IS NOT 'person'",
    )?
    .execute(params![addr, name, kind.name(), basis, now_ms])?;
    let (id, after): (i64, Option<String>) = conn.query_row(
        "SELECT id, kind FROM sender WHERE address = ?1",
        [addr],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    // A missing row or a NULL kind counts as not hostile.
    let hostile = |k: Option<&str>| k == Some(Kind::Hostile.name());
    if hostile(before.flatten().as_deref()) != hostile(after.as_deref()) {
        rerun.push(id);
    }
    Ok(id)
}

/// The domains of the known senders (kind `person` or `service`), except
/// the address `but`.
fn known_domains(conn: &Connection, but: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare_cached(
        "SELECT DISTINCT substr(address, instr(address, '@') + 1) FROM sender
          WHERE kind IN ('person', 'service') AND address != ?1",
    )?
    .query_map([but], |r| r.get(0))?
    .collect()
}

/// The known domain that the domain of `addr` is a near copy of.
fn near<'a>(addr: &str, known: &'a [String]) -> Option<&'a String> {
    known.iter().find(|k| near_copy(domain_of(addr), k))
}

fn domain_of(addr: &str) -> &str {
    addr.rsplit_once('@').map_or("", |(_, d)| d)
}

/// Labels that many unrelated domains share, so a match on them proves
/// nothing.
const COMMON: [&str; 26] = [
    "www", "web", "app", "api", "dev", "pay", "net", "com", "org", "new", "top", "the", "mail",
    "email", "info", "news", "smtp", "mailer", "accounts", "account", "support", "secure", "login",
    "service", "online", "notify",
];

/// True when `domain` is a near copy of the known domain `known`: a domain
/// that another party registered to look like it. Two rules:
/// - A label of `domain` has hyphen parts, and one part is a label of `known`
///   (`shop-secure.example.net` from `shop.example.com`).
/// - The name labels (the label before the public suffix) differ only by
///   look-alike characters (`paypa1` from `paypal`, `dh1` from `dhl`), or,
///   at 6 characters or more, by one edit (short real names such as
///   `gmail` and `email` must not match).
///
/// Domains with the same last two labels have one owner, so they are never
/// near copies. Neither are equal labels under another top-level domain
/// (`shop.de` and `shop.com`).
pub fn near_copy(domain: &str, known: &str) -> bool {
    if domain == known || base(domain) == base(known) {
        return false;
    }
    let (mine, theirs) = (names(domain), names(known));
    let telling: Vec<&String> = theirs
        .iter()
        .filter(|l| l.len() >= 3 && !COMMON.contains(&l.as_str()))
        .collect();
    for l in &mine {
        if l.contains('-') && l.split('-').any(|part| telling.iter().any(|t| *t == part)) {
            return true;
        }
    }
    match (mine.last(), theirs.last()) {
        // Look-alike characters count at each length (`dh1` from `dhl`).
        (Some(a), Some(b)) if a != b && fold(a) == fold(b) => true,
        (Some(a), Some(b)) if a != b && a.len() >= 6 && b.len() >= 6 => one_edit(a, b),
        _ => false,
    }
}

/// Hosting suffixes under which each name has another owner, so each one
/// is a public suffix (as in the public suffix list).
const HOSTED: [&str; 14] = [
    "github.io",
    "gitlab.io",
    "herokuapp.com",
    "vercel.app",
    "netlify.app",
    "pages.dev",
    "workers.dev",
    "web.app",
    "firebaseapp.com",
    "appspot.com",
    "blogspot.com",
    "azurewebsites.net",
    "cloudfront.net",
    "s3.amazonaws.com",
];

/// The number of labels at the end of `d` that form its public suffix.
/// shortcut: a hosting suffix of [`HOSTED`] counts its labels, a two-letter
/// country domain with a common second level (`co.uk`, `com.au`) has 2, each
/// other domain has 1; use the public suffix list if other suffixes give
/// wrong results.
fn suffix_len(labels: &[&str]) -> usize {
    let joined = labels.join(".");
    if let Some(h) = HOSTED.iter().find(|h| joined.ends_with(&format!(".{h}"))) {
        return h.split('.').count();
    }
    match labels {
        [.., second, tld]
            if tld.len() == 2
                && ["co", "com", "net", "org", "ac", "gov", "edu", "ne", "or"].contains(second) =>
        {
            2
        }
        _ => 1,
    }
}

/// The registrable domain: the suffix and one label more.
fn base(d: &str) -> Vec<&str> {
    let l: Vec<&str> = d.split('.').collect();
    let keep = (suffix_len(&l) + 1).min(l.len());
    l[l.len() - keep..].to_vec()
}

/// The labels in front of the public suffix. The last is the name label.
fn names(d: &str) -> Vec<String> {
    let l: Vec<&str> = d.split('.').collect();
    let n = l.len().saturating_sub(suffix_len(&l));
    l[..n].iter().map(|s| s.to_string()).collect()
}

/// Map look-alike characters to one form.
fn fold(s: &str) -> String {
    s.replace("rn", "m")
        .replace("vv", "w")
        .chars()
        .map(|c| match c {
            '0' => 'o',
            '1' | 'i' => 'l',
            '5' => 's',
            _ => c,
        })
        .collect()
}

/// True when `a` and `b` differ by one insert, delete, change, or swap of
/// two characters next to each other.
fn one_edit(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let (ra, rb) = (&a[pre..], &b[pre..]);
    match ra.len().abs_diff(rb.len()) {
        0 if ra.len() >= 2 && ra[0] == rb[1] && ra[1] == rb[0] => ra[2..] == rb[2..],
        0 => !ra.is_empty() && ra[1..] == rb[1..],
        1 if ra.len() > rb.len() => ra[1..] == *rb,
        1 => rb[1..] == *ra,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_copies() {
        let yes = [
            ("shop-secure.example.net", "shop.example.com"),
            ("paypal-verify.com", "accounts.paypal.com"),
            ("dhl-parcel.example.net", "dhl.example.com"),
            ("paypa1.com", "paypal.com"),
            ("paypall.com", "paypal.com"),
            ("pyapal.com", "paypal.com"),
            ("rnicrosoft.com", "microsoft.com"),
            ("dh1.com", "dhl.com"),
            ("paypa1.github.io", "paypal.github.io"),
            ("paypal-login.herokuapp.com", "paypal.herokuapp.com"),
            ("paypall.co.uk", "paypal.co.uk"),
            ("paypal-verify.co.uk", "paypal.co.uk"),
        ];
        for (d, k) in yes {
            assert!(near_copy(d, k), "{d} {k}");
        }
        let no = [
            ("shop.example.com", "shop.example.com"),
            ("mail2.shop.com", "mail.shop.com"),
            ("shop.de", "shop.com"),
            ("example.net", "example.org"),
            ("word.example.net", "shop.example.com"),
            ("mail-secure.other.net", "mail.example.com"),
            ("my-app.example.net", "app.example.com"),
            ("shop.co.uk", "shop.com"),
            ("mail.shop.co.uk", "shop.co.uk"),
            ("other.co.uk", "shop.co.uk"),
            ("mail.paypal.github.io", "paypal.github.io"),
            ("gmail.com", "email.com"),
            ("abc.com", "abd.com"),
            ("github.com", "gitlab.com"),
        ];
        for (d, k) in no {
            assert!(!near_copy(d, k), "{d} {k}");
        }
    }
}
