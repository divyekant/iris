//! The apply of approved actions to an Outlook mailbox through the Graph
//! provider, at the fake Graph server (card R6-15, spec
//! `docs/specs/2026-11-17-outlook-provider.md` section 6). No network.

use std::path::PathBuf;
use std::time::Duration;

use iris_apply::{apply, Outcome, Token};
use iris_fakegraph::FakeGraph;
use iris_graph::{Config, Graph, Outlook};
use iris_provider::Provider;
use iris_store::Store;
use iris_synth::Mailbox;
use rusqlite::params;

const TOKEN: &str = "test-token";
const OWNER: &str = "owner@example.com";
const PERSON: &str = "decided_by = 'person', decided_at = 2";

struct Box {
    fake: FakeGraph,
    outlook: Outlook,
    store: Store,
    account: i64,
    dir: PathBuf,
}

impl Drop for Box {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn setup(name: &str) -> Box {
    let fake = FakeGraph::start(Mailbox::new(7, 120)).unwrap();
    let outlook = Outlook::new(Graph::new(Config {
        base_url: fake.base_url(),
        backoff_start: Duration::from_millis(1),
        backoff_max: Duration::from_millis(4),
        ..Config::default()
    }));
    let dir = std::env::temp_dir().join(format!("iris-apply-o-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("outlook", OWNER).unwrap();
    iris_sync::sync(&store, &outlook, account, TOKEN).unwrap();
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO policy_version(text, sha256, confirmed_at)
                 VALUES ('version = 0\nmode = \"apply\"\n', 'h', 0)",
                [],
            )?;
            // No sender is hostile in these tests.
            tx.execute("UPDATE message SET hostile = NULL", [])?;
            tx.execute("DELETE FROM sender WHERE kind = 'hostile'", [])?;
            Ok(())
        })
        .unwrap();
    Box {
        fake,
        outlook,
        store,
        account,
        dir,
    }
}

impl Box {
    /// Message rows of the fake folder `folder`.
    fn in_folder(&self, folder: &str, n: usize) -> Vec<i64> {
        let ids: Vec<String> = self
            .fake
            .messages()
            .into_iter()
            .filter(|(_, f)| f == folder)
            .map(|(id, _)| id)
            .take(n)
            .collect();
        ids.iter()
            .map(|id| {
                self.store
                    .read(|c| {
                        Ok(c.query_row(
                            "SELECT id FROM message WHERE provider_id = ?1",
                            [id],
                            |r| r.get(0),
                        )?)
                    })
                    .unwrap()
            })
            .collect()
    }

    fn approved(&self, verb: &str, args: &str, messages: &[i64]) -> i64 {
        let ids: Vec<String> = messages.iter().map(|m| format!("\"m-{m}\"")).collect();
        let targets = format!("{{\"messages\":[{}]}}", ids.join(","));
        self.store
            .write(|tx| {
                tx.execute(
                    "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0)",
                    [],
                )?;
                let cs = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                     VALUES (?1, 1, 1, ?2, ?3, ?4)",
                    params![cs, verb, targets, args],
                )?;
                let a = tx.last_insert_rowid();
                for m in messages {
                    tx.execute(
                        "INSERT INTO action_message(action_id, message_id) VALUES (?1, ?2)",
                        params![a, m],
                    )?;
                }
                let (_, h) = iris_changeset::hash(tx, a).unwrap();
                tx.execute(
                    "UPDATE changeset_action SET hash = ?2 WHERE id = ?1",
                    params![a, h],
                )?;
                tx.execute(
                    "UPDATE changeset SET why = 'w', submitted_at = ?2 WHERE id = ?1",
                    params![cs, now()],
                )?;
                tx.execute(
                    &format!(
                        "UPDATE changeset_action SET state = 'approved', {PERSON} WHERE id = ?1"
                    ),
                    [a],
                )?;
                Ok(a)
            })
            .unwrap()
    }

    fn apply(&self, action: i64) -> Outcome {
        let mut renew = || Some(TOKEN.to_string());
        let mut token = Token {
            token: TOKEN.into(),
            renew: &mut renew,
            refused: false,
        };
        apply(&self.store, &self.outlook, &mut token, self.account, action).unwrap()
    }

    fn provider_id(&self, message: i64) -> String {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT provider_id FROM message WHERE id = ?1",
                    [message],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    /// The labels of a message at Outlook, sorted.
    fn outlook_labels(&self, message: i64) -> Vec<String> {
        let mut l = self
            .outlook
            .minimal(TOKEN, &self.provider_id(message))
            .unwrap()
            .label_ids;
        l.sort();
        l
    }

    fn mirror_labels(&self, message: i64) -> Vec<String> {
        self.store
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT l.provider_id FROM message_label ml JOIN label l ON l.id = ml.label_id
                     WHERE ml.message_id = ?1 ORDER BY 1",
                )?;
                let rows = st.query_map([message], |r| r.get(0))?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap()
    }

    /// The count of write requests at the fake: PATCH and move.
    fn writes(&self) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|r| r.starts_with("PATCH") || r.contains("/move"))
            .count()
    }

    fn undo(&self, action: i64) -> Vec<Outcome> {
        let entry: i64 = self
            .store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT id FROM ledger WHERE action_id = ?1",
                    [action],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        let actions = iris_apply::undo(&self.store, entry, now()).unwrap();
        actions.iter().map(|&a| self.apply(a)).collect()
    }

    fn sync(&self) {
        iris_sync::sync(&self.store, &self.outlook, self.account, TOKEN).unwrap();
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn changed(o: Outcome) -> u64 {
    match o {
        Outcome::Applied(c) => c.changed,
        other => panic!("{other:?}"),
    }
}

fn failed(o: Outcome) -> String {
    match o {
        Outcome::Failed { counts, error } => {
            assert_eq!(counts.changed, 0, "{error}");
            error
        }
        other => panic!("{other:?}"),
    }
}

/// Each verb changes Outlook, the undo returns it, and after a sync the
/// mirror has the labels of Outlook.
#[test]
fn each_verb_and_its_undo_change_outlook() {
    let b = setup("verbs");
    let ms = b.in_folder("F-inbox", 4);
    let has = |m: i64, l: &str| b.outlook_labels(m).iter().any(|x| x == l);
    type Done = fn(&[String]) -> bool;
    let cases: [(&str, &str, Done); 5] = [
        ("archive", "{}", |l| {
            l.iter().any(|x| x == "folder:F-archive")
        }),
        ("unread", "{}", |l| l.iter().any(|x| x == "UNREAD")),
        ("read", "{}", |l| !l.iter().any(|x| x == "UNREAD")),
        ("trash", "{}", |l| l.iter().any(|x| x == "TRASH")),
        ("label", r#"{"label":"Blue category"}"#, |l| {
            l.iter().any(|x| x == "category:Blue category")
        }),
    ];
    for (verb, args, done) in cases {
        let before: Vec<Vec<String>> = ms.iter().map(|&m| b.outlook_labels(m)).collect();
        let a = b.approved(verb, args, &ms);
        assert!(changed(b.apply(a)) > 0, "{verb}: the test changes nothing");
        for &m in &ms {
            assert!(
                done(&b.outlook_labels(m)),
                "{verb}: {:?}",
                b.outlook_labels(m)
            );
        }
        b.sync();
        for &m in &ms {
            assert_eq!(b.mirror_labels(m), b.outlook_labels(m), "{verb}");
        }
        let outcomes = b.undo(a);
        assert!(
            outcomes.iter().all(|o| matches!(o, Outcome::Applied(_))),
            "{verb}: {outcomes:?}"
        );
        for (i, &m) in ms.iter().enumerate() {
            assert_eq!(b.outlook_labels(m), before[i], "{verb} undo");
        }
    }
    assert!(has(ms[0], "INBOX"));
}

/// A label action on a folder label, a move out of a user folder and a
/// category that does not exist each fail before any write.
#[test]
fn the_refusals_come_before_any_write() {
    let b = setup("refusals");
    let inbox = b.in_folder("F-inbox", 2);
    let before = b.writes();
    let a = b.approved("label", r#"{"label":"Folders/Receipts"}"#, &inbox);
    let e = failed(b.apply(a));
    assert!(e.contains("folder"), "{e}");
    let user_folder = b.in_folder("F-old", 1);
    assert_eq!(user_folder.len(), 1);
    // An archive of a message with no INBOX label changes nothing.
    let a = b.approved("archive", "{}", &user_folder);
    assert_eq!(changed(b.apply(a)), 0);
    let a = b.approved("trash", "{}", &user_folder);
    let e = failed(b.apply(a));
    assert!(e.contains("inbox"), "{e}");
    let a = b.approved("label", r#"{"label":"Never made"}"#, &inbox);
    let e = failed(b.apply(a));
    assert!(e.contains("Outlook"), "{e}");
    assert_eq!(b.writes(), before, "no write request");
    assert_eq!(b.fake.messages()[&b.provider_id(user_folder[0])], "F-old");
}

/// A category write keeps the categories that another client added: the
/// new list comes from the read of the call.
#[test]
fn a_label_keeps_the_other_categories() {
    let b = setup("categories");
    let m = b.in_folder("F-inbox", 1)[0];
    let id = b.provider_id(m);
    assert!(b.fake.set_categories(&id, &["Receipts", "Other client"]));
    let a = b.approved("label", r#"{"label":"Blue category"}"#, &[m]);
    assert_eq!(changed(b.apply(a)), 1);
    let l = b.outlook_labels(m);
    for c in ["Receipts", "Other client", "Blue category"] {
        assert!(l.contains(&format!("category:{c}")), "{l:?}");
    }
}
