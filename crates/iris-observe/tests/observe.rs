//! Observe mode on a first sync of the synthetic mailbox. No network.

use std::collections::BTreeMap;
use std::time::Duration;

use iris_fakegmail::FakeGmail;
use iris_gmail::{Config, Gmail};
use iris_observe::{counts, write_changeset, Rule};
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

    let mut out = Vec::new();
    write_changeset(&store, account, "2026-11-16T09:00:00Z", &mut out).unwrap();
    let cs: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(cs["author"]["kind"], "triage");
    assert_eq!(cs["state"], "open");
    let actions = cs["actions"].as_array().unwrap();
    assert_eq!(actions.len(), 4);
    for a in actions {
        assert_eq!(a["verb"], "archive");
        assert_eq!(a["decision"]["state"], "proposed");
        let ids: Vec<String> = a["resolved"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, expected[a["why"].as_str().unwrap()]);
        assert_eq!(a["targets"], a["resolved"]);
        assert_eq!(
            a["flags"],
            serde_json::json!([]),
            "400 messages is not bulk"
        );
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
fn an_empty_store_gives_a_closed_changeset_with_no_action() {
    let dir = std::env::temp_dir().join(format!("iris-observe-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("gmail", "owner@example.com").unwrap();
    assert!(counts(&store, account)
        .unwrap()
        .iter()
        .all(|(_, n)| *n == 0));
    let mut out = Vec::new();
    write_changeset(&store, account, "2026-11-16T09:00:00Z", &mut out).unwrap();
    let cs: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(cs["state"], "closed");
    assert_eq!(cs["actions"], serde_json::json!([]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A writer that changes the store through a second handle at its first
/// write, as a sync can do while the brief reads.
struct ChangeOnWrite<'a> {
    other: Option<&'a Store>,
    out: Vec<u8>,
}

impl std::io::Write for ChangeOnWrite<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(store) = self.other.take() {
            store
                .write(|tx| {
                    tx.execute("DELETE FROM message_label", [])?;
                    Ok(())
                })
                .unwrap();
        }
        self.out.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_changeset_comes_from_one_snapshot_when_the_store_changes() {
    let dir = std::env::temp_dir().join(format!("iris-observe-snap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO label(account_id, provider_id, name, kind) VALUES (1, 'INBOX', 'INBOX', 'system');
                 INSERT INTO thread(account_id, provider_id) VALUES (1, 't1');
                 INSERT INTO message(account_id, thread_id, provider_id) VALUES (1, 1, 'a'), (1, 1, 'b');
                 INSERT INTO message_label(message_id, label_id) VALUES (1, 1), (2, 1);
                 INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                   VALUES ('feed', 1, 'new', '{}', 0, 0);
                 INSERT INTO item_source(item_id, message_id, role) VALUES (1, 1, 'origin'), (1, 2, 'update');",
            )?;
            Ok(())
        })
        .unwrap();
    let other = Store::open(&dir).unwrap();
    let mut w = ChangeOnWrite {
        other: Some(&other),
        out: Vec::new(),
    };
    write_changeset(&store, account, "2026-11-16T09:00:00Z", &mut w).unwrap();
    let cs: serde_json::Value = serde_json::from_slice(&w.out).unwrap();
    assert_eq!(
        cs["actions"][0]["resolved"]["messages"],
        serde_json::json!(["m-1", "m-2"]),
        "{cs}"
    );
    // After the run, the change is visible.
    assert!(counts(&store, account)
        .unwrap()
        .iter()
        .all(|(_, n)| *n == 0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The flags of each action, for `feed` feed messages and `records` record
/// messages, all in the inbox.
fn flags_for(feed: usize, records: usize) -> Vec<serde_json::Value> {
    let dir = std::env::temp_dir().join(format!(
        "iris-observe-bulk-{feed}-{records}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO label(account_id, provider_id, name, kind) VALUES (1, 'INBOX', 'INBOX', 'system');
                 INSERT INTO thread(account_id, provider_id) VALUES (1, 't1');",
            )?;
            for i in 0..feed + records {
                let form = if i < feed { "feed" } else { "record" };
                tx.execute(
                    "INSERT INTO message(account_id, thread_id, provider_id) VALUES (1, 1, ?1)",
                    [format!("x{i}")],
                )?;
                tx.execute_batch(&format!(
                    "INSERT INTO message_label(message_id, label_id) VALUES (last_insert_rowid(), 1);
                     INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                       VALUES ('{form}', 1, 'new', '{{}}', 0, 0);
                     INSERT INTO item_source(item_id, message_id, role)
                       VALUES (last_insert_rowid(), (SELECT max(id) FROM message), 'origin');"
                ))?;
            }
            Ok(())
        })
        .unwrap();
    let mut out = Vec::new();
    write_changeset(&store, account, "2026-11-16T09:00:00Z", &mut out).unwrap();
    let cs: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    cs["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["flags"].clone())
        .collect()
}

#[test]
fn the_bulk_flag_starts_above_500_messages_in_one_action_or_in_all() {
    let none = serde_json::json!([]);
    let bulk = serde_json::json!(["bulk"]);
    assert_eq!(flags_for(500, 0), vec![none.clone()]);
    assert_eq!(flags_for(501, 0), vec![bulk.clone()]);
    assert_eq!(flags_for(250, 250), vec![none.clone(), none]);
    assert_eq!(flags_for(250, 251), vec![bulk.clone(), bulk]);
}
