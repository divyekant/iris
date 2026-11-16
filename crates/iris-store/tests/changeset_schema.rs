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
        // approved to held comes with release 3. applied, failed and
        // undone come only with the state of the ledger entry (migration
        // 10, trigger ledger_state): see the_ledger_moves_the_action.
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

/// Migration 11: the state of the ledger entry moves the action. The
/// action keeps the decision that approved it. A row in `writing` keeps the
/// action through a withdraw, and the action then ends only as its row
/// ends.
#[test]
fn the_ledger_moves_the_action() {
    let dir = TempDir::new("ledger");
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
    const CLAIM: &str = "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
         VALUES (1, 1, '{}', 'writing', 'inverse', 7);
         INSERT INTO ledger_message(ledger_id, message_id, call, pre, post)
         VALUES (last_insert_rowid(), 1, 1, '[\"INBOX\"]', '[]');";
    // A new entry claims an approved action, in writing, before its calls.
    for bad in ["'applied', 'inverse', 7", "'writing', 'other', 7"] {
        refused(
            &s,
            &format!(
                "INSERT INTO ledger(action_id, ops, state, undo, at) VALUES (1, '{{}}', {bad})"
            ),
        );
    }
    refused(
        &s,
        "INSERT INTO ledger(action_id, ops, state, undo, at, call)
         VALUES (1, '{}', 'writing', 'inverse', 7, 1)",
    );
    refused(
        &s,
        "INSERT INTO ledger(action_id, ops, state, undo, at)
         VALUES (99, '{}', 'writing', 'inverse', 7)",
    );
    refused(&s, &format!("{CLAIM} {CLAIM}"));
    // Only a resolved message of the action, and only before the first call.
    refused(
        &s,
        "INSERT INTO ledger(action_id, ops, state, undo, at) VALUES (1, '{}', 'writing', 'inverse', 7);
         INSERT INTO ledger_message(ledger_id, message_id, call, pre, post) VALUES (1, 2, 1, '[]', '[]')",
    );
    refused(
        &s,
        &format!("{CLAIM} UPDATE ledger SET call = 1;
         INSERT INTO ledger_message(ledger_id, message_id, call, pre, post) VALUES (1, 2, 2, '[]', '[]')"),
    );
    // No direct move of the action, also with a row.
    refused(
        &s,
        &format!("{CLAIM} UPDATE changeset_action SET state = 'applied'"),
    );
    for to in ["applied", "failed"] {
        works(
            &s,
            &format!("{CLAIM} UPDATE ledger SET call = 1; UPDATE ledger SET state = '{to}'"),
        );
        // A withdraw keeps an action with a row in writing.
        works(
            &s,
            &format!(
                "{CLAIM} UPDATE changeset SET state = 'withdrawn';
                 UPDATE ledger SET state = '{to}'"
            ),
        );
    }
    // While its row is in writing, the action ends only as the row ends.
    for other in [
        "state = 'denied', decided_by = 'person', rule = NULL, policy = NULL, reason = 'No.'",
        "state = 'in_review', decided_by = 'engine', rule = NULL, policy = NULL, reason = 'r'",
        "state = 'expired', decided_by = 'expiry', rule = NULL, policy = NULL,
           decided_at = unixepoch() * 1000",
    ] {
        refused(&s, &format!("{CLAIM} UPDATE changeset_action SET {other}"));
    }
    // The progress of the calls: forward only, and only in writing.
    for bad in [
        "UPDATE ledger SET call = 2; UPDATE ledger SET call = 1",
        "UPDATE ledger SET ops = '[]'",
        "UPDATE ledger SET at = 8",
        "UPDATE ledger SET state = 'undone', undone_by = 1",
        "UPDATE ledger SET state = 'applied'; UPDATE ledger SET state = 'writing'",
        "UPDATE ledger SET state = 'applied'; UPDATE ledger SET call = 3",
        "UPDATE ledger SET state = 'applied'; UPDATE ledger SET error = 'e'",
        "UPDATE ledger SET state = 'applied'; UPDATE ledger_message SET done = 1",
        "UPDATE ledger_message SET done = 1; UPDATE ledger_message SET done = 0",
        "UPDATE ledger_message SET listed = 'gone'; UPDATE ledger_message SET listed = 'unknown'",
        "UPDATE ledger_message SET listed = 'gone', done = 1",
        "UPDATE ledger SET call = 1; UPDATE ledger_message SET pre = '[]'",
        "UPDATE ledger_message SET call = 2",
        "DELETE FROM ledger_message",
        "DELETE FROM ledger_message; DELETE FROM ledger",
    ] {
        refused(&s, &format!("{CLAIM} {bad}"));
    }
    // The recovery reads `pre` again for a call that never started.
    works(&s, &format!("{CLAIM} UPDATE ledger_message SET pre = '[]'"));
    // Two rows in writing never share a message.
    refused(
        &s,
        &format!(
            "{CLAIM}
             UPDATE changeset SET revision = 2, submitted_at = NULL;
             INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
             VALUES (1, 2, 1, 'archive', '{{}}', '{{}}');
             INSERT INTO action_message(action_id, message_id) VALUES (2, 1);
             UPDATE changeset_action SET hash = 'h' WHERE id = 2;
             UPDATE changeset SET submitted_at = 1;
             UPDATE changeset_action SET state = 'approved', decided_by = 'person',
               decided_at = 5 WHERE id = 2;
             INSERT INTO ledger(action_id, ops, state, undo, at)
             VALUES (2, '{{}}', 'writing', 'inverse', 7);
             INSERT INTO ledger_message(ledger_id, message_id, call, pre, post)
             VALUES (2, 1, 1, '[]', '[]')"
        ),
    );
    // A failed entry with no changed message has nothing to undo.
    refused(
        &s,
        &format!(
            "{CLAIM} UPDATE ledger SET state = 'failed';
             UPDATE ledger SET state = 'undone', undone_by = 1"
        ),
    );
    // The applied action keeps its decision, the changeset closes, and the
    // undo moves the action to undone.
    s.write(|tx| {
        tx.execute_batch(&format!(
            "{CLAIM} UPDATE ledger SET call = 1; UPDATE ledger_message SET done = 1;
             UPDATE ledger SET state = 'applied'"
        ))?;
        Ok(())
    })
    .unwrap();
    let row: (String, String, i64, String) = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT a.state, a.decided_by, a.rule, c.state FROM changeset_action a
                 JOIN changeset c ON c.id = a.changeset_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap();
    assert_eq!(row, ("applied".into(), "rule".into(), 3, "closed".into()));
    refused(&s, "UPDATE changeset_action SET state = 'undone'");
    refused(&s, "UPDATE ledger SET state = 'undone'");
    refused(&s, "UPDATE ledger SET state = 'undone', undone_by = 99");
    refused(&s, "UPDATE ledger SET history_id = '5'");
    refused(&s, "UPDATE ledger SET state = 'undone', undone_by = 1");
    // The undo is an action of its own, with its own entry, in a changeset
    // of the person (migration 12). An action with no link is not an undo.
    const UNDO: &str = "INSERT INTO changeset(author_kind, created_at) VALUES ('person', 6);
         INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
         VALUES (2, 1, 1, 'inbox', '{}', '{}');";
    const UNDO_APPLIED: &str = "INSERT INTO action_message(action_id, message_id) VALUES (2, 1);
         UPDATE changeset_action SET hash = 'h' WHERE id = 2;
         UPDATE changeset SET why = 'u', submitted_at = 6 WHERE id = 2;
         UPDATE changeset_action SET state = 'approved', decided_by = 'person',
           decided_at = 6 WHERE id = 2;
         INSERT INTO ledger(action_id, ops, state, undo, at)
         VALUES (2, '{}', 'writing', 'inverse', 7);
         UPDATE ledger SET state = 'applied' WHERE id = 2;";
    refused(
        &s,
        &format!(
            "{UNDO} {UNDO_APPLIED} UPDATE ledger SET state = 'undone', undone_by = 2 WHERE id = 1"
        ),
    );
    // The link: a draft of the person, step 1 or 2, one open undo of each
    // step, and no change after.
    for bad in [
        "INSERT INTO ledger_undo VALUES (2, 1, 3)",
        "INSERT INTO ledger_undo VALUES (2, 2, 1)",
        "UPDATE changeset SET author_kind = 'agent' WHERE id = 2;
         INSERT INTO ledger_undo VALUES (2, 1, 1)",
        "INSERT INTO ledger_undo VALUES (1, 1, 1)",
        // The verb of the link fits the entry: the entry is an `archive`.
        "UPDATE changeset_action SET verb = 'untrash' WHERE id = 2;
         INSERT INTO ledger_undo VALUES (2, 1, 1)",
        "UPDATE changeset_action SET verb = 'archive' WHERE id = 2;
         INSERT INTO ledger_undo VALUES (2, 1, 2)",
        "UPDATE changeset_action SET args = '{\"label\":\"x\"}' WHERE id = 2;
         INSERT INTO ledger_undo VALUES (2, 1, 1)",
        "INSERT INTO ledger_undo VALUES (2, 1, 1);
         INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
         VALUES (2, 1, 2, 'inbox', '{}', '{}');
         INSERT INTO ledger_undo VALUES (3, 1, 1)",
        "INSERT INTO ledger_undo VALUES (2, 1, 1); UPDATE ledger_undo SET step = 2",
        "INSERT INTO ledger_undo VALUES (2, 1, 1); DELETE FROM ledger_undo",
    ] {
        refused(&s, &format!("{UNDO} {bad}"));
    }
    s.write(|tx| {
        tx.execute_batch(&format!(
            "{UNDO} INSERT INTO ledger_undo VALUES (2, 1, 1); {UNDO_APPLIED}
             UPDATE ledger SET state = 'undone', undone_by = 2 WHERE id = 1;"
        ))?;
        Ok(())
    })
    .unwrap();
    // An undo entry undoes one entry only.
    refused(
        &s,
        "UPDATE changeset SET revision = 2, submitted_at = NULL WHERE id = 1;
         INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
         VALUES (1, 2, 1, 'archive', '{}', '{}');
         UPDATE changeset_action SET hash = 'h' WHERE id = 3;
         UPDATE changeset SET submitted_at = 2 WHERE id = 1;
         UPDATE changeset_action SET state = 'approved', decided_by = 'person',
           decided_at = 5 WHERE id = 3;
         INSERT INTO ledger(action_id, ops, state, undo, at)
         VALUES (3, '{}', 'writing', 'inverse', 7);
         UPDATE ledger SET state = 'applied' WHERE id = 3;
         UPDATE ledger SET state = 'undone', undone_by = 2 WHERE id = 3",
    );
    let undone: String = s
        .read(|c| {
            Ok(
                c.query_row("SELECT state FROM changeset_action WHERE id = 1", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(undone, "undone");
}

/// As `fails`, and the store refused the write with an Iris rule (a
/// trigger or a constraint), not for a defect of the SQL text.
fn refused(s: &Store, sql: &str) {
    let e = s
        .dry_write(|tx| Ok(tx.execute_batch(sql)?))
        .expect_err(sql)
        .to_string();
    assert!(
        e.contains("iris:") || e.contains("constraint failed"),
        "{sql}: {e}"
    );
}

/// Migration 11 gives each applied or failed action of migration 9 an
/// entry with no undo, and fails an action with the mark `applying`: its
/// calls started, and nothing lists them.
#[test]
fn migration_11_keeps_the_rule_of_migration_9() {
    let dir = TempDir::new("migrate10");
    fs::create_dir_all(&dir.0).unwrap();
    // A store at migration 9: Store::open makes the key and the folder,
    // then the test steps the database back by hand.
    drop(Store::open(&dir.0).unwrap());
    let db = dir.0.join("iris.db");
    fs::remove_file(&db).unwrap();
    let c = Connection::open(&db).unwrap();
    for sql in [
        include_str!("../src/schema_1.sql"),
        include_str!("../src/schema_2.sql"),
        include_str!("../src/schema_3.sql"),
        include_str!("../src/schema_4.sql"),
        include_str!("../src/schema_5.sql"),
        include_str!("../src/schema_6.sql"),
        include_str!("../src/schema_7.sql"),
        include_str!("../src/schema_8.sql"),
        include_str!("../src/schema_9.sql"),
    ] {
        c.execute_batch(sql).unwrap();
    }
    c.pragma_update(None, "user_version", 9).unwrap();
    c.execute_batch(
        "INSERT INTO account(provider, address) VALUES ('gmail', 'owner@example.com');
         INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('a', 'h1', 0);",
    )
    .unwrap();
    for (n, end) in [
        (1, "approved"),
        (2, "applied"),
        (3, "failed"),
        (4, "marked"),
    ] {
        c.execute_batch(&format!(
            "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0);
             INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
             VALUES ({n}, 1, 1, 'archive', '{{}}', '{{}}');
             UPDATE changeset_action SET hash = 'h' WHERE id = {n};
             UPDATE changeset SET why = 'w', submitted_at = 0 WHERE id = {n};
             UPDATE changeset_action SET state = 'approved', decided_by = 'person',
               decided_at = 1 WHERE id = {n};"
        ))
        .unwrap();
        match end {
            "approved" => {}
            "marked" => c
                .execute(
                    "UPDATE changeset_action SET applying = 1 WHERE id = ?1",
                    [n],
                )
                .map(|_| ())
                .unwrap(),
            _ => c
                .execute(
                    "UPDATE changeset_action SET state = ?2 WHERE id = ?1",
                    rusqlite::params![n, end],
                )
                .map(|_| ())
                .unwrap(),
        }
    }
    drop(c);
    let s = Store::open(&dir.0).unwrap();
    let rows: Vec<(i64, String, Option<String>, Option<String>)> = s
        .read(|c| {
            let mut st = c.prepare(
                "SELECT a.id, a.state, l.state, l.undo FROM changeset_action a
                 LEFT JOIN ledger l ON l.action_id = a.id ORDER BY a.id",
            )?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    let some = |t: &str| Some(t.to_string());
    assert_eq!(
        rows,
        vec![
            (1, "approved".into(), None, None),
            (2, "applied".into(), some("applied"), some("none")),
            (3, "failed".into(), some("failed"), some("none")),
            (4, "failed".into(), some("failed"), some("none")),
        ]
    );
    // A final entry has no undo, also with a later entry that ended.
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO ledger(action_id, ops, state, undo, at)
             VALUES (1, '{}', 'writing', 'inverse', 7);
             UPDATE ledger SET state = 'applied' WHERE action_id = 1;",
        )?;
        Ok(())
    })
    .unwrap();
    refused(
        &s,
        "UPDATE ledger SET state = 'undone',
           undone_by = (SELECT id FROM ledger WHERE action_id = 1)
         WHERE action_id = 2",
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

/// Migration 11: a ledger entry pins the items of its action (specification
/// 1, section 7.5), also after the action left the open states.
#[test]
fn a_ledger_entry_pins_its_items() {
    let dir = TempDir::new("pin");
    let s = store(&dir);
    s.write(|tx| {
        let cs = draft(tx)?;
        tx.execute(
            "INSERT INTO action_item(action_id, item_id) VALUES (1, 42)",
            [],
        )?;
        submit(tx, cs)?;
        tx.execute_batch(
            "UPDATE changeset_action SET state = 'approved', decided_by = 'person',
               decided_at = 5;
             INSERT INTO ledger(action_id, ops, state, undo, at)
             VALUES (1, '{}', 'writing', 'inverse', 7);
             UPDATE ledger SET state = 'applied';",
        )?;
        Ok(())
    })
    .unwrap();
    let pinned: bool = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT EXISTS (SELECT 1 FROM item_pin WHERE item_id = 42)",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(pinned);
}
