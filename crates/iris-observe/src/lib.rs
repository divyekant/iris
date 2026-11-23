//! Observe mode (release 1): the triage proposal.
//!
//! The rules of triage: which inbox messages each rule proposes to archive
//! (specification 2, section 5), and the count of each rule. `iris-triage`
//! stores the proposal as a changeset of the author `triage`.
//!
//! This crate has no Gmail client and opens the store only to read. So it
//! cannot change the mailbox or the store (specification 2, requirement C1).
//!
//! Memory: the counts are SQL `count(*)` queries. `iris-triage` stores the
//! proposal as a changeset with the rules of this crate.

use iris_store::Store;

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
    /// Read the rules in one transaction, so no message is in two rules.
    /// No rule takes a message that waits for its link check (store
    /// migration 20): its items can be of mail that is hostile.
    pub fn query(self) -> String {
        let mut q = self.select().to_string();
        q.push_str(" EXCEPT SELECT message_id FROM link_check");
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
