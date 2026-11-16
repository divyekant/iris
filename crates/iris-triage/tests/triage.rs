//! Triage stores the proposal of observe mode as a changeset. No network.

use std::fs;
use std::path::PathBuf;

use iris_observe::Rule;
use iris_store::{NewMessage, Store};
use iris_triage::{run, Outcome, CHUNK, WHY};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-triage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const NOW: i64 = 1_763_283_422_000;

/// A store with one account and the inbox messages g1 to g4, and no item.
fn store(dir: &TempDir) -> (Store, i64) {
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    s.put_labels(a, &[("INBOX", "INBOX", "system")]).unwrap();
    let msgs: Vec<NewMessage> = ["g1", "g2", "g3", "g4"]
        .iter()
        .map(|id| NewMessage {
            provider_id: id.to_string(),
            thread_provider_id: format!("t{id}"),
            labels: vec!["INBOX".into()],
            ..Default::default()
        })
        .collect();
    s.put_messages(a, &msgs).unwrap();
    (s, a)
}

/// An item of `form` with the origin message `provider`.
fn item(s: &Store, account: i64, form: &str, provider: &str) {
    s.write(|tx| {
        tx.execute(
            "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
             VALUES (?1, ?2, 'new', '{}', 0, 0)",
            rusqlite::params![form, account],
        )?;
        tx.execute(
            "INSERT INTO item_source(item_id, message_id, role)
             SELECT last_insert_rowid(), id, 'origin' FROM message WHERE provider_id = ?1",
            [provider],
        )?;
        Ok(())
    })
    .unwrap();
}

/// (revision, n, verb, why, state, targets) of each action, in order.
type Row = (i64, i64, String, Option<String>, Option<String>, String);

fn actions(s: &Store, changeset: i64) -> Vec<Row> {
    s.read(|c| {
        let mut st = c.prepare(
            "SELECT revision, n, verb, why, state, targets FROM changeset_action
             WHERE changeset_id = ?1 ORDER BY revision, n",
        )?;
        let rows = st.query_map([changeset], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })
    .unwrap()
}

fn changeset_row(s: &Store, id: i64) -> (String, Option<String>, Option<String>, String) {
    s.read(|c| {
        Ok(c.query_row(
            "SELECT author_kind, author_session, why, state FROM changeset WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?)
    })
    .unwrap()
}

#[test]
fn no_match_stores_nothing() {
    let dir = TempDir::new("none");
    let (s, a) = store(&dir);
    assert_eq!(run(&s, a, NOW).unwrap(), Outcome::Nothing);
    let n: i64 = s
        .read(|c| Ok(c.query_row("SELECT count(*) FROM changeset", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 0);
}

/// Each action has the reason of its rule, the changeset has a reason, and
/// each action stays `proposed`: nothing applies without a rule or an
/// approval.
#[test]
fn proposal_is_stored_with_reasons_and_applies_nothing() {
    let dir = TempDir::new("store");
    let (s, a) = store(&dir);
    item(&s, a, "feed", "g1");
    item(&s, a, "record", "g2");
    item(&s, a, "commitment", "g3");
    let Outcome::Submitted {
        changeset,
        revision,
    } = run(&s, a, NOW).unwrap()
    else {
        panic!("a proposal");
    };
    assert_eq!(revision, 1);
    let (kind, session, why, state) = changeset_row(&s, changeset);
    assert_eq!(kind, "triage");
    assert_eq!(session.as_deref(), Some(format!("account-{a}").as_str()));
    assert_eq!(why.as_deref(), Some(WHY));
    assert_eq!(state, "open");
    let rows = actions(&s, changeset);
    let want = [(Rule::Feed, "m-1"), (Rule::Record, "m-2")];
    assert_eq!(rows.len(), want.len());
    for (row, (rule, id)) in rows.iter().zip(want) {
        assert_eq!(row.2, "archive");
        assert_eq!(row.3.as_deref(), Some(rule.why()));
        assert_eq!(row.4.as_deref(), Some("proposed"));
        assert_eq!(row.5, format!(r#"{{"messages":["{id}"]}}"#));
    }
}

/// The same proposal gives no new revision. A changed proposal gives one,
/// and the submit replaces the open actions of the older revision.
#[test]
fn new_revision_only_on_change() {
    let dir = TempDir::new("revise");
    let (s, a) = store(&dir);
    item(&s, a, "feed", "g1");
    let Outcome::Submitted { changeset, .. } = run(&s, a, NOW).unwrap() else {
        panic!("a proposal");
    };
    assert_eq!(run(&s, a, NOW + 1).unwrap(), Outcome::Same { changeset });
    item(&s, a, "feed", "g4");
    assert_eq!(
        run(&s, a, NOW + 2).unwrap(),
        Outcome::Submitted {
            changeset,
            revision: 2
        }
    );
    let rows = actions(&s, changeset);
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].0, rows[0].4.as_deref()), (1, Some("replaced")));
    assert_eq!((rows[1].0, rows[1].4.as_deref()), (2, Some("proposed")));
    assert_eq!(rows[1].5, r#"{"messages":["m-1","m-4"]}"#);
}

/// A failure in a run writes nothing: no changeset and no draft stay.
#[test]
fn a_failed_run_writes_nothing() {
    let dir = TempDir::new("fail");
    let (s, a) = store(&dir);
    item(&s, a, "record", "g2");
    s.write(|tx| {
        tx.execute_batch(
            "CREATE TRIGGER test_fail BEFORE INSERT ON changeset_action
             BEGIN SELECT RAISE(ABORT, 'test'); END;",
        )?;
        Ok(())
    })
    .unwrap();
    assert!(run(&s, a, NOW).is_err());
    let n: i64 = s
        .read(|c| Ok(c.query_row("SELECT count(*) FROM changeset", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 0);
    s.write(|tx| Ok(tx.execute_batch("DROP TRIGGER test_fail")?))
        .unwrap();
    assert!(matches!(run(&s, a, NOW).unwrap(), Outcome::Submitted { revision: 1, .. }));
}

/// Two runs at the same time, also from two store handles (two
/// processes), make at most one open triage changeset.
#[test]
fn concurrent_runs_make_one_changeset() {
    let dir = TempDir::new("race");
    let (s, a) = store(&dir);
    item(&s, a, "feed", "g1");
    drop(s);
    let threads: Vec<_> = (0..4)
        .map(|i| {
            let path = dir.0.clone();
            std::thread::spawn(move || {
                let s = Store::open(&path).unwrap();
                // A run that loses the race to the write lock gives an error.
                let _ = run(&s, a, NOW + i);
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let s = Store::open(&dir.0).unwrap();
    let n: i64 = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM changeset WHERE author_kind = 'triage'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(n <= 1, "{n} changesets");
    assert!(matches!(
        run(&s, a, NOW + 9).unwrap(),
        Outcome::Same { .. } | Outcome::Submitted { revision: 1, .. }
    ));
}

/// A large rule gives one action for each CHUNK messages, and a second run
/// sees the same proposal. An empty proposal leaves the open actions.
#[test]
fn large_rule_is_split_and_empty_keeps_open_actions() {
    let dir = TempDir::new("large");
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    s.put_labels(a, &[("INBOX", "INBOX", "system")]).unwrap();
    let msgs: Vec<NewMessage> = (0..=CHUNK)
        .map(|i| NewMessage {
            provider_id: format!("g{i}"),
            thread_provider_id: format!("t{i}"),
            labels: vec!["INBOX".into()],
            ..Default::default()
        })
        .collect();
    s.put_messages(a, &msgs).unwrap();
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
               SELECT 'feed', account_id, 'new', '{}', 0, 0 FROM message;
             INSERT INTO item_source(item_id, message_id, role)
               SELECT id, id, 'origin' FROM item;",
        )?;
        Ok(())
    })
    .unwrap();
    let Outcome::Submitted { changeset, .. } = run(&s, a, NOW).unwrap() else {
        panic!("a proposal");
    };
    let rows = actions(&s, changeset);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].5, format!(r#"{{"messages":["m-{}"]}}"#, CHUNK + 1));
    assert_eq!(run(&s, a, NOW + 1).unwrap(), Outcome::Same { changeset });
    // The feed items go away: the proposal is empty.
    s.write(|tx| Ok(tx.execute_batch("UPDATE item SET status = 'gone'")?))
        .unwrap();
    assert_eq!(run(&s, a, NOW + 2).unwrap(), Outcome::Same { changeset });
    assert_eq!(actions(&s, changeset).len(), 2);
}

/// A smaller proposal is a change too.
#[test]
fn fewer_actions_give_a_new_revision() {
    let dir = TempDir::new("fewer");
    let (s, a) = store(&dir);
    item(&s, a, "feed", "g1");
    item(&s, a, "record", "g2");
    let Outcome::Submitted { changeset, .. } = run(&s, a, NOW).unwrap() else {
        panic!("a proposal");
    };
    s.write(|tx| Ok(tx.execute_batch("UPDATE item SET status = 'gone' WHERE form = 'record'")?))
        .unwrap();
    assert_eq!(
        run(&s, a, NOW + 1).unwrap(),
        Outcome::Submitted { changeset, revision: 2 }
    );
}

/// Each account gets its own changeset: one action changes one account.
#[test]
fn one_changeset_for_each_account() {
    let dir = TempDir::new("accounts");
    let (s, a) = store(&dir);
    let b = s.add_account("gmail", "other@example.com").unwrap();
    s.put_labels(b, &[("INBOX", "INBOX", "system")]).unwrap();
    s.put_messages(
        b,
        &[NewMessage {
            provider_id: "h1".into(),
            thread_provider_id: "th1".into(),
            labels: vec!["INBOX".into()],
            ..Default::default()
        }],
    )
    .unwrap();
    item(&s, a, "feed", "g1");
    item(&s, b, "feed", "h1");
    let Outcome::Submitted { changeset: ca, .. } = run(&s, a, NOW).unwrap() else {
        panic!()
    };
    let Outcome::Submitted { changeset: cb, .. } = run(&s, b, NOW).unwrap() else {
        panic!()
    };
    assert_ne!(ca, cb);
    assert_eq!(actions(&s, cb)[0].5, r#"{"messages":["m-5"]}"#);
    assert_eq!(
        run(&s, a, NOW + 1).unwrap(),
        Outcome::Same { changeset: ca }
    );
}
