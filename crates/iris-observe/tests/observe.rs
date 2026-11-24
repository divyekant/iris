//! Observe mode on a first sync of the synthetic mailbox. No network.

use std::collections::BTreeMap;
use std::time::Duration;

use iris_fakegmail::FakeGmail;
use iris_gmail::{Config, Gmail};
use iris_observe::{counts, Rule};
use iris_store::Store;
use iris_synth::{Form, Mailbox, Sender};

const TOKEN: &str = "test-token";
const SEED: u64 = 11;
const COUNT: usize = 400;

/// The understanding step is not in this crate. So the test makes the items
/// from the truth of the synthetic mailbox: one item for each message that
/// is not hostile and not from the owner. Signals of odd index are expired.
fn fill_items(store: &Store, mailbox: &Mailbox) {
    store
        .write(|tx| {
            for i in 0..mailbox.len() {
                let t = mailbox.truth(i);
                if matches!(t.sender, Sender::Owner | Sender::Hostile) {
                    continue;
                }
                let status = match t.form {
                    Form::Signal if i % 2 == 1 => "expired",
                    Form::Signal => "active",
                    Form::Feed => "new",
                    Form::Record => "kept",
                    Form::Commitment => "open",
                    Form::Conversation => "active",
                };
                tx.execute(
                    "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                     VALUES (?1, 1, ?2, '{}', 0, 0)",
                    (t.form.name(), status),
                )?;
                tx.execute(
                    "INSERT INTO item_source(item_id, message_id, role)
                     SELECT last_insert_rowid(), id, 'origin' FROM message WHERE provider_id = ?1",
                    [FakeGmail::id_of(i)],
                )?;
            }
            // Each batch run first ends the link check of new mail (store
            // migration 20).
            tx.execute("DELETE FROM link_check", [])?;
            // The engine marks hostile mail (specification 1, section 6.7).
            tx.execute(
                "INSERT INTO sender(address, kind) VALUES ('h@example.net', 'hostile')",
                [],
            )?;
            for i in 0..mailbox.len() {
                if mailbox.truth(i).sender == Sender::Hostile {
                    tx.execute(
                        "UPDATE message SET hostile = '[\"phishing\"]' WHERE provider_id = ?1",
                        [FakeGmail::id_of(i)],
                    )?;
                }
            }
            Ok(())
        })
        .unwrap();
}

fn rule_of(mailbox: &Mailbox, i: usize) -> Option<Rule> {
    let t = mailbox.truth(i);
    match (t.sender, t.form) {
        (Sender::Hostile, _) => Some(Rule::Hostile),
        (Sender::Owner, _) => None,
        (_, Form::Signal) if i % 2 == 1 => Some(Rule::ExpiredSignal),
        (_, Form::Feed) => Some(Rule::Feed),
        (_, Form::Record) => Some(Rule::Record),
        _ => None,
    }
}

#[test]
fn the_first_run_proposes_archives_and_makes_zero_writes_to_gmail() {
    let mailbox = Mailbox::new(SEED, COUNT);
    let fake = FakeGmail::start(Mailbox::new(SEED, COUNT)).unwrap();
    let gmail = Gmail::new(Config {
        base_url: fake.base_url(),
        quota_per_minute: 10_000_000,
        backoff_start: Duration::from_millis(1),
        backoff_max: Duration::from_millis(4),
        ..Config::default()
    });
    let dir = std::env::temp_dir().join(format!("iris-observe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("gmail", "owner@example.com").unwrap();
    iris_sync::first_sync(&store, &gmail, account, TOKEN).unwrap();
    fill_items(&store, &mailbox);

    // A feed message that is also hostile goes to the hostile rule only.
    let feed = (0..COUNT)
        .find(|&i| rule_of(&mailbox, i) == Some(Rule::Feed) && i % 40 != 1)
        .unwrap();
    store
        .write(|tx| {
            tx.execute(
                "UPDATE header SET from_addr = 'h@example.net' WHERE message_id =
                   (SELECT id FROM message WHERE provider_id = ?1)",
                [FakeGmail::id_of(feed)],
            )?;
            Ok(())
        })
        .unwrap();

    // The expected messages of each rule: the truth, and the label INBOX.
    let row_of = |i: usize| -> (i64, bool) {
        store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT m.id, EXISTS (SELECT 1 FROM message_label ml JOIN label l
                       ON l.id = ml.label_id WHERE ml.message_id = m.id AND l.provider_id = 'INBOX')
                     FROM message m WHERE m.provider_id = ?1",
                    [FakeGmail::id_of(i)],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap()
    };
    let mut expected: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for i in 0..COUNT {
        let rule = if i == feed {
            Some(Rule::Hostile)
        } else {
            rule_of(&mailbox, i)
        };
        let (row, in_inbox) = row_of(i);
        if let (Some(rule), true) = (rule, in_inbox) {
            expected
                .entry(rule.why())
                .or_default()
                .push(format!("m-{row}"));
        }
    }
    for ids in expected.values_mut() {
        ids.sort_by_key(|id| id[2..].parse::<i64>().unwrap());
    }
    assert_eq!(expected.len(), 4, "the sample has each rule: {expected:?}");

    let before = fake.requests().len();
    let got = counts(&store, account).unwrap();
    for (rule, n) in &got {
        assert_eq!(*n as usize, expected[rule.why()].len(), "count of {rule:?}");
    }

    // The messages of each rule: the query of the rule, in ID order.
    for rule in Rule::ALL {
        let ids: Vec<String> = store
            .read(|c| {
                let mut st = c.prepare(&format!("{} ORDER BY 1", rule.query()))?;
                let v = st
                    .query_map([account], |r| r.get::<_, i64>(0))?
                    .map(|id| id.map(|id| format!("m-{id}")))
                    .collect::<rusqlite::Result<_>>()?;
                Ok(v)
            })
            .unwrap();
        assert_eq!(ids, expected[rule.why()], "messages of {rule:?}");
    }

    // A message that waits for its link check is in no rule: its items can
    // be of hostile mail. Each rule loses one message.
    let mut waits = Vec::new();
    for (rule, ids) in &expected {
        let row: i64 = ids[0][2..].parse().unwrap();
        store
            .write(|tx| Ok(tx.execute("INSERT INTO link_check(message_id) VALUES (?1)", [row])?))
            .unwrap();
        waits.push((*rule, ids.len() - 1));
    }
    let got = counts(&store, account).unwrap();
    for (rule, n) in &got {
        let want = waits.iter().find(|(w, _)| *w == rule.why()).unwrap().1;
        assert_eq!(*n as usize, want, "count of {rule:?} with a queued message");
    }

    // Observe mode sends nothing to Gmail, and the first sync only reads.
    assert_eq!(fake.requests().len(), before, "observe mode made a request");
    let writes: Vec<String> = fake
        .requests()
        .into_iter()
        .filter(|r| !r.starts_with("GET "))
        .collect();
    assert!(writes.is_empty(), "write calls to Gmail: {writes:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_store_gives_zero_in_each_rule() {
    let dir = std::env::temp_dir().join(format!("iris-observe-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("gmail", "owner@example.com").unwrap();
    assert!(counts(&store, account)
        .unwrap()
        .iter()
        .all(|(_, n)| *n == 0));
    let _ = std::fs::remove_dir_all(&dir);
}
