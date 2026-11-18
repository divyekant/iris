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

/// The answers of the person at the stranger gate.
pub mod gate;
/// The header parser of the sender rules. `iris-understand` reads the
/// same headers with it, so both crates agree on each value.
pub mod head;
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

    pub(crate) fn parse(s: &str) -> Option<Kind> {
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

/// What one call of [`decide`] changed: the rows for [`Decision::rerun`],
/// and the domains that became known or stopped being known.
#[derive(Default)]
struct Changes {
    rerun: Vec<i64>,
    domains: Vec<String>,
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
    let mut ch = Changes::default();
    let mut d = decide_message(conn, message, raw, now_ms, &mut ch)?;
    sweep(conn, now_ms, &mut ch)?;
    d.rerun = dedup(ch.rerun);
    Ok(d)
}

/// Derive each stored engine kind again from the store, and give the
/// `sender` rows that became `hostile` or stopped being `hostile`. The
/// caller runs understanding again on each message from these rows.
///
/// [`decide`] keeps the rows of one message current. Other changes of the
/// store change the evidence too: sync marks a message gone, a fetch of new
/// bytes replaces a qualifying message, a message loses `SENT`. Call
/// `refresh` after each batch of sync or understanding, in the same
/// transaction. Spec 1 section 6.7 says that the kinds are current after
/// each refresh.
///
/// It decides again each engine row with a known or hostile kind, and each
/// address with an evidence row (for example a service whose older mail
/// was understood after its newer mail), then each
/// row that the near-copy rule compares with a domain that changed, until
/// no kind changes.
pub fn refresh(conn: &Connection, now_ms: i64) -> rusqlite::Result<Vec<i64>> {
    let mut ch = Changes::default();
    let rows: Vec<String> = conn
        .prepare_cached(
            "SELECT address FROM sender WHERE decided_by IS NOT 'person'
               AND kind IN ('person', 'service', 'hostile')
             UNION SELECT address FROM sender_evidence",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for a in &rows {
        recheck(conn, a, now_ms, &mut ch)?;
    }
    // A person can set a kind at any time, with no call to this crate. So
    // each domain of a row that a person decided goes to the sweep too.
    let set: Vec<String> = conn
        .prepare_cached(
            "SELECT DISTINCT substr(address, instr(address, '@') + 1) FROM sender
              WHERE decided_by = 'person'",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for d in set {
        if !ch.domains.contains(&d) {
            ch.domains.push(d);
        }
    }
    sweep(conn, now_ms, &mut ch)?;
    Ok(dedup(ch.rerun))
}

fn dedup(mut ids: Vec<i64>) -> Vec<i64> {
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| seen.insert(*id));
    ids
}

/// Decide again each row that the near-copy rule compares with a domain in
/// `ch.domains` (a domain that became known or stopped being known), until
/// no domain changes.
/// shortcut: each pass reads each sender row once; keep a domain index if a
/// large mailbox makes it slow. The passes stop after 8 (a chain of 8
/// look-alike domains that each change the next one).
fn sweep(conn: &Connection, now_ms: i64, ch: &mut Changes) -> rusqlite::Result<()> {
    for _ in 0..8 {
        let domains = std::mem::take(&mut ch.domains);
        if domains.is_empty() {
            break;
        }
        let rows: Vec<String> = conn
            .prepare_cached("SELECT address FROM sender WHERE decided_by IS NOT 'person'")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for a in rows
            .iter()
            .filter(|a| domains.iter().any(|d| near_copy(domain_of(a), d)))
        {
            recheck(conn, a, now_ms, ch)?;
        }
    }
    Ok(())
}

fn decide_message(
    conn: &Connection,
    message: i64,
    raw: &[u8],
    now_ms: i64,
    ch: &mut Changes,
) -> rusqlite::Result<Decision> {
    let head = Head::parse(raw);
    let (date, sha, account, provider): (Option<i64>, Option<String>, String, String) = conn
        .query_row(
            "SELECT m.internal_date, m.content_sha256, a.address, a.provider FROM message m
           JOIN account a ON a.id = m.account_id WHERE m.id = ?1",
            [message],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
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
    // This decision writes the evidence of this message again. Each
    // recipient that loses its `sent` row is decided again after it.
    let old: Vec<String> = conn
        .prepare_cached(
            "SELECT address FROM sender_evidence WHERE message_id = ?1
               AND rule IN ('sent', 'replied')",
        )?
        .query_map([message], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    conn.prepare_cached("DELETE FROM sender_evidence WHERE message_id = ?1")?
        .execute([message])?;
    if !has("SENT") || has("DRAFT") {
        for a in &old {
            recheck(conn, a, now_ms, ch)?;
        }
    }

    if has("DRAFT") {
        // A draft does not count for the `person` rule.
        return Ok(Decision {
            rerun: Vec::new(),
            ..own(if has("SENT") { "sent" } else { "draft" })
        });
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
        // The `hostile` rule comes before the `person` rule. A recipient that
        // becomes a person is a known sender for the next recipients of the
        // same message.
        // Old recipients first: a domain that loses its only known sender is
        // not known for the new recipients.
        for a in old.iter().filter(|a| !recipients.contains(a)) {
            recheck(conn, a, now_ms, ch)?;
        }
        // You wrote these names: a new person row keeps the name.
        let names = head.recipient_names();
        let name_of = |to: &str| {
            names
                .iter()
                .find(|(a, n)| a == to && n.is_some())
                .and_then(|(_, n)| n.as_deref())
        };
        let mut known = known_domains(conn, "")?;
        for to in &recipients {
            conn.prepare_cached(
                "INSERT OR IGNORE INTO sender_evidence(message_id, address, rule, content_sha256)
                 VALUES (?1, ?2, 'sent', ?3)",
            )?
            .execute(params![message, to, sha])?;
            match near(to, &known) {
                Some(k) => upsert(
                    ch,
                    conn,
                    to,
                    None,
                    Kind::Hostile,
                    &format!("near copy of {k}"),
                    now_ms,
                )
                .map(drop)?,
                None => {
                    upsert(ch, conn, to, name_of(to), Kind::Person, "sent", now_ms)?;
                    // A row that a person set keeps its kind: only a known
                    // sender adds its domain.
                    let kind: Option<String> =
                        conn.query_row("SELECT kind FROM sender WHERE address = ?1", [to], |r| {
                            r.get(0)
                        })?;
                    if matches!(kind.as_deref(), Some("person" | "service")) {
                        known.push(domain_of(to).to_string());
                    }
                }
            };
        }

        // The `replied` rule: each earlier sender of the thread. Its row
        // is decided again now, and by each refresh.
        let earlier: Vec<String> = conn
            .prepare_cached(&format!(
                "SELECT DISTINCT h.from_addr FROM message m JOIN header h ON h.message_id = m.id
                  WHERE m.thread_id = (SELECT thread_id FROM message WHERE id = ?1)
                    AND m.id != ?1 AND m.internal_date < ?2 AND m.gone_at IS NULL AND {}
                    AND h.from_addr IS NOT NULL AND h.from_addr != '' AND h.from_addr != lower(?3)",
                received("m")
            ))?
            .query_map(params![message, date.unwrap_or(i64::MIN), account], |r| {
                r.get(0)
            })?
            .collect::<rusqlite::Result<_>>()?;
        for a in &earlier {
            conn.prepare_cached(
                "INSERT OR IGNORE INTO sender_evidence(message_id, address, rule, content_sha256)
                 VALUES (?1, ?2, 'replied', ?3)",
            )?
            .execute(params![message, a, sha])?;
            recheck(conn, a, now_ms, ch)?;
        }
        return Ok(Decision {
            rerun: Vec::new(),
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
    // The `service` rule: this message is automated mail with a Gmail DKIM
    // pass. The rule counts its earlier messages when it reads the row, so
    // an older message that is understood later counts too.
    let authserv = head::trusted_authserv(&provider);
    if head.automated() && head.dkim_pass(domain_of(&addr), authserv) {
        conn.prepare_cached(
            "INSERT OR IGNORE INTO sender_evidence(message_id, address, rule, content_sha256)
             VALUES (?1, ?2, 'service', ?3)",
        )?
        .execute(params![message, addr, sha])?;
    }
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

    let (kind, basis) = rules(conn, &addr)?;
    let id = upsert(ch, conn, &addr, name.as_deref(), kind, &basis, now_ms)?;
    Ok(Decision {
        rerun: Vec::new(),
        kind,
        sender: Some(id),
        basis,
        previous,
    })
}

/// True when the owner sent mail to `addr` (lower case): a `sent` evidence
/// row whose message is a sent message, not gone, with the same bytes.
/// The changeset flag `new_recipient` uses the same rule.
pub fn sent_to(conn: &Connection, addr: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM sender_evidence e JOIN message s ON s.id = e.message_id
              WHERE e.address = ?1 AND e.rule = 'sent' AND s.content_sha256 IS e.content_sha256
                AND {SENT_NOT_DRAFT})"
        ),
        [addr],
        |r| r.get(0),
    )
}

/// True when the message `s` is not gone and has the label `SENT` and not
/// `DRAFT`.
const SENT_NOT_DRAFT: &str =
    "(s.gone_at IS NULL AND EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                  WHERE ml.message_id = s.id AND l.provider_id = 'SENT')
      AND NOT EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                  WHERE ml.message_id = s.id AND l.provider_id = 'DRAFT'))";

/// SQL that is true when the message with the alias `m` is received mail:
/// no label `SENT` or `DRAFT`.
fn received(m: &str) -> String {
    format!(
        "NOT EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                      WHERE ml.message_id = {m}.id AND l.provider_id IN ('SENT', 'DRAFT'))"
    )
}

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

/// Decide the address `addr` again with no message from it, from the
/// evidence in the store.
fn recheck(conn: &Connection, addr: &str, now_ms: i64, ch: &mut Changes) -> rusqlite::Result<()> {
    let (kind, basis) = rules(conn, addr)?;
    upsert(ch, conn, addr, None, kind, &basis, now_ms)?;
    Ok(())
}

/// The engine rules in the order of spec 1 section 6.7: `hostile`, then
/// `person`, then `service`, else `stranger`. They read only the store, so
/// [`refresh`] gives the same result as [`decide`].
fn rules(conn: &Connection, addr: &str) -> rusqlite::Result<(Kind, String)> {
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
    if sent_to(conn, addr)? {
        return Ok((Kind::Person, "sent".into()));
    }
    // shortcut: this query reads each message from the address, so its cost
    // grows with the history of the address (about 1.7 ms for each message on the
    // 100,000-message synthetic mailbox, release build). Keep a "replied"
    // flag in the store if a large mailbox makes it slow.
    // The `replied` rule: a sent message in a thread after a message from
    // the address. A sent message before the address joined the thread does
    // not count, and neither does one at the same time (no order is known). A sent message of this thread to the address also counts
    // (the `sent` rule), because a sent message that is older than this one
    // can still wait for its own decision (the engine reads newest first).
    let replied: bool = conn.query_row(
        &format!(
            "SELECT EXISTS (
               SELECT 1 FROM header hm JOIN message m ON m.id = hm.message_id
                 JOIN message s ON s.thread_id = m.thread_id AND s.id != m.id
                 JOIN header h ON h.message_id = s.id
                WHERE hm.from_addr = ?1 AND m.gone_at IS NULL AND {} AND {SENT_NOT_DRAFT}
                  AND (s.internal_date > m.internal_date OR {}))",
            received("m"),
            to_addr(1)
        ),
        [addr],
        |r| r.get(0),
    )?;
    if replied {
        return Ok((Kind::Person, "replied".into()));
    }

    // The `service` rule: a message with a `service` row qualifies while
    // it is there with the same bytes (so the same automated and DKIM
    // headers) and has 2 earlier messages from the address.
    let service: bool = conn.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM sender_evidence ev JOIN message q ON q.id = ev.message_id
              WHERE ev.address = ?1 AND ev.rule = 'service' AND q.content_sha256 IS ev.content_sha256
                AND q.gone_at IS NULL AND {}
                AND (SELECT count(*) FROM (SELECT 1 FROM header h JOIN message e ON e.id = h.message_id
                      WHERE h.from_addr = ?1 AND e.internal_date < q.internal_date
                        AND e.gone_at IS NULL AND {} LIMIT 2)) >= 2)",
            received("q"),
            received("e")
        ),
        [addr],
        |r| r.get(0),
    )?;
    if service {
        return Ok((
            Kind::Service,
            "automated, dkim pass, 2 earlier messages".into(),
        ));
    }
    Ok((Kind::Stranger, "no rule matched".into()))
}

/// Write the engine kind of `addr`, and give the row ID. A row that a person
/// decided keeps its kind. A row that another crate made (kind NULL) gets
/// the kind, and keeps its name.
#[allow(clippy::too_many_arguments)]
fn upsert(
    ch: &mut Changes,
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
    let before = before.flatten();
    if hostile(before.as_deref()) != hostile(after.as_deref()) {
        ch.rerun.push(id);
    }
    let known = |k: Option<&str>| matches!(k, Some("person" | "service"));
    if known(before.as_deref()) != known(after.as_deref()) {
        let d = domain_of(addr).to_string();
        if !ch.domains.contains(&d) {
            ch.domains.push(d);
        }
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
/// Domains with the same registrable domain have one owner, so they are
/// never near copies. A shared domain (a public suffix such as `github.io`)
/// has a registrable domain for each name under it. Equal labels under
/// another top-level domain are not near copies (`shop.de` and `shop.com`).
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
const HOSTED: [&str; 34] = [
    "wordpress.com",
    "blogspot.com.au",
    "wixsite.com",
    "weebly.com",
    "tumblr.com",
    "substack.com",
    "myshopify.com",
    "squarespace.com",
    "webflow.io",
    "notion.site",
    "glitch.me",
    "onrender.com",
    "fly.dev",
    "surge.sh",
    "ngrok.io",
    "azurestaticapps.net",
    "duckdns.org",
    "ddns.net",
    "no-ip.org",
    "dynu.net",
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

/// The number of labels at the end of `d` that form its public suffix: the
/// longer of the suffix of the public suffix list (with its private part,
/// for example `github.io`) and a hosting suffix of [`HOSTED`]. The list
/// has no rule for some hosts of [`HOSTED`], for example `wordpress.com`.
/// A domain with no known suffix has a suffix of one label.
fn suffix_len(labels: &[&str]) -> usize {
    let joined = labels.join(".");
    let listed = psl::suffix_str(&joined).map_or(1, |s| s.split('.').count());
    let hosted = HOSTED
        .iter()
        .find(|h| joined.ends_with(&format!(".{h}")))
        .map_or(0, |h| h.split('.').count());
    listed.max(hosted)
}

/// The registrable domain of `d`: the public suffix and one label more.
/// Two domains with the same registrable domain have one owner.
pub fn registrable(d: &str) -> String {
    base(d).join(".")
}

/// True when the public suffix list knows the suffix of `d` (`com`,
/// `co.uk`, `xn--p1ai`), so `d` reads as a domain and not as a file name
/// (`report.pdf`). `d` is lowercase ASCII (punycode).
pub fn known_suffix(d: &str) -> bool {
    psl::suffix(d.as_bytes()).is_some_and(|s| s.is_known())
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
            ("paypa1.duckdns.org", "paypal.duckdns.org"),
            ("paypa1.wordpress.com", "paypal.wordpress.com"),
            ("paypa1.me.uk", "paypal.me.uk"),
            ("paypal-login.pages.dev", "paypal.pages.dev"),
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
            ("mail2.shop.com", "mail.shop.com"),
            ("eu.shop.com", "us.shop.com"),
            ("a.b.shop.com", "b.shop.com"),
            ("bi1ling.example.com", "billing.example.com"),
            ("rnicrosoft.example.net", "microsoft.example.net"),
            ("other.me.uk", "shop.me.uk"),
            ("mail.shop.me.uk", "shop.me.uk"),
        ];
        for (d, k) in no {
            assert!(!near_copy(d, k), "{d} {k}");
        }
    }
}
