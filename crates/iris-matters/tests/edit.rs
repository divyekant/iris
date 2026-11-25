//! The writes of the person on a confirmed matter, the matter by hand, the
//! agent proposal, and the store rules of migration 46
//! (`docs/specs/2026-11-24-matters-edit.md`).

use iris_matters::{
    confirm, create, decline, delete, edit, insert_proposal, propose_by_agent, write, Edit, Error,
};
use iris_store::{NewMessage, Store};
use rusqlite::params;

struct Box {
    store: Store,
    dir: std::path::PathBuf,
}

impl Drop for Box {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Item `i` (row ID `i + 1`) has the form `forms[i]`, its origin in thread
/// `i % threads`, and the status `active` for a signal, else `open`.
fn setup(name: &str, threads: usize, forms: &[&str]) -> Box {
    let dir = std::env::temp_dir().join(format!("iris-medit-{name}-{}", std::process::id()));
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
                let status = if *form == "signal" { "active" } else { "open" };
                tx.execute(
                    "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                     VALUES (?1, ?2, ?3, '{}', 0, 0)",
                    params![form, a, status],
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

/// rows 1..=7: record, conversation, commitment, record, feed, signal, record.
const FORMS: [&str; 7] = [
    "record",
    "conversation",
    "commitment",
    "record",
    "feed",
    "signal",
    "record",
];

fn r(row: i64) -> (i64, &'static str) {
    (row, FORMS[(row - 1) as usize])
}

fn q<T: rusqlite::types::FromSql>(b: &Box, text: &str) -> T {
    b.store
        .read(|c| Ok(c.query_row(text, [], |r| r.get(0))?))
        .unwrap()
}

fn sql(b: &Box, text: &str) -> rusqlite::Result<usize> {
    b.store.write(|tx| Ok(tx.execute(text, []))).unwrap()
}

fn kept(b: &Box, m: i64) -> Vec<i64> {
    b.store
        .read(|c| {
            let mut s = c.prepare(
                "SELECT item_id FROM matter_item WHERE matter_id = ?1 AND kept = 1 ORDER BY 1",
            )?;
            let v = s
                .query_map([m], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(v)
        })
        .unwrap()
}

fn confirmed(b: &Box, items: &[i64]) -> i64 {
    let m = write(&b.store, |tx| {
        insert_proposal(tx, "Trip", "fake", None, items, 1)
    })
    .unwrap();
    confirm(&b.store, m, "Trip", items, 2).unwrap();
    m
}

#[test]
fn a_matter_by_hand_is_confirmed_at_once() {
    let b = setup("create", 4, &FORMS);
    let m = create(&b.store, "Move", &[r(1)], 5).unwrap();
    assert_eq!(
        q::<String>(
            &b,
            &format!("SELECT status || author FROM matter WHERE id = {m}")
        ),
        "confirmedperson"
    );
    assert_eq!(kept(&b, m), vec![1]);
    // A signal is a member; a feed is not; an item of an open matter is not.
    create(&b.store, "Code", &[r(6)], 5).unwrap();
    assert!(matches!(
        create(&b.store, "Feed", &[r(5)], 5),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        create(&b.store, "Again", &[r(1)], 5),
        Err(Error::Conflict(_))
    ));
    // The ID names the form: `c-1` of the record 1 is no item.
    assert!(matches!(
        create(&b.store, "Form", &[(2, "record")], 5),
        Err(Error::Conflict(_))
    ));
    for bad in [vec![], vec![r(2), r(2)]] {
        assert!(matches!(create(&b.store, "x", &bad, 5), Err(Error::Bad(_))));
    }
    assert!(matches!(
        create(&b.store, " ", &[r(2)], 5),
        Err(Error::Bad(_))
    ));
}

#[test]
fn the_person_renames_adds_and_removes() {
    let b = setup("edit", 4, &FORMS);
    let m = confirmed(&b, &[1, 2]);
    let e = Edit {
        title: Some("Tokyo".into()),
        add: vec![r(3), r(6)],
        remove: vec![r(1)],
    };
    edit(&b.store, m, &e, 9).unwrap();
    assert_eq!(kept(&b, m), vec![2, 3, 6]);
    assert_eq!(
        q::<String>(
            &b,
            &format!("SELECT title || updated_at FROM matter WHERE id = {m}")
        ),
        "Tokyo9"
    );
    // A removed item can come back, and is a candidate meanwhile.
    edit(
        &b.store,
        m,
        &Edit {
            add: vec![r(1)],
            ..Default::default()
        },
        10,
    )
    .unwrap();
    assert_eq!(kept(&b, m), vec![1, 2, 3, 6]);
    // All removed but one added: the count stays 1 or more at each step.
    let e = Edit {
        add: vec![r(4)],
        remove: vec![r(1), r(2), r(3), r(6)],
        ..Default::default()
    };
    edit(&b.store, m, &e, 11).unwrap();
    assert_eq!(kept(&b, m), vec![4]);
    // The last member does not go.
    let last = Edit {
        remove: vec![r(4)],
        ..Default::default()
    };
    assert!(matches!(
        edit(&b.store, m, &last, 12),
        Err(Error::Conflict(_))
    ));
    // An item that is not a member, a feed, an empty edit.
    let not = Edit {
        remove: vec![r(7)],
        ..Default::default()
    };
    assert!(matches!(
        edit(&b.store, m, &not, 12),
        Err(Error::Conflict(_))
    ));
    let feed = Edit {
        add: vec![r(5)],
        ..Default::default()
    };
    assert!(matches!(
        edit(&b.store, m, &feed, 12),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        edit(&b.store, m, &Edit::default(), 12),
        Err(Error::Bad(_))
    ));
    let both = Edit {
        add: vec![r(7)],
        remove: vec![r(7)],
        ..Default::default()
    };
    assert!(matches!(edit(&b.store, m, &both, 12), Err(Error::Bad(_))));
    // A kept member of a proposal is not added by hand.
    let p = write(&b.store, |tx| {
        insert_proposal(tx, "P", "fake", None, &[1, 2], 1)
    })
    .unwrap();
    let taken = Edit {
        add: vec![r(1)],
        ..Default::default()
    };
    assert!(matches!(
        edit(&b.store, m, &taken, 12),
        Err(Error::Conflict(_))
    ));
    // A proposal is not edited.
    let rename = Edit {
        title: Some("x".into()),
        ..Default::default()
    };
    assert!(matches!(
        edit(&b.store, p, &rename, 12),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        edit(&b.store, 99, &rename, 12),
        Err(Error::NotFound)
    ));
}

#[test]
fn an_edit_keeps_50_members_at_most() {
    let forms = vec!["record"; 52];
    let b = setup("fifty", 2, &forms);
    let all: Vec<i64> = (1..=50).collect();
    let m = create(
        &b.store,
        "Big",
        &all.iter().map(|i| (*i, "record")).collect::<Vec<_>>(),
        1,
    )
    .unwrap();
    let more = Edit {
        add: vec![(51, "record")],
        ..Default::default()
    };
    assert!(matches!(
        edit(&b.store, m, &more, 2),
        Err(Error::Conflict(_))
    ));
    // A swap at 50 works: one in, one out.
    let swap = Edit {
        add: vec![(51, "record"), (52, "record")],
        remove: vec![(1, "record"), (2, "record")],
        ..Default::default()
    };
    edit(&b.store, m, &swap, 2).unwrap();
    assert_eq!(kept(&b, m).len(), 50);
}

#[test]
fn a_delete_remembers_the_grouping() {
    let b = setup("delete", 4, &FORMS);
    let m = confirmed(&b, &[1, 2, 3]);
    delete(&b.store, m, 7).unwrap();
    assert_eq!(
        q::<String>(&b, &format!("SELECT status FROM matter WHERE id = {m}")),
        "declined"
    );
    assert_eq!(
        q::<i64>(
            &b,
            "SELECT count(*) FROM item_pin WHERE item_id IN (1, 2, 3)"
        ),
        3
    );
    assert!(matches!(delete(&b.store, m, 8), Err(Error::Conflict(_))));
    assert!(matches!(delete(&b.store, 99, 8), Err(Error::NotFound)));
    // A matter of one item goes as a row.
    // Invariant: a declined matter has 2 member rows or more, for each
    // writer. A direct SQL decline of a matter with one member is refused.
    let one = create(&b.store, "One", &[r(4)], 1).unwrap();
    assert!(sql(
        &b,
        &format!(
            "UPDATE matter SET status = 'declined', keep = NULL, decided_at = 9, updated_at = 9
             WHERE id = {one}"
        )
    )
    .is_err());
    delete(&b.store, one, 8).unwrap();
    assert_eq!(
        q::<i64>(&b, &format!("SELECT count(*) FROM matter WHERE id = {one}")),
        0
    );
    assert_eq!(
        q::<i64>(&b, "SELECT count(*) FROM matter_item WHERE item_id = 4"),
        0
    );
}

#[test]
fn an_addition_waits_for_the_person() {
    let b = setup("add", 4, &FORMS);
    let m = confirmed(&b, &[1, 2]);
    let add = |item: i64| {
        sql(
            &b,
            &format!(
                "INSERT INTO matter_add(item_id, matter_id, created_at) VALUES ({item}, {m}, 1)"
            ),
        )
    };
    add(6).unwrap();
    add(3).unwrap();
    assert_eq!(q::<i64>(&b, "SELECT count(*) FROM matter_add_live"), 2);
    // Add: the item is a member, the addition is gone.
    edit(
        &b.store,
        m,
        &Edit {
            add: vec![r(6)],
            ..Default::default()
        },
        3,
    )
    .unwrap();
    // Not this: a refusal row, the addition is gone.
    edit(
        &b.store,
        m,
        &Edit {
            remove: vec![r(3)],
            ..Default::default()
        },
        3,
    )
    .unwrap();
    assert_eq!(q::<i64>(&b, "SELECT count(*) FROM matter_add"), 0);
    assert_eq!(kept(&b, m), vec![1, 2, 6]);
    assert_eq!(
        q::<i64>(
            &b,
            &format!("SELECT kept FROM matter_item WHERE matter_id = {m} AND item_id = 3")
        ),
        0
    );
    // A refused item, a member, a feed, an item of an open matter: no addition.
    for item in [3, 1, 5] {
        assert!(add(item).is_err(), "{item}");
    }
    // A signal that is not active is not a live addition.
    add(7).unwrap();
    sql(&b, "UPDATE item SET status = 'expired' WHERE id = 6").unwrap();
    assert_eq!(q::<i64>(&b, "SELECT count(*) FROM matter_add_live"), 1);
    // A delete ends each addition of the matter.
    delete(&b.store, m, 4).unwrap();
    assert_eq!(q::<i64>(&b, "SELECT count(*) FROM matter_add"), 0);
    // An addition goes only to a confirmed matter.
    assert!(add(4).is_err());
}

/// A signal past its `expires` time is not live, also while the stored
/// status is still `active` (issue #927): no matter by hand, no addition,
/// no agent proposal.
#[test]
fn a_signal_past_its_time_is_not_live() {
    let b = setup("expired", 4, &FORMS);
    sql(
        &b,
        "UPDATE item SET doc = json_set(doc, '$.expires', '1970-01-01T00:00:01Z') WHERE id = 6",
    )
    .unwrap();
    // Each refusal rolls back, so each path must expire the signal itself.
    let m = create(&b.store, "T", &[r(1), r(2)], 999).unwrap();
    assert!(matches!(
        create(&b.store, "T", &[r(4), r(6)], 1_000),
        Err(Error::Conflict(_))
    ));
    let e = Edit {
        add: vec![r(6)],
        ..Default::default()
    };
    assert!(matches!(
        edit(&b.store, m, &e, 1_000),
        Err(Error::Conflict(_))
    ));
    let p = write(&b.store, |tx| {
        propose_by_agent(tx, "U", &[r(3), r(6)], 1_000)
    });
    assert!(matches!(p, Err(Error::Bad(_))), "{p:?}");
}

/// A proposed signal that passes its `expires` time before the confirm,
/// with no engine tick, makes the proposal not valid for each decision
/// (issue #985).
#[test]
fn a_signal_that_expires_before_the_confirm_is_not_confirmed() {
    let b = setup("expired-confirm", 4, &FORMS);
    let p = write(&b.store, |tx| {
        insert_proposal(tx, "P", "fake", None, &[3, 6], 1)
    })
    .unwrap();
    sql(
        &b,
        "UPDATE item SET doc = json_set(doc, '$.expires', '1970-01-01T00:00:01Z') WHERE id = 6",
    )
    .unwrap();
    assert!(matches!(
        confirm(&b.store, p, "P", &[3, 6], 1_000),
        Err(Error::Conflict(_))
    ));
    // The refusal rolled back the expiry; a decline must expire it too.
    assert!(matches!(
        decline(&b.store, p, 1_000),
        Err(Error::Conflict(_))
    ));
}

#[test]
fn an_agent_proposes_and_the_person_decides() {
    let b = setup("agent", 4, &FORMS);
    let m = write(&b.store, |tx| {
        propose_by_agent(tx, "Tokyo", &[r(1), r(2), r(6)], 1)
    })
    .unwrap();
    assert_eq!(
        q::<String>(
            &b,
            &format!("SELECT status || author || sealed FROM matter WHERE id = {m}")
        ),
        "proposedagent1"
    );
    assert_eq!(
        q::<i64>(
            &b,
            &format!("SELECT count(*) FROM matter_valid WHERE matter_id = {m}")
        ),
        1
    );
    let refused = |items: &[(i64, &str)]| {
        matches!(
            write(&b.store, |tx| propose_by_agent(tx, "Trip", items, 1)),
            Err(Error::Bad(_))
        )
    };
    // One item, one thread (rows 3 and 7), a feed, an open member, a wrong form.
    assert!(refused(&[r(3)]));
    assert!(refused(&[r(3), r(7)]));
    assert!(refused(&[r(3), r(5)]));
    assert!(refused(&[r(3), r(1)]));
    assert!(refused(&[r(3), (4, "commitment")]));
    // A subset of a declined matter comes not back.
    decline(&b.store, m, 2).unwrap();
    assert!(refused(&[r(1), r(2)]));
    write(&b.store, |tx| propose_by_agent(tx, "New", &[r(3), r(4)], 3)).unwrap();
}

#[test]
fn the_triggers_hold_the_rules_of_migration_46() {
    let b = setup("rules46", 4, &FORMS);
    let m = confirmed(&b, &[1, 2]);
    // The title changes; the rest of a confirmed matter does not.
    sql(
        &b,
        &format!("UPDATE matter SET title = 'New' WHERE id = {m}"),
    )
    .unwrap();
    for text in [
        format!("UPDATE matter SET author = 'person' WHERE id = {m}"),
        format!(
            "UPDATE matter SET status = 'proposed', decided_at = NULL, keep = NULL WHERE id = {m}"
        ),
        format!("UPDATE matter SET keep = '[1]' WHERE id = {m}"),
        format!("UPDATE matter SET status = 'declined', keep = '[1,2]' WHERE id = {m}"),
        format!("UPDATE matter_item SET kept = 0 WHERE matter_id = {m}"),
        format!("INSERT INTO matter_item(matter_id, item_id) VALUES ({m}, 5)"),
        format!("DELETE FROM matter WHERE id = {m}"),
    ] {
        assert!(sql(&b, &text).is_err(), "{text}");
    }
    // A delete names its own time: not the time of the confirm.
    assert!(sql(
        &b,
        &format!("UPDATE matter SET status = 'declined', keep = NULL WHERE id = {m}")
    )
    .is_err());
    sql(
        &b,
        &format!(
            "UPDATE matter SET status = 'declined', keep = NULL, decided_at = 5, updated_at = 5
             WHERE id = {m}"
        ),
    )
    .unwrap();
    for text in [
        format!("UPDATE matter SET title = 'x' WHERE id = {m}"),
        format!("INSERT INTO matter_item(matter_id, item_id, kept) VALUES ({m}, 3, 0)"),
        format!("UPDATE matter_item SET kept = 0 WHERE matter_id = {m} AND item_id = 1"),
        format!("DELETE FROM matter WHERE id = {m}"),
    ] {
        assert!(sql(&b, &text).is_err(), "{text}");
    }
    // A matter by hand is confirmed only, never declined at its decision.
    let p = b
        .store
        .write(|tx| {
            tx.execute(
                "INSERT INTO matter(title, status, author, model, prompt, created_at, updated_at)
                 VALUES ('x', 'proposed', 'person', '', '', 0, 0)",
                [],
            )?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO matter_item(matter_id, item_id) VALUES (?1, 3)",
                [id],
            )?;
            tx.execute("UPDATE matter SET sealed = 1 WHERE id = ?1", [id])?;
            Ok(id)
        })
        .unwrap();
    assert!(sql(
        &b,
        &format!("UPDATE matter SET status = 'declined', decided_at = 1 WHERE id = {p}")
    )
    .is_err());
}

/// The author names the model fields: a matter of the model has them, a
/// matter of an agent or the person has none.
#[test]
fn the_author_and_the_model_fields_agree() {
    let b = setup("author", 4, &FORMS);
    for (author, model, prompt) in [
        ("person", "m", "p"),
        ("agent", "m", ""),
        ("model", "", "p"),
        ("model", "m", ""),
    ] {
        let text = format!(
            "INSERT INTO matter(title, status, author, model, prompt, created_at, updated_at)
             VALUES ('x', 'proposed', '{author}', '{model}', '{prompt}', 0, 0)"
        );
        assert!(sql(&b, &text).is_err(), "{text}");
    }
}

/// No matter has more than 50 kept members: a proposal of 51 cannot be
/// made, so it cannot be confirmed.
#[test]
fn a_proposal_holds_50_members_at_most() {
    let forms = vec!["record"; 51];
    let b = setup("fifty-one", 2, &forms);
    let all: Vec<i64> = (1..=51).collect();
    assert!(write(&b.store, |tx| insert_proposal(
        tx, "Big", "fake", None, &all, 1
    ))
    .is_err());
    let m = write(&b.store, |tx| {
        insert_proposal(tx, "Big", "fake", None, &all[..50], 1)
    })
    .unwrap();
    // A direct confirm with a `keep` of 51 is refused too.
    let keep = serde_json::to_string(&all).unwrap();
    assert!(sql(
        &b,
        &format!(
            "UPDATE matter SET status = 'confirmed', keep = '{keep}', decided_at = 2,
             updated_at = 2 WHERE id = {m}"
        )
    )
    .is_err());
}

/// Step 0 deletes an addition whose item is not live, and the item is a
/// candidate again.
#[test]
fn clean_ends_an_addition_that_is_not_live() {
    let b = setup("clean-add", 4, &FORMS);
    let m = confirmed(&b, &[1, 2]);
    sql(
        &b,
        &format!("INSERT INTO matter_add(item_id, matter_id, created_at) VALUES (3, {m}, 1)"),
    )
    .unwrap();
    sql(
        &b,
        r#"UPDATE item SET doc = '{"attention":{"state":"hidden"}}' WHERE id = 3"#,
    )
    .unwrap();
    write(&b.store, iris_matters::clean).unwrap();
    assert_eq!(q::<i64>(&b, "SELECT count(*) FROM matter_add"), 0);
    sql(
        &b,
        "UPDATE item SET doc = json_set(doc, '$.attention.state', 'shown') WHERE id = 3",
    )
    .unwrap();
    // Free again: an agent can propose it.
    write(&b.store, |tx| propose_by_agent(tx, "New", &[r(3), r(4)], 3)).unwrap();
}
