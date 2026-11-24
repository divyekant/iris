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

/// A draft with a warning flag goes to a person in the mode `apply`, and
/// the decision does not panic.
#[test]
fn apply_mode_sends_a_flagged_draft_to_a_person() {
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
    // The draft goes to a new address: the flag new_recipient is a hard
    // limit (spec 2, section 6.5).
    assert_eq!(
        (d[0].0.as_str(), d[0].1.as_str(), d[0].2),
        ("in_review", "limit", None)
    );
    assert!(d[0].3.contains("warning flag"), "{}", d[0].3);
}

/// `skip`, `mute` and `dismiss` (R5-02) are verbs of the policy: a rule can
/// name them, and the hard limit of hostile mail (spec 2, section 6.5)
/// denies an agent action on an item from hostile mail.
#[test]
fn apply_mode_decides_skip_with_the_rules_and_the_hard_limits() {
    use iris_changeset::{add_action, create, submit, Author, AuthorKind, Verb};
    let dir = TempDir::new("applyskip");
    let (s, a) = store(&dir);
    item(&s, a, "feed", "g1");
    item(&s, a, "feed", "g2");
    s.write(|tx| {
        tx.execute_batch(
            "UPDATE item SET doc = json_object('id', 'f-' || id);
             UPDATE message SET hostile = '[\"phishing\"]' WHERE provider_id = 'g2';",
        )?;
        Ok(())
    })
    .unwrap();
    apply_policy(
        &s,
        &dir,
        "[[rule]]\nid = 1\nname = \"Skip the feed\"\neffect = \"allow\"\n\
         verbs = [\"skip\"]\nwhen = { form = \"feed\" }\nby = \"person\"\nadded = 2026-11-10\n",
    );
    let who = Author {
        kind: AuthorKind::Agent,
        name: Some("t".into()),
        session: None,
        job: None,
    };
    let cs = create(&s, &who, NOW).unwrap();
    let mut args = serde_json::Map::new();
    args.insert("reason".into(), "Old.".into());
    for id in ["f-1", "f-2"] {
        add_action(&s, cs, &who, Verb::Skip, &[id], &args, None).unwrap();
    }
    submit(&s, cs, &who, "Skip.", NOW).unwrap();
    s.write(|c| {
        iris_triage::decide_submitted(c, cs, 1, iris_policy::Author::Local, NOW)
            .map_err(|e| iris_store::Error::Io(std::io::Error::other(e.to_string())))
    })
    .unwrap();
    let d = decisions(&s, cs);
    assert_eq!((d[0].0.as_str(), d[0].2), ("approved", Some(1)), "{d:?}");
    assert_eq!(d[1].0, "denied", "{d:?}");
}

/// Decision D10 (issue #193): in the mode `observe`, `would` gives for each
/// proposed action exactly the rule decision that the mode `apply` gives to
/// the same proposal, and the actions stay `proposed`. In the mode `apply`,
/// `would` is empty.
#[test]
fn observe_mode_shows_what_the_rules_would_decide() {
    const RULES: &str = "[[rule]]\nid = 1\nname = \"Archive the feed\"\neffect = \"allow\"\n\
         verbs = [\"archive\"]\nwhen = { form = \"feed\" }\nby = \"person\"\nadded = 2026-11-10\n\
         [[rule]]\nid = 2\nname = \"Keep records\"\neffect = \"deny\"\n\
         verbs = [\"archive\"]\nwhen = { form = \"record\" }\nby = \"person\"\nadded = 2026-11-10\n";
    let mut got = Vec::new();
    for mode in ["observe", "apply"] {
        let dir = TempDir::new(&format!("would-{mode}"));
        let (s, a) = store(&dir);
        item(&s, a, "feed", "g1");
        item(&s, a, "record", "g2");
        item(&s, a, "conversation", "g3");
        let text = format!("version = 0\nmode = \"{mode}\"\n{RULES}");
        fs::write(dir.0.join(iris_policy::FILE), &text).unwrap();
        iris_policy::confirm(&s, &iris_policy::sha256_hex(&text), 0).unwrap();
        let Outcome::Submitted { changeset, .. } = run(&s, a, NOW).unwrap() else {
            panic!("a proposal");
        };
        let would = s
            .read(|c| {
                let mut out = Vec::new();
                if let Some(pass) = iris_triage::would_start(c, changeset).unwrap() {
                    let mut after = 0;
                    while let Some((id, w)) =
                        iris_triage::would_next(c, &pass, changeset, after, NOW).unwrap()
                    {
                        after = id;
                        out.extend(w);
                    }
                }
                Ok(out)
            })
            .unwrap();
        if mode == "observe" {
            let rows = actions(&s, changeset);
            assert!(
                rows.iter().all(|r| r.4.as_deref() == Some("proposed")),
                "{rows:?}"
            );
            got = would
                .iter()
                .map(|w| {
                    let state = match w.outcome {
                        iris_policy::Outcome::Allow => "approved",
                        iris_policy::Outcome::Review => "in_review",
                        iris_policy::Outcome::Deny => "denied",
                    };
                    (w.n, state.to_string(), i64::from(w.rule), w.reason.clone())
                })
                .collect();
            assert_eq!(got.len(), 2, "{would:?}");
            assert!(would.iter().all(|w| w.policy == 1 && w.revision == 1));
        } else {
            assert!(would.is_empty(), "{would:?}");
            let want: Vec<_> = decisions(&s, changeset)
                .iter()
                .enumerate()
                .filter(|(_, r)| r.1 == "rule")
                .map(|(i, r)| (i as i64 + 1, r.0.clone(), r.2.unwrap(), r.3.clone()))
                .collect();
            assert_eq!(got, want);
        }
    }
}

/// Decisions D2 and D3: a rule allows a reply draft whose recipients are in
/// its thread. A draft with a warning flag still goes to a person.
#[test]
fn apply_mode_lets_a_rule_allow_a_draft_to_the_thread() {
    use iris_changeset::{add_action, create, submit, Author, AuthorKind, Verb};
    let dir = TempDir::new("applydraft");
    let (s, _) = store(&dir);
    apply_policy(
        &s,
        &dir,
        "[[rule]]\nid = 1\nname = \"Replies\"\neffect = \"allow\"\nverbs = [\"draft\"]\n\
         when = { recipients = \"thread\", author = \"local\" }\nby = \"person\"\nadded = 2026-11-10\n",
    );
    let who = Author {
        kind: AuthorKind::Local,
        name: Some("t".into()),
        session: None,
        job: None,
    };
    let thread: i64 = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT thread_id FROM message WHERE provider_id = 'g1'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    let cs = create(&s, &who, NOW).unwrap();
    // To your own address: no new recipient, and in the thread.
    for to in ["owner@example.com", "p@x.org"] {
        let args = serde_json::json!({"to": [to], "body": "Thanks."});
        add_action(
            &s,
            cs,
            &who,
            Verb::Draft,
            &[&format!("t-{thread}")],
            args.as_object().unwrap(),
            None,
        )
        .unwrap();
    }
    submit(&s, cs, &who, "Reply.", NOW).unwrap();
    s.write(|c| {
        iris_triage::decide_submitted(c, cs, 1, iris_policy::Author::Local, NOW)
            .map_err(|e| iris_store::Error::Io(std::io::Error::other(e.to_string())))
    })
    .unwrap();
    let d = decisions(&s, cs);
    assert_eq!(
        (d[0].0.as_str(), d[0].1.as_str(), d[0].2),
        ("approved", "rule", Some(1)),
        "{d:?}"
    );
    assert_eq!(
        (d[1].0.as_str(), d[1].1.as_str(), d[1].2),
        ("in_review", "limit", None),
        "{d:?}"
    );
}

/// Spec 2 section 6.5: a send with a warning flag goes to review by
/// `limit`, as a draft does. A send with no warning flag goes to review by
/// `policy`: no rule can allow a send (decision D2).
#[test]
fn apply_mode_sends_a_send_to_a_person_by_its_flags() {
    use iris_changeset::{add_action, create, submit, Author, AuthorKind, Verb};
    let dir = TempDir::new("applysend");
    let (s, _) = store(&dir);
    apply_policy(&s, &dir, "");
    // d-1 is a new mail to a new address (new_recipient); d-2 is to you.
    s.write(|tx| {
        tx.execute(
            "INSERT INTO changeset(author_kind, created_at) VALUES ('person', 0)",
            [],
        )?;
        for (n, to) in [(1, "p@x.org"), (2, "owner@example.com")] {
            tx.execute(
                "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                 VALUES (1, 1, ?1, 'draft', '{\"accounts\":[\"a-1\"]}', '{}')",
                [n],
            )?;
            let action = tx.last_insert_rowid();
            let c = iris_compose::Content {
                to: vec![to.into()],
                cc: vec![],
                subject: "Hi".into(),
                body: "Thanks.".into(),
            }
            .check()
            .unwrap();
            let id = format!("iris.{n}@example.com");
            let raw = iris_compose::compose("owner@example.com", &id, &c, None, 1);
            tx.execute(
                "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                                   action_id, created_at, updated_at)
                 VALUES (1, ?1, ?2, '{}', ?3, ?4, ?5, 0, 0)",
                rusqlite::params![
                    format!("r{n}"),
                    id,
                    iris_compose::hash(&c, None),
                    raw,
                    action
                ],
            )?;
        }
        Ok(())
    })
    .unwrap();
    let who = Author {
        kind: AuthorKind::Local,
        name: Some("t".into()),
        session: None,
        job: None,
    };
    let cs = create(&s, &who, NOW).unwrap();
    for d in ["d-1", "d-2"] {
        add_action(&s, cs, &who, Verb::Send, &[d], &Default::default(), None).unwrap();
    }
    submit(&s, cs, &who, "Send.", NOW).unwrap();
    s.write(|c| {
        iris_triage::decide_submitted(c, cs, 1, iris_policy::Author::Local, NOW)
            .map_err(|e| iris_store::Error::Io(std::io::Error::other(e.to_string())))
    })
    .unwrap();
    let d = decisions(&s, cs);
    assert_eq!(
        (d[0].0.as_str(), d[0].1.as_str()),
        ("in_review", "limit"),
        "{d:?}"
    );
    assert_eq!(d[0].3, iris_policy::WARNING_REASON);
    assert_eq!(
        (d[1].0.as_str(), d[1].1.as_str()),
        ("in_review", "policy"),
        "{d:?}"
    );
    assert!(d[1].3.contains("No rule can allow a send"), "{d:?}");
}
