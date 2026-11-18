//! The verbs `draft` and `send` on an Outlook mailbox through the Graph
//! provider, at the fake Graph server (card R6-17, spec
//! `docs/specs/2026-11-17-outlook-provider.md` section 5). No network.

use std::path::PathBuf;
use std::time::Duration;

use iris_apply::{apply, release, Outcome, Token, HOLD_MS};
use iris_changeset::{add_action, create, submit, Author, AuthorKind, Verb};
use iris_fakegraph::FakeGraph;
use iris_graph::{Config, Graph, Outlook};
use iris_store::Store;
use iris_synth::Mailbox;
use rusqlite::params;
use serde_json::{json, Map, Value};

const TOKEN: &str = "test-token";
const OWNER: &str = "owner@example.com";
const AGED: i64 = HOLD_MS + 1_000;

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
    let fake = FakeGraph::start(Mailbox::new(7, 40)).unwrap();
    let outlook = Outlook::new(Graph::new(Config {
        base_url: fake.base_url(),
        backoff_start: Duration::from_millis(1),
        backoff_max: Duration::from_millis(4),
        ..Config::default()
    }));
    let dir = std::env::temp_dir().join(format!("iris-osend-{name}-{}", std::process::id()));
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
            tx.execute("UPDATE message SET hostile = NULL", [])?;
            tx.execute("UPDATE send_switch SET on_at = 0 WHERE id = 1", [])?;
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

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn tok() -> Token<'static> {
    let renew: &'static mut dyn FnMut() -> Option<String> =
        std::boxed::Box::leak(std::boxed::Box::new(|| Some(TOKEN.to_string())));
    Token {
        token: TOKEN.into(),
        renew,
        refused: false,
    }
}

impl Box {
    fn approved(&self, verb: Verb, target: &str, args: Value, age: i64) -> i64 {
        let who = Author {
            kind: AuthorKind::Person,
            name: None,
            session: None,
            job: None,
        };
        let cs = create(&self.store, &who, now()).unwrap();
        let args: Map<String, Value> = args.as_object().unwrap().clone();
        add_action(&self.store, cs, &who, verb, &[target], &args, None).unwrap();
        submit(&self.store, cs, &who, "Write.", now()).unwrap();
        self.store
            .write(|tx| {
                let id: i64 = tx.query_row(
                    "SELECT id FROM changeset_action WHERE changeset_id = ?1",
                    [cs],
                    |r| r.get(0),
                )?;
                tx.execute(
                    "UPDATE changeset_action SET state = 'approved', decided_by = 'person',
                       decided_at = ?2 WHERE id = ?1",
                    params![id, now() - age],
                )?;
                Ok(id)
            })
            .unwrap()
    }

    /// A new mail, made in Outlook. Gives the send action, approved `age`
    /// milliseconds ago, and the Graph ID of the draft.
    fn new_mail(&self, age: i64) -> (i64, String) {
        let a = self.approved(
            Verb::Draft,
            "a-1",
            json!({"to": ["priya@example.com"], "subject": "Q4 numbers", "body": "Hi Priya,\n\nHere.\n"}),
            0,
        );
        assert!(matches!(
            apply(&self.store, &self.outlook, &mut tok(), self.account, a),
            Ok(Outcome::Applied(_))
        ));
        let (d, provider): (String, String) = self
            .store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT 'd-' || id, provider_id FROM draft WHERE action_id = ?1",
                    [a],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        (self.approved(Verb::Send, &d, json!({}), age), provider)
    }

    fn release(&self, action: i64) -> Outcome {
        iris_changeset::hold(&self.store, self.account, now()).unwrap();
        release(&self.store, &self.outlook, &mut tok(), self.account, action).unwrap()
    }

    fn state(&self, action: i64) -> String {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT state FROM changeset_action WHERE id = ?1",
                    [action],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    /// The raw bytes and the sent message of the draft row.
    fn row(&self) -> (Vec<u8>, Option<String>) {
        self.store
            .read(|c| {
                Ok(c.query_row("SELECT raw, sent_message FROM draft", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?)
            })
            .unwrap()
    }
}

/// A draft goes to Outlook Drafts. Its send carries the approved bytes,
/// the sent copy completes the send, and the draft stays in Drafts.
#[test]
fn a_draft_and_its_send_reach_outlook() {
    let b = setup("send");
    let (s, draft) = b.new_mail(AGED);
    assert_eq!(b.fake.messages()[&draft], "F-drafts");
    assert!(matches!(b.release(s), Outcome::Applied(_)));
    assert_eq!(b.state(s), "applied");
    let (raw, sent) = b.row();
    assert_eq!(b.fake.sent_mail(), vec![raw]);
    let sent = sent.expect("the sent copy");
    assert_ne!(sent, draft);
    assert_eq!(b.fake.messages()[&sent], "F-sent");
    assert_eq!(
        b.fake.messages()[&draft],
        "F-drafts",
        "Iris deletes no draft"
    );
}

/// Graph saves the sent copy after the 202: the send is pending, the
/// recovery finds the copy later, and the mail goes one time only.
#[test]
fn a_late_sent_copy_is_found_and_never_sent_again() {
    let b = setup("late");
    let (s, _) = b.new_mail(AGED);
    b.fake.hold_copies(true);
    assert_eq!(b.release(s), Outcome::Pending);
    for _ in 0..2 {
        let _ = iris_apply::recover(&b.store, &b.outlook, &mut tok(), b.account);
        let _ = b.release(s);
    }
    assert_eq!((b.state(s).as_str(), b.fake.sent_mail().len()), ("held", 1));
    b.fake.deliver_copies();
    let r = iris_apply::recover(&b.store, &b.outlook, &mut tok(), b.account);
    eprintln!(
        "RECOVER {r:?} {:?}",
        b.fake.requests().iter().rev().take(8).collect::<Vec<_>>()
    );
    assert_eq!(
        (b.state(s).as_str(), b.fake.sent_mail().len()),
        ("applied", 1)
    );
    assert!(b.row().1.is_some());
}

/// A draft that the person deleted in Outlook is not sent.
#[test]
fn a_deleted_draft_is_not_sent() {
    let b = setup("deleted");
    let (s, draft) = b.new_mail(AGED);
    assert!(b.fake.delete_message(&draft));
    match b.release(s) {
        Outcome::Denied(why) => assert!(why.contains("gone"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(b.fake.sent_mail().is_empty());
}
