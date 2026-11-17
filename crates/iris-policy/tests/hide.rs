//! Tests of the `hide` proposals of card R5-02
//! (docs/specs/2026-11-17-preference-rules.md): Iris proposes a `hide` rule
//! for one sender and one form after 10 choices of the person (`skip`,
//! `mute`, `dismiss`) with no refusal, and the rule goes into the file only
//! after an accept.

use iris_policy::propose::{self, MIN_APPROVALS};
use iris_policy::*;
use iris_store::Store;
use rusqlite::params;
use std::path::PathBuf;

const NOW: i64 = 1_800_000_000;

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A store with one account and one thread. The pass reads only the
/// action rows, so the test drops the triggers of `changeset_action` and
/// writes `applied` and `undone` rows with no ledger.
fn store(name: &str) -> (Dir, Store) {
    let dir = std::env::temp_dir().join(format!("iris-hide-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO account(id, provider, address) VALUES (1, 'gmail', 'me@x.example');
                 INSERT INTO thread(account_id, provider_id) VALUES (1, 't');
                 INSERT INTO label(id, account_id, provider_id, name, kind)
                   VALUES (1, 1, 'SENT', 'SENT', 'system');
                 INSERT INTO sender(address, kind) VALUES ('news@shop.example', 'service');
                 INSERT INTO sender(address, kind) VALUES ('ann@friends.example', 'person');",
            )?;
            let names: Vec<String> = tx
                .prepare(
                    "SELECT name FROM sqlite_schema WHERE type = 'trigger'
                     AND tbl_name IN ('changeset_action', 'changeset', 'action_item')",
                )?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for n in names {
                tx.execute_batch(&format!("DROP TRIGGER \"{n}\""))?;
            }
            Ok(())
        })
        .unwrap();
    (Dir(dir), store)
}

/// A new item of `form` whose origin message is from `from`. Gives the
/// item row ID.
fn item(store: &Store, form: &str, from: &str, hostile: bool, sent: bool) -> i64 {
    store
        .write(|tx| {
            let n: i64 = tx.query_row("SELECT count(*) FROM message", [], |r| r.get(0))?;
            tx.execute(
                "INSERT INTO message(account_id, thread_id, provider_id, internal_date, hostile)
                 VALUES (1, 1, ?1, ?2, ?3)",
                params![
                    format!("m{n}"),
                    NOW * 1000,
                    hostile.then_some("[\"phishing\"]")
                ],
            )?;
            let m = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO header(message_id, from_addr) VALUES (?1, ?2)",
                params![m, from],
            )?;
            if sent {
                tx.execute(
                    "INSERT INTO message_label(message_id, label_id) VALUES (?1, 1)",
                    [m],
                )?;
            }
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES (?1, 1, 'gone', '{}', 0, 0)",
                [form],
            )?;
            let i = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
                params![i, m],
            )?;
            Ok(i)
        })
        .unwrap()
}

/// A choice of the person: an action of `verb` on item `i`, `applied` or
/// `undone`.
fn chose(store: &Store, verb: &str, i: i64, state: &str) {
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO changeset(author_kind, created_at, why, submitted_at)
                 VALUES ('person', 0, 'w', 1)",
                [],
            )?;
            let cs = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args,
                   hash, state, decided_by, decided_at)
                 VALUES (?1, 1, 1, ?2, '{}', '{}', 'h', ?3, 'person', 2)",
                params![cs, verb, state],
            )?;
            let a = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO action_item(action_id, item_id) VALUES (?1, ?2)",
                params![a, i],
            )?;
            Ok(())
        })
        .unwrap();
}

/// `n` applied choices of `verb` on new items of `form` from `from`.
fn choices(store: &Store, verb: &str, form: &str, from: &str, n: i64) {
    for _ in 0..n {
        let i = item(store, form, from, false, false);
        chose(store, verb, i, "applied");
    }
}

fn refresh(store: &Store) -> Vec<propose::Proposal> {
    let mut mark = None;
    propose::refresh(store, NOW, &mut mark).unwrap();
    store.read(propose::open).unwrap()
}

fn hides(ps: &[propose::Proposal]) -> Vec<(String, Form, i64)> {
    ps.iter()
        .filter(|p| p.verb.is_none())
        .map(|p| {
            (
                p.when.sender.clone().unwrap(),
                p.when.form.unwrap(),
                p.approvals,
            )
        })
        .collect()
}

#[test]
fn ten_choices_give_a_hide_proposal_and_only_an_accept_writes_the_file() {
    let (dir, store) = store("accept");
    choices(
        &store,
        "skip",
        "feed",
        "News@Shop.example",
        MIN_APPROVALS - 1,
    );
    assert!(hides(&refresh(&store)).is_empty(), "one choice less");
    choices(&store, "skip", "feed", "news@shop.example", 1);
    // Two passes with the same choices keep one open row.
    refresh(&store);
    let ps = refresh(&store);
    assert_eq!(
        hides(&ps),
        vec![("news@shop.example".into(), Form::Feed, MIN_APPROVALS)]
    );
    let rows: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM rule_proposal", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(rows, 1);
    assert!(!dir.0.join(FILE).exists(), "a proposal is a row");

    let p = &ps[0];
    assert_eq!(
        p.rule(1, "2027-01-15").name,
        "Hide feed items from news@shop.example"
    );
    let edited = change(&store, None, Change::Accept(p.id), NOW).unwrap();
    let Edited::Done { policy, .. } = edited else {
        panic!("accept: {edited:?}")
    };
    let text = std::fs::read_to_string(dir.0.join(FILE)).unwrap();
    assert!(text.contains("effect = \"hide\""), "{text}");
    assert!(text.contains("by = \"proposal\""), "{text}");
    assert!(text.contains("you hid 10 items"), "{text}");
    let rule = &policy.rules[0];
    assert_eq!((rule.effect, rule.by), (Effect::Hide, By::Proposal));
    assert!(rule.verbs.is_empty());
    assert_eq!(rule.when.sender.as_deref(), Some("news@shop.example"));
    assert_eq!(rule.when.form, Some(Form::Feed));
    let state: String = store
        .read(|c| Ok(c.query_row("SELECT state FROM rule_proposal", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(state, "accepted");
    // The rule hides these items now: no new proposal, and a second accept
    // finds nothing.
    assert!(hides(&refresh(&store)).is_empty());
    let v = store.read(active).unwrap().version;
    assert_eq!(
        change(&store, v, Change::Accept(p.id), NOW).unwrap(),
        Edited::NoRule
    );
}

#[test]
fn each_form_has_its_own_proposal() {
    let (_dir, store) = store("forms");
    choices(
        &store,
        "mute",
        "conversation",
        "ann@friends.example",
        MIN_APPROVALS,
    );
    choices(
        &store,
        "dismiss",
        "signal",
        "news@shop.example",
        MIN_APPROVALS,
    );
    choices(
        &store,
        "skip",
        "feed",
        "news@shop.example",
        MIN_APPROVALS - 1,
    );
    let mut got = hides(&refresh(&store));
    got.sort_by_key(|h| h.0.clone());
    assert_eq!(
        got,
        vec![
            (
                "ann@friends.example".into(),
                Form::Conversation,
                MIN_APPROVALS
            ),
            ("news@shop.example".into(), Form::Signal, MIN_APPROVALS),
        ]
    );
}

#[test]
fn one_refusal_stops_the_proposal() {
    let (_dir, store) = store("refusal");
    choices(
        &store,
        "skip",
        "feed",
        "news@shop.example",
        MIN_APPROVALS + 3,
    );
    assert_eq!(hides(&refresh(&store)).len(), 1);
    let i = item(&store, "feed", "news@shop.example", false, false);
    chose(&store, "skip", i, "undone");
    assert!(hides(&refresh(&store)).is_empty(), "the open row goes");
}

/// A refusal that the engine cannot read again (the cleanup deleted the
/// header of its origin) stops each `hide` proposal of its form, and only
/// of its form.
#[test]
fn an_unreadable_refusal_stops_its_form() {
    let (_dir, store) = store("unreadable");
    choices(&store, "skip", "feed", "news@shop.example", MIN_APPROVALS);
    choices(
        &store,
        "dismiss",
        "signal",
        "news@shop.example",
        MIN_APPROVALS,
    );
    let i = item(&store, "feed", "ann@friends.example", false, false);
    chose(&store, "skip", i, "undone");
    store
        .write(|tx| {
            Ok(tx.execute(
                "DELETE FROM header WHERE message_id =
                   (SELECT message_id FROM item_source WHERE item_id = ?1)",
                [i],
            )?)
        })
        .unwrap();
    assert_eq!(
        hides(&refresh(&store)),
        vec![("news@shop.example".into(), Form::Signal, MIN_APPROVALS)]
    );
}

#[test]
fn hostile_mail_and_mail_from_you_give_no_proposal() {
    let (_dir, store) = store("hostile");
    for _ in 0..MIN_APPROVALS {
        let i = item(&store, "feed", "news@shop.example", true, false);
        chose(&store, "skip", i, "applied");
        // A conversation that the person started: its origin is the
        // person's own sent message.
        let i = item(&store, "conversation", "me@x.example", false, true);
        chose(&store, "mute", i, "applied");
    }
    assert!(hides(&refresh(&store)).is_empty());
}

/// A choice on an item that a rule of the file hides or bundles already
/// does not count: the proposal counts only what its rule changes.
#[test]
fn an_item_that_a_rule_hides_already_does_not_count() {
    let (dir, store) = store("covered");
    std::fs::write(
        dir.0.join(FILE),
        "version = 0\nmode = \"observe\"\n\n[[rule]]\nid = 1\nname = \"Bundle the shop\"\n\
         effect = \"bundle\"\nwhen = { sender = \"news@shop.example\" }\nby = \"person\"\n\
         added = 2027-01-01\n",
    )
    .unwrap();
    let state = file_state(&store).unwrap();
    let FileState::Changed(loaded) = state else {
        panic!("the file is not new: {state:?}")
    };
    confirm(&store, &loaded.sha256, NOW).unwrap();
    choices(
        &store,
        "skip",
        "feed",
        "news@shop.example",
        MIN_APPROVALS + 5,
    );
    assert!(hides(&refresh(&store)).is_empty());
}
