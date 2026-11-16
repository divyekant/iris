//! Tests of spec 2 section 6.6: Iris proposes a rule after a pattern of
//! approvals, and the rule goes into the file only after an accept.

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

fn store(name: &str) -> (Dir, Store) {
    let dir = std::env::temp_dir().join(format!("iris-propose-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO account(id, provider, address) VALUES (1, 'gmail', 'me@x.example');
                 INSERT INTO thread(account_id, provider_id) VALUES (1, 't');
                 INSERT INTO sender(address, kind) VALUES ('deals@shop.example', 'service');
                 INSERT INTO sender(address, kind) VALUES ('news@shop.example', 'service');
                 INSERT INTO sender(address, kind) VALUES ('ann@friends.example', 'person');",
            )?;
            Ok(())
        })
        .unwrap();
    (Dir(dir), store)
}

/// A new message from `from`. Gives its row ID.
fn message(store: &Store, from: &str, hostile: bool) -> i64 {
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
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO header(message_id, from_addr) VALUES (?1, ?2)",
                params![id, from],
            )?;
            Ok(id)
        })
        .unwrap()
}

/// An action of `verb` on `messages` by an agent, that the person decided:
/// `approved` or `denied`.
fn decided(store: &Store, verb: &str, messages: &[i64], state: &str) {
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0)",
                [],
            )?;
            let cs = tx.last_insert_rowid();
            let ids: Vec<String> = messages.iter().map(|m| format!("\"m-{m}\"")).collect();
            tx.execute(
                "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, hash)
                 VALUES (?1, 1, 1, ?2, ?3, '{}', 'h')",
                params![cs, verb, format!("{{\"messages\":[{}]}}", ids.join(","))],
            )?;
            let a = tx.last_insert_rowid();
            for m in messages {
                tx.execute(
                    "INSERT INTO action_message(action_id, message_id) VALUES (?1, ?2)",
                    params![a, m],
                )?;
            }
            tx.execute(
                "UPDATE changeset SET why = 'w', submitted_at = 1 WHERE id = ?1",
                [cs],
            )?;
            let reason = (state == "denied").then_some("no");
            tx.execute(
                "UPDATE changeset_action SET state = ?2, decided_by = 'person', reason = ?3,
                 decided_at = 1 + ?1 WHERE id = ?1",
                params![a, state, reason],
            )?;
            Ok(())
        })
        .unwrap();
}

fn refresh(store: &Store) -> Vec<propose::Proposal> {
    let mut mark = None;
    propose::refresh(store, NOW, &mut mark).unwrap();
    store.read(propose::open).unwrap()
}

fn find<'a>(ps: &'a [propose::Proposal], sender: &str) -> Option<&'a propose::Proposal> {
    ps.iter().find(|p| p.when.sender.as_deref() == Some(sender))
}

#[test]
fn a_pattern_gives_a_proposal_and_only_an_accept_writes_the_file() {
    let (dir, store) = store("accept");
    for _ in 0..MIN_APPROVALS - 1 {
        let m = message(&store, "Deals@Shop.example", false);
        decided(&store, "archive", &[m], "approved");
    }
    assert!(
        refresh(&store).is_empty(),
        "one approval less than the minimum"
    );

    let m = message(&store, "deals@shop.example", false);
    decided(&store, "archive", &[m], "approved");
    let ps = refresh(&store);
    let p = find(&ps, "deals@shop.example").expect("a sender proposal");
    assert_eq!(p.verb, Verb::Archive);
    assert_eq!(p.approvals, MIN_APPROVALS);
    let kind = ps
        .iter()
        .find(|p| p.when.sender_kind == Some(SenderKind::Service));
    assert!(kind.is_some(), "a sender kind proposal");
    // A proposal is a row, never a line in the file.
    assert!(!dir.0.join(FILE).exists());
    assert_eq!(store.read(active).unwrap().version, None);

    let id = p.id;
    let edited = change(&store, None, Change::Accept(id), NOW).unwrap();
    let Edited::Done { policy, .. } = edited else {
        panic!("accept: {edited:?}")
    };
    let text = std::fs::read_to_string(dir.0.join(FILE)).unwrap();
    assert_eq!(text.matches("[[rule]]").count(), 1, "{text}");
    assert!(text.contains("by = \"proposal\""), "{text}");
    assert!(text.contains("added = 2027-01-15"), "{text}");
    let rule = &policy.rules[0];
    assert_eq!(
        (rule.id, rule.by, rule.effect),
        (1, By::Proposal, Effect::Allow)
    );
    assert_eq!(rule.when.sender.as_deref(), Some("deals@shop.example"));
    assert_eq!(rule.name, "Archive mail from deals@shop.example");

    // The rule now approves these actions, so the proposal does not come
    // back, and a second accept finds nothing.
    let ps = refresh(&store);
    assert!(find(&ps, "deals@shop.example").is_none());
    let v = store.read(active).unwrap().version;
    assert_eq!(
        change(&store, v, Change::Accept(id), NOW).unwrap(),
        Edited::NoRule
    );
}

#[test]
fn one_refusal_stops_the_proposal() {
    let (_dir, store) = store("refusal");
    for _ in 0..MIN_APPROVALS + 5 {
        let m = message(&store, "deals@shop.example", false);
        decided(&store, "archive", &[m], "approved");
    }
    assert!(find(&refresh(&store), "deals@shop.example").is_some());
    let m = message(&store, "deals@shop.example", false);
    decided(&store, "archive", &[m], "denied");
    let ps = refresh(&store);
    assert!(
        find(&ps, "deals@shop.example").is_none(),
        "the open row goes"
    );
    assert!(ps.is_empty(), "the service kind has the refusal too");
}

#[test]
fn a_dismissed_proposal_does_not_come_back() {
    let (_dir, store) = store("dismiss");
    for _ in 0..MIN_APPROVALS {
        let m = message(&store, "ann@friends.example", false);
        decided(&store, "read", &[m], "approved");
    }
    let ps = refresh(&store);
    let p = find(&ps, "ann@friends.example").unwrap();
    assert!(propose::dismiss(&store, p.id, NOW).unwrap());
    assert!(!propose::dismiss(&store, p.id, NOW).unwrap());
    for _ in 0..3 {
        let m = message(&store, "ann@friends.example", false);
        decided(&store, "read", &[m], "approved");
    }
    assert!(find(&refresh(&store), "ann@friends.example").is_none());
}

#[test]
fn hostile_mail_and_mixed_senders_give_no_proposal() {
    let (_dir, store) = store("hostile");
    for _ in 0..MIN_APPROVALS {
        let m = message(&store, "deals@shop.example", true);
        decided(&store, "archive", &[m], "approved");
        let a = message(&store, "deals@shop.example", false);
        let b = message(&store, "ann@friends.example", false);
        decided(&store, "trash", &[a, b], "approved");
    }
    let ps = refresh(&store);
    assert!(
        ps.iter()
            .all(|p| p.when.sender_kind != Some(SenderKind::Hostile)),
        "{ps:?}"
    );
    // A hostile mark matches only `hostile`, so no sender rule matches it.
    assert!(ps.iter().all(|p| p.verb != Verb::Archive), "{ps:?}");
    // No one sender or kind is in each trash action.
    assert!(ps.iter().all(|p| p.verb != Verb::Trash), "{ps:?}");
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// The invariant: each open proposal is a rule that, added to the active
/// policy, approves no action that the person refused, and changes the
/// decision of at least the minimum of approved actions.
#[test]
fn each_proposal_agrees_with_the_decisions() {
    const FROM: [&str; 4] = [
        "deals@shop.example",
        "news@shop.example",
        "ann@friends.example",
        "who@else.example",
    ];
    const VERBS: [&str; 3] = ["archive", "read", "trash"];
    let mut proposed = 0;
    for seed in 1..6u64 {
        let (_dir, store) = store(&format!("prop{seed}"));
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let mut all = Vec::new();
        for _ in 0..200 {
            let verb = VERBS[(rng.next() % 3) as usize];
            let n = 1 + rng.next() % 2;
            let msgs: Vec<i64> = (0..n)
                .map(|_| {
                    message(
                        &store,
                        FROM[(rng.next() % 4) as usize],
                        rng.next().is_multiple_of(9),
                    )
                })
                .collect();
            let state = if rng.next().is_multiple_of(25) {
                "denied"
            } else {
                "approved"
            };
            decided(&store, verb, &msgs, state);
            all.push((verb, msgs, state));
        }
        let ps = refresh(&store);
        proposed += ps.len();
        let facts = |m: i64| store.read(|c| facts(c, m, NOW)).unwrap().unwrap();
        for p in &ps {
            let mut policy = Policy {
                mode: Mode::Apply,
                rules: Vec::new(),
            };
            let mut yes = 0;
            policy.rules.push(p.rule(1, "2027-01-15"));
            for (verb, msgs, state) in &all {
                if Verb::from_name(verb) != Some(p.verb) {
                    continue;
                }
                let fs: Vec<Facts> = msgs.iter().map(|&m| facts(m)).collect();
                let action = Action {
                    verb: p.verb,
                    author: Author::Agent,
                    job: None,
                    bulk: false,
                    now: NOW,
                };
                let d = policy.decide(&action, fs.iter().map(Facts::message));
                if d.outcome == Outcome::Allow {
                    assert_eq!(*state, "approved", "seed {seed}: {p:?} allows a refusal");
                    yes += 1;
                }
            }
            assert_eq!(yes, p.approvals, "seed {seed}: {p:?}");
            assert!(yes >= MIN_APPROVALS);
        }
    }
    assert!(proposed > 0, "the test makes no proposal");
}

#[test]
fn an_accept_after_a_new_refusal_adds_nothing() {
    let (dir, store) = store("stale");
    for _ in 0..MIN_APPROVALS {
        let m = message(&store, "deals@shop.example", false);
        decided(&store, "archive", &[m], "approved");
    }
    let p = find(&refresh(&store), "deals@shop.example").unwrap().id;
    // The refusal comes before the next pass.
    let m = message(&store, "deals@shop.example", false);
    decided(&store, "archive", &[m], "denied");
    assert_eq!(
        change(&store, None, Change::Accept(p), NOW).unwrap(),
        Edited::NoRule
    );
    assert!(!dir.0.join(FILE).exists());
}

#[test]
fn a_hostile_sender_kind_gives_no_proposal() {
    let (_dir, store) = store("hostile-kind");
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO sender(address, kind) VALUES ('bad@far.example', 'hostile')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    for _ in 0..MIN_APPROVALS {
        let m = message(&store, "bad@far.example", false);
        decided(&store, "archive", &[m], "approved");
    }
    assert!(refresh(&store).is_empty());
}

#[test]
fn bulk_approvals_do_not_count() {
    let (_dir, store) = store("bulk");
    for _ in 0..MIN_APPROVALS {
        let m = message(&store, "deals@shop.example", false);
        decided(&store, "archive", &[m], "approved");
    }
    store
        .write(|tx| {
            tx.execute(
                "UPDATE changeset_action SET flags = '[\"bulk\"]' WHERE id = 1",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(find(&refresh(&store), "deals@shop.example").is_none());
}

#[test]
fn a_refusal_whose_header_is_gone_blocks_its_verb() {
    let (_dir, store) = store("tombstone");
    for _ in 0..MIN_APPROVALS {
        let m = message(&store, "deals@shop.example", false);
        decided(&store, "archive", &[m], "approved");
    }
    // Approvals of another sender, so that only the blocked verb stops them.
    let m = message(&store, "news@shop.example", false);
    decided(&store, "archive", &[m], "denied");
    // The cleanup of a gone message keeps the row and deletes the header.
    store
        .write(|tx| {
            tx.execute("DELETE FROM header WHERE message_id = ?1", [m])?;
            Ok(())
        })
        .unwrap();
    assert!(refresh(&store).is_empty(), "the refusal can be from deals@");
}
