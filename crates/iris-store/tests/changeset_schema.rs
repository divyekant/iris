//! Migration 5: the changeset tables and their triggers (specification 2,
//! section 5). Each test writes raw SQL, as any writer of the store can.

use std::fs;
use std::path::PathBuf;

use iris_store::{NewMessage, Store};
use rusqlite::Connection;

/// A new empty folder for one test, removed at the end.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-cs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const STATES: [&str; 11] = [
    "proposed",
    "in_review",
    "approved",
    "held",
    "applied",
    "failed",
    "denied",
    "expired",
    "cancelled",
    "replaced",
    "undone",
];

/// Section 5.2, as the test sees it: the next states of each state.
fn allowed(from: Option<&str>) -> &'static [&'static str] {
    match from {
        None => &["proposed", "denied", "cancelled"],
        Some("proposed") => &[
            "approved",
            "in_review",
            "denied",
            "expired",
            "cancelled",
            "replaced",
        ],
        Some("in_review") => &["approved", "denied", "expired", "cancelled", "replaced"],
        // approved to held comes with release 3, failed to undone with the
        // ledger (R2-03).
        Some("approved") => &[
            "applied",
            "failed",
            "in_review",
            "denied",
            "expired",
            "cancelled",
        ],
        Some("applied") => &["undone"],
        Some(_) => &[],
    }
}

/// A store with messages 1 and 2, and policy version 1 with the rule IDs 1
/// to 4.
fn store(dir: &TempDir) -> Store {
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    let m = |id: &str| NewMessage {
        provider_id: id.into(),
        thread_provider_id: "gt1".into(),
        ..Default::default()
    };
    s.put_messages(a, &[m("g1"), m("g2")]).unwrap();
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('a', 'h1', 0);
             INSERT INTO policy_rule_id(id) VALUES (1), (2), (3), (4);",
        )?;
        Ok(())
    })
    .unwrap();
    s
}

/// A changeset with one draft action on message 1. Gives the changeset ID.
fn draft(c: &Connection) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0)",
        [],
    )?;
    let cs = c.last_insert_rowid();
    c.execute(
        "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
         VALUES (?1, 1, 1, 'archive', '{\"messages\":[\"m-1\"]}', '{}')",
        [cs],
    )?;
    c.execute(
        "INSERT INTO action_message(action_id, message_id) VALUES (?1, 1)",
        [c.last_insert_rowid()],
    )?;
    Ok(cs)
}

/// The submit of the changeset, as `iris-changeset` writes it.
fn submit(c: &Connection, cs: i64) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE changeset SET why = 'w', submitted_at = 0 WHERE id = ?1",
        [cs],
    )?;
    c.execute(
        "UPDATE changeset_action SET hash = 'h', submitted_at = 0, state = 'proposed'
         WHERE changeset_id = ?1",
        [cs],
    )?;
    Ok(())
}

fn fails(s: &Store, sql: &str) {
    assert!(
        s.dry_write(|tx| Ok(tx.execute_batch(sql)?)).is_err(),
        "{sql}"
    );
}

fn works(s: &Store, sql: &str) {
    s.dry_write(|tx| Ok(tx.execute_batch(sql)?))
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

/// Each move of each state: the store allows only the moves of section 5.2.
#[test]
fn only_the_moves_of_section_5_2() {
    let dir = TempDir::new("moves");
    let s = store(&dir);
    let cs = s.write(|tx| Ok(draft(tx)?)).unwrap();
    let froms = std::iter::once(None).chain(STATES.iter().copied().map(Some));
    for from in froms {
        for to in STATES {
            let r = s.dry_write(|tx| {
                // Set the from state past the trigger, then put it back.
                let trigger: String = tx.query_row(
                    "SELECT sql FROM sqlite_master WHERE name = 'action_state'",
                    [],
                    |r| r.get(0),
                )?;
                tx.execute_batch("DROP TRIGGER action_state")?;
                tx.execute(
                    "UPDATE changeset_action SET state = ?1,
                       decided_by = iif(coalesce(?1, 'proposed') = 'proposed', NULL, 'rule'),
                       rule = iif(coalesce(?1, 'proposed') = 'proposed', NULL, 1),
                       policy = iif(coalesce(?1, 'proposed') = 'proposed', NULL, 1),
                       reason = iif(coalesce(?1, 'proposed') = 'proposed', NULL, 'r'),
                       decided_at = iif(coalesce(?1, 'proposed') = 'proposed', NULL, 1209600000),
                       hash = iif(?1 IS NULL, NULL, 'h'),
                       submitted_at = iif(?1 IS NULL, NULL, 0)",
                    [from],
                )?;
                tx.execute_batch(&trigger)?;
                // The submit. For replaced, a newer revision is submitted.
                tx.execute("UPDATE changeset SET submitted_at = 0, why = 'w'", [])?;
                if to == "replaced" {
                    tx.execute_batch(
                        "UPDATE changeset SET revision = 2, submitted_at = NULL;
                         UPDATE changeset SET submitted_at = 0;",
                    )?;
                }
                // A withdraw cancels the action in the same write, or leaves it.
                if to == "cancelled" {
                    tx.execute("UPDATE changeset SET state = 'withdrawn'", [])?;
                    let now: Option<String> =
                        tx.query_row("SELECT state FROM changeset_action", [], |r| r.get(0))?;
                    return Ok(from != Some("cancelled") && now.as_deref() == Some("cancelled"));
                }
                // 14 days after the submit: the event of expired. From a
                // draft, submit sets the hash and the time with the state.
                // Submit proposes with no decision.
                let decision = if to == "proposed" {
                    "decided_by = NULL, rule = NULL, policy = NULL, reason = NULL, decided_at = NULL"
                } else {
                    "decided_by = 'rule', rule = 1, policy = 1, reason = 'r', decided_at = 1209600000"
                };
                let hash = if from.is_none() {
                    ", hash = 'h', submitted_at = 0"
                } else {
                    ""
                };
                let sql = format!(
                    "UPDATE changeset_action SET state = ?1, {decision}{hash} WHERE changeset_id = ?2"
                );
                Ok(tx.execute(&sql, rusqlite::params![to, cs]).is_ok())
            });
            let expect = allowed(from).contains(&to);
            assert_eq!(r.unwrap(), expect, "{from:?} -> {to:?}");
        }
    }
}

/// Without its event, each of these moves is refused.
#[test]
fn a_move_needs_its_event() {
    let dir = TempDir::new("events");
    let s = store(&dir);
    // A new action has no state.
    fails(
        &s,
        "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, state)
         VALUES (1, 1, 9, 'archive', '{}', '{}', 'approved')",
    );
    s.write(|tx| Ok(draft(tx)?)).unwrap();
    // Only submit moves a draft action: no state without the hash and the
    // time, and not while the changeset is a draft.
    fails(&s, "UPDATE changeset_action SET state = 'proposed'");
    fails(
        &s,
        "UPDATE changeset_action SET state = 'proposed', hash = 'h'",
    );
    fails(
        &s,
        "UPDATE changeset_action SET state = 'proposed', hash = 'h', submitted_at = 0",
    );
    // An action stays in its changeset and revision.
    fails(&s, "UPDATE changeset_action SET revision = 2");
    fails(&s, "UPDATE changeset_action SET n = 2");
    // A draft is cancelled only with a withdraw.
    fails(
        &s,
        "UPDATE changeset_action SET state = 'cancelled', decided_by = 'author'",
    );
    // A submitted changeset has a reason.
    fails(&s, "UPDATE changeset SET submitted_at = 0, why = NULL");
    fails(&s, "UPDATE changeset SET submitted_at = 0, why = ' '");
    fails(
        &s,
        "INSERT INTO changeset(author_kind, created_at, submitted_at) VALUES ('agent', 0, 0)",
    );
    s.write(|tx| Ok(submit(tx, 1)?)).unwrap();
    for sql in [
        // Expired only 14 days after the submit.
        "UPDATE changeset_action SET state = 'expired', decided_by = 'expiry',
           decided_at = submitted_at + 1209599999",
        "UPDATE changeset_action SET state = 'expired', decided_by = 'expiry'",
        // Replaced only by the submit of a newer revision.
        "UPDATE changeset_action SET state = 'replaced', decided_by = 'author'",
        // Cancelled only with a withdraw.
        "UPDATE changeset_action SET state = 'cancelled', decided_by = 'author'",
        "UPDATE changeset_action SET state = 'held', decided_by = 'engine'",
        // Each decision names who decided, and a denial says why.
        "UPDATE changeset_action SET state = 'approved'",
        "UPDATE changeset_action SET state = 'approved', decided_by = 'engine'",
        "UPDATE changeset_action SET state = 'denied', decided_by = 'person'",
        "UPDATE changeset_action SET state = 'denied', decided_by = 'person', reason = ' '",
    ] {
        fails(&s, sql);
    }
    works(
        &s,
        "UPDATE changeset_action SET state = 'expired', decided_by = 'expiry',
           decided_at = submitted_at + 1209600000",
    );
    works(
        &s,
        "UPDATE changeset_action SET state = 'denied', decided_by = 'person', reason = 'No.'",
    );
    // No approval while a new revision is a draft.
    works(
        &s,
        "UPDATE changeset_action SET state = 'approved', decided_by = 'person'",
    );
    s.write(|tx| Ok(tx.execute("UPDATE changeset SET revision = 2, submitted_at = NULL", [])?))
        .unwrap();
    fails(
        &s,
        "UPDATE changeset_action SET state = 'approved', decided_by = 'person'",
    );
}

/// A submitted revision stays submitted, and a new action goes only into the
/// draft revision.
#[test]
fn a_submitted_revision_stays_submitted() {
    let dir = TempDir::new("revisions");
    let s = store(&dir);
    s.write(|tx| Ok(submit(tx, draft(tx)?)?)).unwrap();
    let insert = |revision: i64| {
        format!(
            "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
             VALUES (1, {revision}, 2, 'archive', '{{}}', '{{}}')"
        )
    };
    for sql in [
        &insert(1),
        &insert(2),
        "UPDATE changeset SET submitted_at = NULL",
        "UPDATE changeset SET submitted_at = 1",
        "UPDATE changeset SET why = 'x'",
        "UPDATE changeset SET revision = 3, submitted_at = NULL",
        "UPDATE changeset SET revision = 2",
        "UPDATE changeset SET author_name = 'x'",
        "UPDATE changeset SET created_at = 1",
        "INSERT INTO changeset(author_kind, created_at, revision) VALUES ('agent', 0, 2)",
        "INSERT INTO changeset(author_kind, created_at, state) VALUES ('agent', 0, 'open')",
        "INSERT OR REPLACE INTO changeset(id, author_kind, created_at) VALUES (1, 'agent', 0)",
    ] {
        fails(&s, sql);
    }
    // A new revision.
    s.write(|tx| Ok(tx.execute("UPDATE changeset SET revision = 2, submitted_at = NULL", [])?))
        .unwrap();
    works(&s, &insert(2));
    for sql in [
        &insert(1),
        // An insert does not replace a submitted action.
        "INSERT OR REPLACE INTO changeset_action(id, changeset_id, revision, n, verb, targets, args)
         VALUES (1, 1, 2, 1, 'archive', '{}', '{}')",
        "INSERT OR REPLACE INTO changeset_action(changeset_id, revision, n, verb, targets, args)
         VALUES (1, 1, 1, 'archive', '{}', '{}')",
        // Revision 1 does not become a draft again.
        "UPDATE changeset SET revision = 1",
        "UPDATE changeset SET revision = 1, submitted_at = 0",
    ] {
        fails(&s, sql);
    }
    let resolved: i64 = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM action_message WHERE action_id = 1",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(resolved, 1);
    // A withdrawn changeset does not change.
    s.write(|tx| Ok(tx.execute("UPDATE changeset SET state = 'withdrawn'", [])?))
        .unwrap();
    fails(&s, "UPDATE changeset SET submitted_at = 0, why = 'w'");
    fails(&s, &insert(2));
}

/// A withdraw cancels each open action in the same write, and a withdrawn
/// changeset applies nothing more.
#[test]
fn a_withdraw_cancels_the_open_actions() {
    let dir = TempDir::new("withdraw");
    let s = store(&dir);
    s.write(|tx| {
        submit(tx, draft(tx)?)?;
        tx.execute(
            "UPDATE changeset_action SET state = 'approved', decided_by = 'person'",
            [],
        )?;
        Ok(tx.execute("UPDATE changeset SET state = 'withdrawn'", [])?)
    })
    .unwrap();
    let row: (String, String) = s
        .read(|c| {
            Ok(
                c.query_row("SELECT state, decided_by FROM changeset_action", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?,
            )
        })
        .unwrap();
    assert_eq!(row, ("cancelled".into(), "author".into()));
    for sql in [
        "UPDATE changeset_action SET state = 'applied'",
        "UPDATE changeset SET state = 'open'",
        "UPDATE changeset SET state = 'closed'",
    ] {
        fails(&s, sql);
    }
}

/// A draft action has no decision, and submit proposes with no decision.
#[test]
fn a_draft_has_no_decision() {
    let dir = TempDir::new("nodecision");
    let s = store(&dir);
    for column in [
        "decided_by = 'person'",
        "reason = 'r'",
        "decided_at = 1",
        "decided_by = 'rule', rule = 1, policy = 1",
    ] {
        let (name, value) = column.split_once(" = ").unwrap();
        let (name, value) = if name == "decided_by" && value.contains("rule") {
            ("decided_by, rule, policy", "'rule', 1, 1")
        } else {
            (name, value)
        };
        fails(
            &s,
            &format!(
                "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0);
                 INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, {name})
                 VALUES (last_insert_rowid(), 1, 1, 'archive', '{{}}', '{{}}', {value});"
            ),
        );
        // A decision written into the draft, or into a proposed action.
        s.dry_write(|tx| Ok(draft(tx)?)).unwrap();
        let r = s.dry_write(|tx| {
            let cs = draft(tx)?;
            tx.execute(&format!("UPDATE changeset_action SET {column}"), [])?;
            Ok(())
        });
        assert!(r.is_err(), "{column}");
        let r = s.dry_write(|tx| {
            submit(tx, draft(tx)?)?;
            tx.execute(&format!("UPDATE changeset_action SET {column}"), [])?;
            Ok(())
        });
        assert!(r.is_err(), "{column}");
    }
    works(
        &s,
        "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0);
         INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
         VALUES (last_insert_rowid(), 1, 1, 'archive', '{}', '{}');",
    );
    // A person does not send an action to review. Each decided state names
    // who decided.
    s.write(|tx| Ok(submit(tx, draft(tx)?)?)).unwrap();
    fails(
        &s,
        "UPDATE changeset_action SET state = 'denied', reason = 'No.'",
    );
    fails(
        &s,
        "UPDATE changeset_action SET state = 'expired', decided_at = submitted_at + 1209600000",
    );
    fails(
        &s,
        "UPDATE changeset_action SET state = 'in_review', decided_by = 'person'",
    );
    works(
        &s,
        "UPDATE changeset_action SET state = 'in_review', decided_by = 'policy'",
    );
}

/// The state of a changeset comes from its actions.
#[test]
fn the_changeset_state_comes_from_its_actions() {
    let dir = TempDir::new("csstate");
    let s = store(&dir);
    s.write(|tx| Ok(draft(tx)?)).unwrap();
    fails(&s, "UPDATE changeset SET state = 'open'");
    fails(&s, "UPDATE changeset SET state = 'closed'");
    s.write(|tx| Ok(submit(tx, 1)?)).unwrap();
    for state in ["draft", "closed"] {
        fails(&s, &format!("UPDATE changeset SET state = '{state}'"));
    }
    works(&s, "UPDATE changeset SET state = 'open'");
    works(&s, "UPDATE changeset SET state = 'withdrawn'");
}

/// A rule decision names a rule ID of the active policy.
#[test]
fn a_rule_decision_names_a_live_rule_of_the_active_policy() {
    let dir = TempDir::new("rules");
    let s = store(&dir);
    s.write(|tx| Ok(submit(tx, draft(tx)?)?)).unwrap();
    let rule = |rule: i64, policy: i64| {
        format!(
            "UPDATE changeset_action SET state = 'approved', decided_by = 'rule',
               rule = {rule}, policy = {policy}"
        )
    };
    works(&s, &rule(3, 1));
    // A rule decision names its rule and the version; only a rule decision
    // names a rule.
    fails(
        &s,
        "UPDATE changeset_action SET state = 'approved', decided_by = 'rule', rule = 3",
    );
    fails(
        &s,
        "UPDATE changeset_action SET state = 'approved', decided_by = 'person', rule = 3, policy = 1",
    );
    s.write(|tx| {
        Ok(tx.execute_batch(
            "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('b', 'h2', 0);
             UPDATE policy_rule_id SET removed_at = 0 WHERE id = 4;",
        )?)
    })
    .unwrap();
    works(&s, &rule(3, 2));
    // An older version, a version or an ID that the store does not have, a
    // removed ID.
    for (id, policy) in [(3, 1), (3, 9), (7, 2), (4, 2)] {
        fails(&s, &rule(id, policy));
    }
    // With no confirmed policy, no rule decides.
    s.write(|tx| Ok(tx.execute_batch("DELETE FROM policy_version; DELETE FROM policy_rule_id;")?))
        .unwrap();
    fails(&s, &rule(1, 1));
}

/// A submitted action does not change, whoever writes: the approval hash
/// covers its targets, arguments and messages.
#[test]
fn a_submitted_action_does_not_change() {
    let dir = TempDir::new("fixed");
    let s = store(&dir);
    s.write(|tx| Ok(draft(tx)?)).unwrap();
    // A draft action changes.
    works(&s, "UPDATE changeset_action SET args = '{\"x\":1}'");
    works(&s, "INSERT INTO action_message VALUES (1, 2)");
    works(
        &s,
        "DELETE FROM action_message; DELETE FROM changeset_action",
    );
    s.write(|tx| Ok(submit(tx, 1)?)).unwrap();
    for sql in [
        "UPDATE changeset_action SET targets = '{\"messages\":[\"m-2\"]}'",
        "UPDATE changeset_action SET args = '{\"x\":1}'",
        "UPDATE changeset_action SET verb = 'trash'",
        "UPDATE changeset_action SET hash = 'x'",
        "UPDATE changeset_action SET submitted_at = 1",
        "UPDATE changeset_action SET why = 'x'",
        "UPDATE changeset_action SET n = 2",
        "UPDATE changeset_action SET reason = 'x'",
        "UPDATE changeset_action SET decided_by = 'person'",
        "UPDATE action_message SET message_id = 2",
        "INSERT INTO action_message VALUES (1, 2)",
        "DELETE FROM action_message",
        "DELETE FROM changeset_action",
        // Text that is not JSON.
        "UPDATE changeset_action SET state = 'denied', decided_by = 'engine', reason = 'r',
           flags = 'x'",
    ] {
        fails(&s, sql);
    }
    // The engine can add a flag before apply (section 5.4). A flag stays.
    s.write(|tx| Ok(tx.execute("UPDATE changeset_action SET flags = '[\"bulk\"]'", [])?))
        .unwrap();
    fails(&s, "UPDATE changeset_action SET flags = '[]'");
    for flags in [
        "[null]",
        "[\"bulk\",null]",
        "{\"bulk\":1}",
        "\"bulk\"",
        "[1]",
        "[\"bulk\",\"other\"]",
    ] {
        fails(
            &s,
            &format!("UPDATE changeset_action SET flags = '{flags}'"),
        );
    }
    fails(
        &s,
        "UPDATE changeset_action SET flags = '[\"first_contact\"]'",
    );
    works(
        &s,
        "UPDATE changeset_action SET flags = '[\"bulk\",\"first_contact\"]'",
    );
}

/// Applied and failed keep the decision that approved the action.
#[test]
fn the_apply_keeps_who_approved() {
    let dir = TempDir::new("approver");
    let s = store(&dir);
    s.write(|tx| {
        submit(tx, draft(tx)?)?;
        Ok(tx.execute(
            "UPDATE changeset_action SET state = 'approved', decided_by = 'rule', rule = 3,
               policy = 1, decided_at = 5",
            [],
        )?)
    })
    .unwrap();
    for to in ["applied", "failed"] {
        for change in [
            "flags = '[\"bulk\"]'",
            "decided_by = 'engine', rule = NULL, policy = NULL",
            "decided_by = 'person', rule = NULL, policy = NULL",
            "rule = 2",
            "reason = 'x'",
            "decided_at = 6",
        ] {
            fails(
                &s,
                &format!("UPDATE changeset_action SET state = '{to}', {change}"),
            );
        }
        works(&s, &format!("UPDATE changeset_action SET state = '{to}'"));
    }
    // After the apply, no flag is added.
    fails(
        &s,
        "UPDATE changeset_action SET state = 'applied';
         UPDATE changeset_action SET flags = '[\"bulk\"]';",
    );
    // A newer policy does not change the approval of rule 3 in version 1.
    s.write(|tx| {
        Ok(tx.execute_batch(
            "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('b', 'h2', 0);
             UPDATE policy_rule_id SET removed_at = 0 WHERE id = 3;
             UPDATE changeset_action SET state = 'applied';",
        )?)
    })
    .unwrap();
    let by: (String, i64, i64) = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT decided_by, rule, policy FROM changeset_action",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(by, ("rule".into(), 3, 1));
}

/// The changeset is open while an action is open, then closed.
#[test]
fn the_changeset_closes_with_its_last_open_action() {
    let dir = TempDir::new("closes");
    let s = store(&dir);
    let state = |s: &Store| -> String {
        s.read(|c| Ok(c.query_row("SELECT state FROM changeset", [], |r| r.get(0))?))
            .unwrap()
    };
    s.write(|tx| Ok(draft(tx)?)).unwrap();
    assert_eq!(state(&s), "draft");
    s.write(|tx| Ok(submit(tx, 1)?)).unwrap();
    assert_eq!(state(&s), "open");
    s.write(|tx| {
        Ok(tx.execute(
            "UPDATE changeset_action SET state = 'approved', decided_by = 'person'",
            [],
        )?)
    })
    .unwrap();
    assert_eq!(state(&s), "open");
    s.write(|tx| Ok(tx.execute("UPDATE changeset_action SET state = 'applied'", [])?))
        .unwrap();
    assert_eq!(state(&s), "closed");
}
