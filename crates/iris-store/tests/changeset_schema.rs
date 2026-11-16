//! Migration 7: the changeset tables and their triggers (specification 2,
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
        // The submit proposes a draft action (a_submit_proposes_each_action);
        // no other write gives it a state, except a withdraw.
        None => &["cancelled"],
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
        // The ledger migration (R2-03) adds the moves to applied, failed and
        // undone, with their ledger entry.
        Some("approved") => &["in_review", "denied", "expired", "cancelled"],
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

/// The submit of the newest revision, as `iris-changeset` writes it: the
/// hash of each draft action, then the submit time. The trigger proposes
/// each action.
fn submit(c: &Connection, cs: i64) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE changeset_action SET hash = 'h' WHERE changeset_id = ?1 AND state IS NULL",
        [cs],
    )?;
    c.execute(
        "UPDATE changeset SET why = 'w', submitted_at = 0 WHERE id = ?1",
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

/// The decision columns that go with a state, as a person, a rule or the
/// engine writes them.
fn decision(from: Option<&str>, to: &str) -> &'static str {
    match to {
        "proposed" => {
            "decided_by = NULL, rule = NULL, policy = NULL, reason = NULL, decided_at = NULL"
        }
        "expired" => {
            "decided_by = 'expiry', rule = NULL, policy = NULL, reason = NULL,
             decided_at = 1209600000"
        }
        "cancelled" | "replaced" => {
            "decided_by = 'author', rule = NULL, policy = NULL, reason = NULL,
             decided_at = 1209600000"
        }
        "applied" | "failed" if from.is_some() => "decided_by = decided_by",
        "approved" | "denied" if from != Some("in_review") => {
            "decided_by = 'rule', rule = 1, policy = 1, reason = 'r', decided_at = 1209600000"
        }
        "in_review" => {
            "decided_by = 'rule', rule = 1, policy = 1, reason = 'r', decided_at = 1209600000"
        }
        _ => {
            "decided_by = 'person', rule = NULL, policy = NULL, reason = 'r',
             decided_at = 1209600000"
        }
    }
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
                if let Some(from) = from {
                    // Set the from state past the trigger, then put it back.
                    let trigger: String = tx.query_row(
                        "SELECT sql FROM sqlite_master WHERE name = 'action_state'",
                        [],
                        |r| r.get(0),
                    )?;
                    tx.execute_batch("DROP TRIGGER action_state")?;
                    tx.execute(
                        &format!(
                            "UPDATE changeset_action SET state = ?1, hash = 'h', submitted_at = 0,
                               {}",
                            decision(None, from)
                        ),
                        [from],
                    )?;
                    tx.execute_batch(&trigger)?;
                    tx.execute("UPDATE changeset SET submitted_at = 0, why = 'w'", [])?;
                }
                let now = |tx: &rusqlite::Transaction| -> rusqlite::Result<Option<String>> {
                    tx.query_row("SELECT state FROM changeset_action WHERE id = 1", [], |r| {
                        r.get(0)
                    })
                };
                // A withdraw cancels the action in the same write, or leaves it.
                if to == "cancelled" {
                    tx.execute("UPDATE changeset SET state = 'withdrawn'", [])?;
                    return Ok(from != Some("cancelled") && now(tx)?.as_deref() == Some(to));
                }
                // The submit of a newer revision replaces the action, or
                // leaves it.
                if to == "replaced" && from.is_some() {
                    tx.execute_batch(
                        "UPDATE changeset SET revision = 2, submitted_at = NULL;
                         INSERT INTO changeset_action(changeset_id, revision, n, verb, targets,
                           args, hash) VALUES (1, 2, 1, 'archive', '{}', '{}', 'h');
                         UPDATE changeset SET submitted_at = 1209600000;",
                    )?;
                    return Ok(from != Some("replaced") && now(tx)?.as_deref() == Some(to));
                }
                // 14 days after the submit: the event of expired.
                let sql = format!(
                    "UPDATE changeset_action SET state = ?1, {} WHERE changeset_id = ?2",
                    decision(from, to)
                );
                Ok(tx.execute(&sql, rusqlite::params![to, cs]).is_ok())
            });
            let expect = allowed(from).contains(&to);
            assert_eq!(r.unwrap(), expect, "{from:?} -> {to:?}");
        }
    }
}

/// The submit of a revision proposes each of its actions in the same write.
#[test]
fn a_submit_proposes_each_action() {
    let dir = TempDir::new("proposes");
    let s = store(&dir);
    s.write(|tx| Ok(draft(tx)?)).unwrap();
    // No submit with an action that has no hash, or with no action.
    fails(&s, "UPDATE changeset SET submitted_at = 0, why = 'w'");
    fails(
        &s,
        "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0);
         UPDATE changeset SET submitted_at = 0, why = 'w' WHERE id = last_insert_rowid();",
    );
    s.write(|tx| Ok(submit(tx, 1)?)).unwrap();
    let row: (String, i64, String) = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT a.state, a.submitted_at, c.state FROM changeset_action a
                 JOIN changeset c ON c.id = a.changeset_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(row, ("proposed".into(), 0, "open".into()));
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
    // An action keeps its ID, and stays in its changeset and revision.
    fails(&s, "UPDATE changeset_action SET id = 7");
    // Also an action with no message row, where no foreign key holds it.
    fails(
        &s,
        "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
         VALUES (1, 1, 5, 'close', '{}', '{}');
         UPDATE changeset_action SET id = 70 WHERE n = 5;",
    );
    works(
        &s,
        "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
         VALUES (1, 1, 5, 'close', '{}', '{}');",
    );
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
        // Not at a time after the clock.
        "UPDATE changeset_action SET state = 'expired', decided_by = 'expiry',
           decided_at = CAST(unixepoch('subsec') * 1000 AS INTEGER) + 60000",
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
    // No submit in the same write as a withdraw: each submit has its row.
    fails(
        &s,
        "UPDATE changeset_action SET hash = 'h' WHERE revision = 2;
         UPDATE changeset SET state = 'withdrawn', why = 'w', submitted_at = 123;",
    );
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
            draft(tx)?;
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

/// The submit of a new revision replaces the proposed and in_review actions
/// of the older revisions, and a new revision makes the changeset a draft.
#[test]
fn a_new_revision_replaces_and_is_a_draft() {
    let dir = TempDir::new("newrev");
    let s = store(&dir);
    let state = |s: &Store, sql: &str| -> String {
        s.read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
    };
    s.write(|tx| {
        submit(tx, draft(tx)?)?;
        Ok(tx.execute(
            "UPDATE changeset_action SET state = 'in_review', decided_by = 'policy'",
            [],
        )?)
    })
    .unwrap();
    s.write(|tx| Ok(tx.execute("UPDATE changeset SET revision = 2, submitted_at = NULL", [])?))
        .unwrap();
    assert_eq!(state(&s, "SELECT state FROM changeset"), "draft");
    s.write(|tx| {
        Ok(tx.execute_batch(
            "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, hash)
             VALUES (1, 2, 1, 'archive', '{}', '{}', 'h');
             UPDATE changeset SET submitted_at = 5;",
        )?)
    })
    .unwrap();
    assert_eq!(
        state(&s, "SELECT state FROM changeset_action WHERE revision = 1"),
        "replaced"
    );
    assert_eq!(state(&s, "SELECT state FROM changeset"), "open");
    fails(
        &s,
        "UPDATE changeset_action SET state = 'approved', decided_by = 'person'
         WHERE revision = 1",
    );
    s.write(|tx| {
        Ok(tx.execute(
            "UPDATE changeset_action SET state = 'denied', decided_by = 'person', reason = 'No.'
             WHERE revision = 2",
            [],
        )?)
    })
    .unwrap();
    assert_eq!(state(&s, "SELECT state FROM changeset"), "closed");
    // An approved action of revision 1 comes back to review after the
    // submit of revision 2 (section 5.5). Only the next submit replaces it,
    // also with a clock that went back (15 is before the review at 21).
    s.write(|tx| {
        Ok(tx.execute_batch(
            "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0);
             INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, hash)
             VALUES (2, 1, 1, 'archive', '{}', '{}', 'h');
             UPDATE changeset SET why = 'w', submitted_at = 10 WHERE id = 2;
             UPDATE changeset_action SET state = 'approved', decided_by = 'person',
               decided_at = 11 WHERE changeset_id = 2;
             UPDATE changeset SET revision = 2, submitted_at = NULL WHERE id = 2;
             INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, hash)
             VALUES (2, 2, 1, 'archive', '{}', '{}', 'h');
             UPDATE changeset SET submitted_at = 20 WHERE id = 2;
             UPDATE changeset_action SET state = 'in_review', decided_by = 'policy',
               decided_at = 21 WHERE changeset_id = 2 AND revision = 1;",
        )?)
    })
    .unwrap();
    // The marker of the replace is set only in a submit.
    for r in [2, 3] {
        fails(
            &s,
            &format!("UPDATE changeset SET replacing = {r} WHERE id = 2"),
        );
    }
    for at in [20, 21, 22] {
        fails(
            &s,
            &format!(
                "UPDATE changeset_action SET state = 'replaced', decided_by = 'author',
                   rule = NULL, policy = NULL, reason = NULL, decided_at = {at}
                 WHERE changeset_id = 2 AND revision = 1"
            ),
        );
    }
    s.write(|tx| {
        Ok(tx.execute_batch(
            "UPDATE changeset SET revision = 3, submitted_at = NULL WHERE id = 2;
             INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, hash)
             VALUES (2, 3, 1, 'archive', '{}', '{}', 'h');
             UPDATE changeset SET submitted_at = 15 WHERE id = 2;",
        )?)
    })
    .unwrap();
    assert_eq!(
        state(
            &s,
            "SELECT state FROM changeset_action WHERE changeset_id = 2 AND revision = 1"
        ),
        "replaced"
    );
    // Each submitted revision keeps its reason and its time.
    let revisions: Vec<(i64, String, i64)> = s
        .read(|c| {
            let mut q = c.prepare(
                "SELECT revision, why, submitted_at FROM changeset_revision
                 WHERE changeset_id = 2 ORDER BY revision",
            )?;
            let rows = q
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(
        revisions,
        [
            (1, "w".into(), 10),
            (2, "w".into(), 20),
            (3, "w".into(), 15)
        ]
    );
    for sql in [
        "UPDATE changeset_revision SET why = 'x'",
        "UPDATE changeset_revision SET replaced = 0",
        "DELETE FROM changeset_revision",
        "INSERT INTO changeset_revision VALUES (2, 4, 'x', 40)",
        "INSERT INTO changeset_revision VALUES (1, 3, 'w', 0)",
        // An insert does not replace a revision row, so the marker of the
        // replace does not come back.
        "INSERT OR REPLACE INTO changeset_revision(changeset_id, revision, why, submitted_at)
         VALUES (2, 3, 'w', 15)",
        "INSERT OR REPLACE INTO changeset_revision(changeset_id, revision, why, submitted_at)
         VALUES (2, 3, 'w', 15);
         UPDATE changeset SET replacing = 3 WHERE id = 2;",
    ] {
        fails(&s, sql);
    }
    // All actions decided, then a new revision: the changeset is a draft.
    s.write(|tx| {
        Ok(tx.execute(
            "UPDATE changeset SET revision = 3, submitted_at = NULL WHERE id = 1",
            [],
        )?)
    })
    .unwrap();
    assert_eq!(
        state(&s, "SELECT state FROM changeset WHERE id = 1"),
        "draft"
    );
}

/// The moves are part of the trigger. No table holds them.
#[test]
fn no_write_adds_a_move() {
    let dir = TempDir::new("nomoves");
    let s = store(&dir);
    let tables: i64 = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name LIKE '%transition%'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(tables, 0);
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
    // An action in review waits for a person: no rule decides it.
    s.write(|tx| {
        Ok(tx.execute(
            "UPDATE changeset_action SET state = 'in_review', decided_by = 'policy'",
            [],
        )?)
    })
    .unwrap();
    fails(&s, &rule(3, 2));
    fails(
        &s,
        "UPDATE changeset_action SET state = 'denied', decided_by = 'rule', rule = 3,
           policy = 2, reason = 'r'",
    );
    works(
        &s,
        "UPDATE changeset_action SET state = 'approved', decided_by = 'person'",
    );
    // Each end names its own actor.
    for sql in [
        "UPDATE changeset_action SET state = 'expired', decided_by = 'rule', rule = 3,
           policy = 2, decided_at = 1209600000",
        "UPDATE changeset_action SET state = 'expired', decided_by = 'person',
           decided_at = 1209600000",
    ] {
        fails(&s, sql);
    }
    works(
        &s,
        "UPDATE changeset_action SET state = 'expired', decided_by = 'expiry',
           decided_at = 1209600000",
    );
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
        "UPDATE changeset_action SET id = 2",
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

/// No apply, failure or undo before the ledger (R2-03), and no flag on a
/// closed action.
#[test]
fn no_apply_before_the_ledger() {
    let dir = TempDir::new("noapply");
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
    for to in ["applied", "failed", "undone"] {
        fails(&s, &format!("UPDATE changeset_action SET state = '{to}'"));
    }
    fails(
        &s,
        "UPDATE changeset_action SET state = 'denied', decided_by = 'person', reason = 'No.',
           flags = '[\"bulk\"]'",
    );
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
    s.write(|tx| {
        Ok(tx.execute(
            "UPDATE changeset_action SET state = 'denied', decided_by = 'person', reason = 'No.'",
            [],
        )?)
    })
    .unwrap();
    assert_eq!(state(&s), "closed");
}
