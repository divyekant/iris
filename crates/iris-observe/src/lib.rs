//! Observe mode (release 1): the triage proposal.
//!
//! Triage reads the forms in the store and computes the mailbox changes that
//! Iris would propose (specification 2, section 5). In release 1 the mode is
//! always `observe`, so each action stays `proposed`, and the engine applies
//! nothing. Release 1 does not store the proposal (specification 1,
//! section 8): the engine computes it when the brief opens.
//!
//! This crate has no Gmail client and opens the store only to read. So it
//! cannot change the mailbox or the store (specification 2, requirement C1).
//!
//! Memory: the counts are SQL `count(*)` queries. [`write_changeset`] streams
//! each message ID to the writer, so the full list is never in memory.

use std::io::Write;

use iris_store::{format_id, Kind, Store};

/// The flag `bulk`: more than this count of messages in one action, or in
/// one changeset (specification 2, section 5.3).
pub const BULK: u64 = 500;

/// One triage rule. Each rule gives one `archive` action. Archive only
/// removes `INBOX`: the mail stays in Gmail, and the undo adds `INBOX` again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// Hostile mail in the inbox (specification 1, section 6.7).
    Hostile,
    /// A signal with the status `expired`.
    ExpiredSignal,
    /// A feed item: Iris shows it in the feed, so the inbox does not need it.
    Feed,
    /// A record: Iris keeps it, so the inbox does not need it.
    Record,
}

impl Rule {
    /// The rules in order. A message goes to the first rule that matches it,
    /// so no message is in two actions.
    pub const ALL: [Rule; 4] = [Rule::Hostile, Rule::ExpiredSignal, Rule::Feed, Rule::Record];

    pub fn verb(self) -> &'static str {
        "archive"
    }

    pub fn why(self) -> &'static str {
        match self {
            Rule::Hostile => "Hostile mail. Iris shows a count only.",
            Rule::ExpiredSignal => "Signals that stopped to matter.",
            Rule::Feed => "Feed items. They stay in the feed of Iris.",
            Rule::Record => "Records. Iris keeps them in records.",
        }
    }

    /// The messages that the rule matches: not gone, with the label `INBOX`.
    fn select(self) -> &'static str {
        match self {
            Rule::Hostile => {
                "SELECT m.id FROM message m WHERE m.account_id = ?1 AND m.gone_at IS NULL
                   AND m.id IN (SELECT ml.message_id FROM message_label ml
                     JOIN label l ON l.id = ml.label_id WHERE l.provider_id = 'INBOX')
                   AND ((m.hostile IS NOT NULL AND m.hostile NOT IN ('', '[]'))
                     OR EXISTS (SELECT 1 FROM header h JOIN sender p ON p.address = h.from_addr
                       WHERE h.message_id = m.id AND p.kind = 'hostile'))"
            }
            Rule::ExpiredSignal => {
                "SELECT m.id FROM item i JOIN item_source s ON s.item_id = i.id
                   JOIN message m ON m.id = s.message_id
                 WHERE i.account_id = ?1 AND i.form = 'signal' AND i.status = 'expired'
                   AND m.gone_at IS NULL
                   AND m.id IN (SELECT ml.message_id FROM message_label ml
                     JOIN label l ON l.id = ml.label_id WHERE l.provider_id = 'INBOX')"
            }
            Rule::Feed => {
                "SELECT m.id FROM item i JOIN item_source s ON s.item_id = i.id
                   JOIN message m ON m.id = s.message_id
                 WHERE i.account_id = ?1 AND i.form = 'feed' AND i.status != 'gone'
                   AND m.gone_at IS NULL
                   AND m.id IN (SELECT ml.message_id FROM message_label ml
                     JOIN label l ON l.id = ml.label_id WHERE l.provider_id = 'INBOX')"
            }
            Rule::Record => {
                "SELECT m.id FROM item i JOIN item_source s ON s.item_id = i.id
                   JOIN message m ON m.id = s.message_id
                 WHERE i.account_id = ?1 AND i.form = 'record' AND i.status != 'gone'
                   AND m.gone_at IS NULL
                   AND m.id IN (SELECT ml.message_id FROM message_label ml
                     JOIN label l ON l.id = ml.label_id WHERE l.provider_id = 'INBOX')"
            }
        }
    }

    /// The message row IDs of the rule, minus the messages of earlier rules.
    fn query(self) -> String {
        let mut q = self.select().to_string();
        for earlier in Rule::ALL.iter().take_while(|r| **r != self) {
            q.push_str(" EXCEPT ");
            q.push_str(earlier.select());
        }
        q
    }
}

/// The count of messages of each rule, in the order of [`Rule::ALL`].
pub fn counts(store: &Store, account: i64) -> iris_store::Result<Vec<(Rule, u64)>> {
    store.read(|c| {
        let tx = c.unchecked_transaction()?;
        count_in(&tx, account)
    })
}

fn count_in(c: &rusqlite::Connection, account: i64) -> iris_store::Result<Vec<(Rule, u64)>> {
    Rule::ALL
        .iter()
        .map(|&rule| {
            let sql = format!("SELECT count(*) FROM ({})", rule.query());
            let n: i64 = c.query_row(&sql, [account], |r| r.get(0))?;
            Ok((rule, n as u64))
        })
        .collect()
}

/// Write the triage changeset as JSON (specification 2, section 5.1).
///
/// `created` is the time in RFC 3339. Each call gives a new ID (see
/// [`next_id`]). A rule with no message gives no action. Each action has the
/// state `proposed`.
pub fn write_changeset(
    store: &Store,
    account: i64,
    created: &str,
    out: &mut impl Write,
) -> iris_store::Result<()> {
    store.read(|c| {
        // One read transaction, so the counts and each ID list come from one
        // snapshot, also when the sync writes at the same time.
        let tx = c.unchecked_transaction()?;
        let c = &*tx;
        let counts = count_in(c, account)?;
        let total: u64 = counts.iter().map(|(_, n)| n).sum();
        write!(
            out,
            r#"{{"model":"iris/0","id":"cs-{id}","revision":1,"author":{{"kind":"triage","name":"Iris"}},"why":"Triage proposes these mailbox changes. Nothing is applied.","created":{created:?},"submitted":{created:?},"state":"{state}","actions":["#,
            id = next_id(),
            state = if total == 0 { "closed" } else { "open" },
        )?;
        let mut n = 0;
        for (rule, count) in counts.into_iter().filter(|(_, n)| *n > 0) {
            n += 1;
            if n > 1 {
                out.write_all(b",")?;
            }
            write!(out, r#"{{"n":{n},"verb":"{}","targets":{{"messages":["#, rule.verb())?;
            write_ids(c, rule, account, out)?;
            out.write_all(br#"]},"resolved":{"messages":["#)?;
            write_ids(c, rule, account, out)?;
            let bulk = count > BULK || total > BULK;
            write!(
                out,
                r#"]}},"args":{{}},"why":"{}","flags":[{}],"decision":{{"state":"proposed"}}}}"#,
                rule.why(),
                if bulk { r#""bulk""# } else { "" },
            )?;
        }
        out.write_all(b"]}")?;
        Ok(())
    })
}

/// A changeset number that no other call gives: the time in microseconds at
/// the first call, plus one for each later call in the process.
// shortcut: unique only while the clock does not go back; release 2 stores
// changesets and uses the row ID.
fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| d.as_micros() as u64);
    // Never below the clock, so a new process does not give an old number.
    let mut cur = NEXT.load(Ordering::Relaxed);
    loop {
        let id = cur.max(now) + 1;
        match NEXT.compare_exchange(cur, id, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return id,
            Err(seen) => cur = seen,
        }
    }
}

fn write_ids(
    c: &rusqlite::Connection,
    rule: Rule,
    account: i64,
    out: &mut impl Write,
) -> iris_store::Result<()> {
    let mut st = c.prepare(&format!("{} ORDER BY 1", rule.query()))?;
    let mut rows = st.query([account])?;
    let mut first = true;
    while let Some(row) = rows.next()? {
        if !first {
            out.write_all(b",")?;
        }
        first = false;
        write!(out, "\"{}\"", format_id(Kind::Message, row.get(0)?))?;
    }
    Ok(())
}
