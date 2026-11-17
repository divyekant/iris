//! Triage stores the proposal of observe mode as a changeset. No network.

use std::fs;
use std::path::PathBuf;

use iris_changeset::{approve, decide, ActionId, By, State};
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
    assert!(matches!(
        run(&s, a, NOW).unwrap(),
        Outcome::Submitted { revision: 1, .. }
    ));
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
fn large_rule_is_split_and_empty_withdraws() {
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
    // In the mode apply, the policy decides each action: the flag bulk
    // sends each one to review.
    apply_policy(&s, &dir, "");
    let Outcome::Submitted { changeset, .. } = run(&s, a, NOW).unwrap() else {
        panic!("a proposal");
    };
    let d = decisions(&s, changeset);
    assert!(
        d.iter().all(|r| r.0 == "in_review" && r.1 == "limit"),
        "{d:?}"
    );
    let rows = actions(&s, changeset);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].5, format!(r#"{{"messages":["m-{}"]}}"#, CHUNK + 1));
    assert_eq!(run(&s, a, NOW + 1).unwrap(), Outcome::Same { changeset });
    // The feed items go away: the proposal is empty.
    s.write(|tx| Ok(tx.execute_batch("UPDATE item SET status = 'gone'")?))
        .unwrap();
    // The open actions do not wait for a person: the run withdraws them.
    assert_eq!(
        run(&s, a, NOW + 2).unwrap(),
        Outcome::Withdrawn { changeset }
    );
    let rows = actions(&s, changeset);
    assert!(
        rows.iter().all(|r| r.4.as_deref() == Some("cancelled")),
        "{rows:?}"
    );
    assert_eq!(changeset_row(&s, changeset).3, "withdrawn");
    assert_eq!(run(&s, a, NOW + 3).unwrap(), Outcome::Nothing);
}

/// An empty proposal does not withdraw a changeset with an approved
/// action: the withdraw would cancel the approval, and the proposal leaves
/// out the messages of the approved action.
#[test]
fn empty_keeps_an_approved_action() {
    let dir = TempDir::new("approved");
    let (s, a) = store(&dir);
    item(&s, a, "feed", "g1");
    item(&s, a, "record", "g2");
    let Outcome::Submitted { changeset, .. } = run(&s, a, NOW).unwrap() else {
        panic!("a proposal");
    };
    let first = ActionId {
        changeset,
        revision: 1,
        n: 1,
    };
    let hex = s
        .read(|c| {
            let row: i64 = c.query_row(
                "SELECT id FROM changeset_action WHERE changeset_id = ?1 AND n = 1",
                [changeset],
                |r| r.get(0),
            )?;
            Ok(iris_changeset::hash(c, row).unwrap().1)
        })
        .unwrap();
    approve(&s, first, &hex, NOW).unwrap();
    s.write(|tx| Ok(tx.execute_batch("UPDATE item SET status = 'gone' WHERE form = 'record'")?))
        .unwrap();
    assert_eq!(run(&s, a, NOW + 1).unwrap(), Outcome::Same { changeset });
    let states: Vec<Option<String>> = actions(&s, changeset).into_iter().map(|r| r.4).collect();
    assert_eq!(
        states,
        [Some("approved".to_string()), Some("proposed".to_string())]
    );
    assert_eq!(changeset_row(&s, changeset).3, "open");
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
        Outcome::Submitted {
            changeset,
            revision: 2
        }
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

/// A person denies the proposal: the next run with the same inbox does not
/// propose the same messages again, also when the changeset is closed.
#[test]
fn a_denial_holds() {
    let dir = TempDir::new("deny");
    let (s, a) = store(&dir);
    item(&s, a, "feed", "g1");
    let Outcome::Submitted {
        changeset,
        revision,
    } = run(&s, a, NOW).unwrap()
    else {
        panic!("a proposal");
    };
    let id = ActionId {
        changeset,
        revision,
        n: 1,
    };
    decide(&s, id, State::Denied, By::Person, Some("keep it"), NOW + 1).unwrap();
    assert_eq!(run(&s, a, NOW + 2).unwrap(), Outcome::Nothing);
    // A new feed message is proposed alone.
    item(&s, a, "feed", "g4");
    let Outcome::Submitted {
        changeset: next, ..
    } = run(&s, a, NOW + 3).unwrap()
    else {
        panic!("a proposal");
    };
    assert_eq!(actions(&s, next)[0].5, r#"{"messages":["m-4"]}"#);
}

/// Make `rules` (TOML `[[rule]]` tables) the active policy, in the mode
/// `apply`.
fn apply_policy(s: &Store, dir: &TempDir, rules: &str) {
    let text = format!("version = 0\nmode = \"apply\"\n{rules}");
    fs::write(dir.0.join(iris_policy::FILE), &text).unwrap();
    let done = iris_policy::confirm(s, &iris_policy::sha256_hex(&text), 0).unwrap();
    assert!(
        matches!(done, iris_policy::Confirm::Done { .. }),
        "{done:?}"
    );
}

/// (state, decided_by, rule, reason) of each action of the newest revision.
fn decisions(s: &Store, changeset: i64) -> Vec<(String, String, Option<i64>, String)> {
    s.read(|c| {
        let mut st = c.prepare(
            "SELECT a.state, a.decided_by, a.rule, a.reason FROM changeset_action a
             JOIN changeset s ON s.id = a.changeset_id AND s.revision = a.revision
             WHERE s.id = ?1 ORDER BY a.n",
        )?;
        let rows = st.query_map([changeset], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })
    .unwrap()
}

/// In the mode `apply`, an allow rule approves the triage action, with the
/// rule and its reason. With no rule, the action goes to review.
#[test]
fn apply_mode_decides_each_action_with_a_reason() {
    let dir = TempDir::new("apply");
    let (s, a) = store(&dir);
    item(&s, a, "feed", "g1");
    item(&s, a, "record", "g2");
    apply_policy(
        &s,
        &dir,
        "[[rule]]\nid = 1\nname = \"Archive the feed\"\neffect = \"allow\"\n\
         verbs = [\"archive\"]\nwhen = { form = \"feed\" }\nby = \"person\"\nadded = 2026-11-10\n",
    );
    let Outcome::Submitted { changeset, .. } = run(&s, a, NOW).unwrap() else {
        panic!("a proposal");
    };
    let d = decisions(&s, changeset);
    assert_eq!(d.len(), 2);
    assert_eq!(
        (d[0].0.as_str(), d[0].1.as_str(), d[0].2),
        ("approved", "rule", Some(1))
    );
    assert!(d[0].3.contains("Rule 1"), "{}", d[0].3);
    assert_eq!(
        (d[1].0.as_str(), d[1].1.as_str(), d[1].2),
        ("in_review", "policy", None)
    );
    assert!(!d[1].3.is_empty());
    // The approved and the reviewed messages do not come back as new actions.
    assert_eq!(run(&s, a, NOW + 1).unwrap(), Outcome::Same { changeset });
}

/// A deny rule denies the action, and the denial holds.
#[test]
fn apply_mode_deny_rule() {
    let dir = TempDir::new("applydeny");
    let (s, a) = store(&dir);
    item(&s, a, "feed", "g1");
    apply_policy(
        &s,
        &dir,
        "[[rule]]\nid = 1\nname = \"Keep the feed\"\neffect = \"deny\"\n\
         verbs = [\"archive\"]\nwhen = { form = \"feed\" }\nby = \"person\"\nadded = 2026-11-10\n",
    );
    let Outcome::Submitted { changeset, .. } = run(&s, a, NOW).unwrap() else {
        panic!("a proposal");
    };
    assert_eq!(decisions(&s, changeset)[0].0, "denied");
    assert_eq!(run(&s, a, NOW + 1).unwrap(), Outcome::Nothing);
}

/// A draft and a send are verbs that no rule decides: a submit of each
/// goes to a person in the mode `apply`, and the decision does not panic.
#[test]
fn apply_mode_sends_a_draft_and_a_send_to_a_person() {
    use iris_changeset::{add_action, create, submit, Author, AuthorKind, Verb};
    let dir = TempDir::new("applywrite");
    let (s, a) = store(&dir);
    apply_policy(&s, &dir, "");
    let who = Author {
        kind: AuthorKind::Agent,
        name: Some("t".into()),
        session: None,
        job: None,
    };
    let cs = create(&s, &who, NOW).unwrap();
    let args = serde_json::json!({"to": ["p@x.org"], "subject": "Hi", "body": "Hello"});
    let account = format!("a-{a}");
    add_action(
        &s,
        cs,
        &who,
        Verb::Draft,
        &[&account],
        args.as_object().unwrap(),
        None,
    )
    .unwrap();
    submit(&s, cs, &who, "Write.", NOW).unwrap();
    s.write(|c| {
        iris_triage::decide_submitted(c, cs, 1, iris_policy::Author::Local, NOW)
            .map_err(|e| iris_store::Error::Io(std::io::Error::other(e.to_string())))
    })
    .unwrap();
    let d = decisions(&s, cs);
    assert_eq!(
        (d[0].0.as_str(), d[0].1.as_str(), d[0].2),
        ("in_review", "policy", None)
    );
    assert!(d[0].3.contains("a person decides"), "{}", d[0].3);
}
