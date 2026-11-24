//! The store rules of matters (migration 16) and the writes of the person.

use iris_matters::{
    check_title, clean, confirm, confirm_forms, decline, insert_proposal, write, Error,
};
use iris_store::{NewMessage, Store};
use rusqlite::{params, Connection};

/// A small xorshift generator: the tests need no crate for it.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
    fn chance(&mut self, per_mille: u64) -> bool {
        self.below(1000) < per_mille
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

struct Box {
    store: Store,
    dir: std::path::PathBuf,
}

impl Drop for Box {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A store with `threads` threads of one message each. Item `i` (row ID
/// `i + 1`) has the form `forms[i]` and its origin in thread `i % threads`.
fn setup(name: &str, threads: usize, forms: &[&str]) -> Box {
    let dir = std::env::temp_dir().join(format!("iris-matters-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let a = store.add_account("gmail", "owner@example.com").unwrap();
    let msgs: Vec<NewMessage> = (0..threads)
        .map(|t| NewMessage {
            provider_id: format!("g{t}"),
            thread_provider_id: format!("gt{t}"),
            ..Default::default()
        })
        .collect();
    let ids = store.put_messages(a, &msgs).unwrap();
    store
        .write(|tx| {
            for (i, form) in forms.iter().enumerate() {
                tx.execute(
                    "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                     VALUES (?1, ?2, 'open', '{}', 0, 0)",
                    params![form, a],
                )?;
                tx.execute(
                    "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
                    params![tx.last_insert_rowid(), ids[i % threads]],
                )?;
            }
            Ok(())
        })
        .unwrap();
    Box { store, dir }
}

fn propose(b: &Box, items: &[i64]) -> Result<i64, Error> {
    write(&b.store, |tx| {
        insert_proposal(tx, "Tokyo trip", "fake", None, items, 1)
    })
}

fn read<T>(b: &Box, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> T {
    b.store.read(|c| Ok(f(c)?)).unwrap()
}

fn status(b: &Box, m: i64) -> String {
    read(b, |c| {
        c.query_row("SELECT status FROM matter WHERE id = ?1", [m], |r| r.get(0))
    })
}

fn pinned(b: &Box) -> Vec<i64> {
    read(b, |c| {
        let mut s = c.prepare("SELECT item_id FROM item_pin ORDER BY item_id")?;
        let v = s.query_map([], |r| r.get(0))?.collect();
        v
    })
}

fn sql(b: &Box, text: &str) -> rusqlite::Result<usize> {
    b.store.write(|tx| Ok(tx.execute(text, []))).unwrap()
}

const FORMS: [&str; 5] = ["record", "conversation", "commitment", "record", "feed"];

/// A member with another form than its row is not a member, and a matter
/// that is not there answers `NotFound` before any check of the members.
#[test]
fn confirm_forms_checks_each_form_in_the_decision() {
    let b = setup("forms", 4, &FORMS);
    let m = propose(&b, &[1, 2, 3]).unwrap();
    for wrong in [
        vec![(1, "conversation")],
        vec![(1, "record"), (2, "record")],
        vec![(3, "record"), (1, "record")],
    ] {
        assert!(
            matches!(
                confirm_forms(&b.store, m, "Trip", &wrong, 2),
                Err(Error::Bad(_))
            ),
            "{wrong:?}"
        );
    }
    assert_eq!(status(&b, m), "proposed");
    assert!(matches!(
        confirm_forms(&b.store, 99, "Trip", &[(1, "conversation")], 2),
        Err(Error::NotFound)
    ));
    confirm_forms(
        &b.store,
        m,
        "Trip",
        &[(2, "conversation"), (1, "record")],
        2,
    )
    .unwrap();
    assert_eq!(pinned(&b), vec![1, 2]);
}

#[test]
fn confirm_keeps_some_members_and_pins_them() {
    let b = setup("confirm", 4, &FORMS);
    let m = propose(&b, &[1, 2, 3]).unwrap();
    assert!(pinned(&b).is_empty(), "a proposal pins nothing");
    confirm(&b.store, m, "  Tokyo  ", &[1, 2], 5).unwrap();
    assert_eq!(status(&b, m), "confirmed");
    let (title, decided): (String, i64) = read(&b, |c| {
        c.query_row(
            "SELECT title, decided_at FROM matter WHERE id = ?1",
            [m],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    });
    assert_eq!((title.as_str(), decided), ("Tokyo", 5));
    assert_eq!(
        pinned(&b),
        vec![1, 2],
        "only kept members of a confirmed matter"
    );
    // The member that the person did not keep is a candidate again.
    let m2 = propose(&b, &[3, 4]).unwrap();
    assert!(m2 > m);
    // A second decision changes nothing.
    assert!(matches!(
        confirm(&b.store, m, "x", &[1, 2], 6),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(decline(&b.store, m, 6), Err(Error::Conflict(_))));
    assert!(matches!(decline(&b.store, 99, 6), Err(Error::NotFound)));
}

#[test]
fn confirm_refuses_bad_requests() {
    let b = setup("bad", 4, &FORMS);
    let m = propose(&b, &[1, 2, 3]).unwrap();
    for (title, keep) in [
        ("", vec![1]),
        ("a\nb", vec![1]),
        ("Tokyo", vec![]),
        ("Tokyo", vec![1, 1]),
        ("Tokyo", vec![4]),
    ] {
        let r = confirm(&b.store, m, title, &keep, 2);
        assert!(matches!(r, Err(Error::Bad(_))), "{title:?} {keep:?}: {r:?}");
    }
    assert_eq!(status(&b, m), "proposed");
    let long = "x".repeat(81);
    assert!(check_title(&long).is_none());
    assert_eq!(check_title(&"é".repeat(80)).unwrap().chars().count(), 80);
}

#[test]
fn decline_pins_each_member() {
    let b = setup("decline", 4, &FORMS);
    let m = propose(&b, &[1, 2, 3]).unwrap();
    decline(&b.store, m, 3).unwrap();
    assert_eq!(status(&b, m), "declined");
    assert_eq!(pinned(&b), vec![1, 2, 3]);
    // The members are free for a new proposal.
    propose(&b, &[1, 2]).unwrap();
}

#[test]
fn the_triggers_hold_the_member_rules() {
    let b = setup("rules", 4, &FORMS);
    // A feed item is never a member.
    assert!(propose(&b, &[1, 5]).is_err());
    // An item is a kept member of one open matter only.
    let m = propose(&b, &[1, 2]).unwrap();
    assert!(propose(&b, &[2, 3]).is_err());
    let m2 = propose(&b, &[3, 4]).unwrap();
    // Members go in only with the proposal write, also for the newest
    // proposal, and `kept` changes only with the confirm.
    for text in [
        format!("INSERT INTO matter_item(matter_id, item_id) VALUES ({m}, 4)"),
        format!("INSERT INTO matter_item(matter_id, item_id) VALUES ({m2}, 2)"),
        format!("UPDATE matter_item SET kept = 0 WHERE matter_id = {m2}"),
        format!("UPDATE matter SET sealed = 0 WHERE id = {m2}"),
    ] {
        assert!(sql(&b, &text).is_err(), "{text}");
    }
    // A direct delete of a member row, or a change of it, is refused.
    assert!(sql(
        &b,
        &format!("DELETE FROM matter_item WHERE matter_id = {m}")
    )
    .is_err());
    assert!(sql(
        &b,
        &format!("UPDATE matter_item SET item_id = 4 WHERE matter_id = {m}")
    )
    .is_err());
    // A decided matter does not go back to a proposal and is not deleted
    // (migration 46 lets the person edit a confirmed matter: see edit.rs).
    confirm(&b.store, m, "Trip", &[1], 2).unwrap();
    for text in [
        format!("UPDATE matter SET status = 'proposed', decided_at = NULL WHERE id = {m}"),
        format!("DELETE FROM matter WHERE id = {m}"),
        "INSERT INTO matter(title, status, model, prompt, created_at, updated_at, decided_at)
         VALUES ('x', 'confirmed', 'm', 'p', 0, 0, 0)"
            .to_string(),
        "INSERT INTO matter(title, status, model, prompt, created_at, updated_at, sealed)
         VALUES ('x', 'proposed', 'm', 'p', 0, 0, 1)"
            .to_string(),
    ] {
        assert!(sql(&b, &text).is_err(), "{text}");
    }
    // `INSERT OR REPLACE` does not replace a decided matter.
    assert!(sql(
        &b,
        &format!(
            "INSERT OR REPLACE INTO matter(id, title, status, model, prompt, created_at, updated_at)
             VALUES ({m}, 'x', 'proposed', 'm', 'p', 0, 0)"
        )
    )
    .is_err());
    assert_eq!(status(&b, m), "confirmed");
    // A proposal can be deleted, with its member rows.
    assert_eq!(
        sql(&b, &format!("DELETE FROM matter WHERE id = {m2}")).unwrap(),
        1
    );
    let rows: i64 = read(&b, |c| {
        c.query_row(
            "SELECT count(*) FROM matter_item WHERE matter_id = ?1",
            [m2],
            |r| r.get(0),
        )
    });
    assert_eq!(rows, 0);
}

/// A decision by any write path: the store accepts a confirm only with a
/// `keep` of 1 or more distinct live members, and a decision only on a
/// valid proposal. Each try runs in a savepoint that rolls back.
#[test]
fn a_decision_needs_a_valid_proposal_and_a_live_keep() {
    // Items 1 to 3 are members (3 is hidden, so not live), 4 is not.
    let b = setup("decide", 4, &FORMS);
    let m = propose(&b, &[1, 2, 3]).unwrap();
    sql(
        &b,
        r#"UPDATE item SET doc = '{"attention":{"state":"hidden"}}' WHERE id = 3"#,
    )
    .unwrap();
    let parts = ["1", "2", "3", "4", "\"1\"", "1.5", "null", "[1]"];
    let mut rng = Rng::new(7);
    let mut accepted = 0;
    for round in 0..400 {
        let n = rng.below(4) as usize;
        let picked: Vec<&str> = (0..n).map(|_| *rng.pick(&parts)).collect();
        let keep = match round % 10 {
            0 => "not json".to_string(),
            1 => "{\"a\": 1}".to_string(),
            _ => format!("[{}]", picked.join(",")),
        };
        let mut distinct = picked.clone();
        distinct.sort();
        distinct.dedup();
        let good = round % 10 > 1
            && n > 0
            && distinct.len() == n
            && picked.iter().all(|p| *p == "1" || *p == "2");
        let text = format!(
            "UPDATE matter SET status = 'confirmed', decided_at = 1, keep = '{keep}' WHERE id = {m}"
        );
        let done = b
            .store
            .write(|tx| {
                tx.execute_batch("SAVEPOINT try")?;
                let r = tx.execute(&text, []);
                tx.execute_batch("ROLLBACK TO try; RELEASE try")?;
                Ok(r.is_ok())
            })
            .unwrap();
        assert_eq!(done, good, "keep {keep}");
        accepted += usize::from(done);
    }
    assert!(accepted > 0, "the test made no good confirm");
    assert_eq!(status(&b, m), "proposed");
    // A proposal that is not valid takes no decision, also by SQL.
    sql(&b, "UPDATE item SET status = 'gone' WHERE id = 2").unwrap();
    for text in [
        format!("UPDATE matter SET status = 'declined', decided_at = 1 WHERE id = {m}"),
        format!(
            "UPDATE matter SET status = 'confirmed', decided_at = 1, keep = '[1]' WHERE id = {m}"
        ),
    ] {
        assert!(sql(&b, &text).is_err(), "{text}");
    }
    assert_eq!(status(&b, m), "proposed");
}

#[test]
fn a_proposal_that_loses_a_member_is_deleted_whole() {
    // 3 threads: after the loss, 2 members of 2 threads stay, which would
    // be valid. The proposal goes all the same, so no decision covers a
    // part of what Iris proposed.
    let b = setup("lost", 3, &["record", "conversation", "commitment"]);
    let m = propose(&b, &[1, 2, 3]).unwrap();
    b.store
        .write(|tx| Ok(tx.execute("DELETE FROM item WHERE id = 2", [])?))
        .unwrap();
    assert!(matches!(decline(&b.store, m, 2), Err(Error::NotFound)));
    assert!(matches!(
        confirm(&b.store, m, "Trip", &[1], 2),
        Err(Error::NotFound)
    ));
    assert_eq!(
        read(&b, |c| c.query_row(
            "SELECT count(*) FROM matter_item",
            [],
            |r| r.get::<_, i64>(0)
        )),
        0
    );
    // The members are free again.
    propose(&b, &[1, 3]).unwrap();
}

#[test]
fn a_hidden_member_is_not_live() {
    let b = setup("hidden", 3, &["record", "conversation", "commitment"]);
    let m = propose(&b, &[1, 2, 3]).unwrap();
    sql(
        &b,
        r#"UPDATE item SET doc = '{"attention":{"state":"hidden"}}' WHERE id = 3"#,
    )
    .unwrap();
    // Still valid (2 live members of 2 threads), but item 3 is not live.
    assert!(matches!(
        confirm(&b.store, m, "Trip", &[1, 3], 2),
        Err(Error::Conflict(_))
    ));
    confirm(&b.store, m, "Trip", &[1, 2], 2).unwrap();
    // A gone member makes it invalid for a new proposal.
    let b = setup("gone", 3, &["record", "conversation", "commitment"]);
    let m = propose(&b, &[1, 2]).unwrap();
    sql(&b, "UPDATE item SET status = 'gone' WHERE id = 2").unwrap();
    assert!(matches!(
        confirm(&b.store, m, "Trip", &[1], 2),
        Err(Error::Conflict(_))
    ));
    // A decline of the same stale proposal is a conflict, not a store error.
    assert!(matches!(decline(&b.store, m, 2), Err(Error::Conflict(_))));
    assert_eq!(status(&b, m), "proposed");
}

/// Random proposals, decisions, item deletes and clean-ups. After each step:
/// no item is a kept member of two open matters, a decided matter has not
/// changed, and after `clean` each proposal is valid.
#[test]
fn random_steps_keep_the_invariants() {
    for seed in 0..20u64 {
        let forms = ["record", "conversation", "commitment", "feed"];
        let mut rng = Rng::new(seed);
        let all: Vec<&str> = (0..24).map(|_| *rng.pick(&forms)).collect();
        let b = setup(&format!("random-{seed}"), 6, &all);
        let mut decided: Vec<(i64, String, Option<String>)> = Vec::new();
        for step in 0..120 {
            match rng.below(5) {
                0 | 1 => {
                    let n = 1 + rng.below(4) as usize;
                    let items: Vec<i64> = (0..n).map(|_| 1 + rng.below(24) as i64).collect();
                    let _ = propose(&b, &items);
                }
                2 => {
                    let m = 1 + rng.below(40) as i64;
                    let keep = [1 + rng.below(24) as i64];
                    let r = if rng.chance(500) {
                        confirm(&b.store, m, "T", &keep, step)
                    } else {
                        decline(&b.store, m, step)
                    };
                    if r.is_ok() {
                        let row = read(&b, |c| {
                            c.query_row(
                                "SELECT status, (SELECT group_concat(item_id || ':' || kept)
                                   FROM matter_item WHERE matter_id = ?1 AND kept = 1) FROM matter WHERE id = ?1",
                                [m],
                                |r| Ok((r.get(0)?, r.get(1)?)),
                            )
                        });
                        decided.push((m, row.0, row.1));
                    }
                }
                3 => {
                    // Understanding deletes an item that is not pinned.
                    let i = 1 + rng.below(24) as i64;
                    let _ = sql(
                        &b,
                        &format!(
                            "DELETE FROM item WHERE id = {i} AND id NOT IN (SELECT item_id FROM item_pin)"
                        ),
                    );
                }
                _ => {
                    b.store.write(|tx| Ok(clean(tx))).unwrap().unwrap();
                    let bad: i64 = read(&b, |c| {
                        c.query_row(
                            "SELECT count(*) FROM matter WHERE status = 'proposed'
                               AND id NOT IN (SELECT matter_id FROM matter_valid)",
                            [],
                            |r| r.get(0),
                        )
                    });
                    assert_eq!(bad, 0, "seed {seed} step {step}");
                }
            }
            let twice: i64 = read(&b, |c| {
                c.query_row(
                    "SELECT count(*) FROM (SELECT mi.item_id FROM matter_item mi
                       JOIN matter m ON m.id = mi.matter_id
                       WHERE mi.kept = 1 AND m.status IN ('proposed', 'confirmed')
                       GROUP BY mi.item_id HAVING count(*) > 1)",
                    [],
                    |r| r.get(0),
                )
            });
            assert_eq!(twice, 0, "seed {seed} step {step}");
            for (m, s, members) in &decided {
                let now: (String, Option<String>) = read(&b, |c| {
                    c.query_row(
                        "SELECT status, (SELECT group_concat(item_id || ':' || kept)
                           FROM matter_item WHERE matter_id = ?1 AND kept = 1) FROM matter WHERE id = ?1",
                        [m],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                });
                assert_eq!(
                    (&now.0, &now.1),
                    (s, members),
                    "seed {seed} step {step} matter {m}"
                );
            }
        }
    }
}
