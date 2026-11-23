//! The verbs `draft` and `send` on an Outlook mailbox through the Graph
//! provider, at the fake Graph server (card R6-17, spec
//! `docs/specs/2026-11-17-outlook-provider.md` section 5). No network.

use std::path::PathBuf;
use std::time::Duration;

use iris_apply::{apply, release, Outcome, Token, HOLD_MS};
use iris_changeset::{add_action, create, submit, Author, AuthorKind, Verb};
use iris_fakegraph::FakeGraph;
use iris_graph::{Config, Graph, Outlook};
use iris_provider::Provider;
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
    // The good send tells the person that the draft stays (limit #634).
    let note: Option<String> = b
        .store
        .read(|c| {
            Ok(
                c.query_row("SELECT error FROM ledger WHERE action_id = ?1", [s], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(
        note.as_deref(),
        Some("Sent. The draft stays in Outlook: delete it there.")
    );
    // The note is no content error, so the next send can leave.
    let (s2, _) = b.new_mail(AGED);
    assert!(matches!(b.release(s2), Outcome::Applied(_)));
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
        Outcome::Denied(why) => assert_eq!(why, "the draft is gone at Outlook"),
        other => panic!("{other:?}"),
    }
    assert!(b.fake.sent_mail().is_empty());
}

impl Box {
    /// Move the claim of the send `action` back by `ms`, as a clock.
    fn age_claim(&self, action: i64, ms: i64) {
        self.store
            .write(|tx| {
                // ledger_change forbids a change of `at`: drop and remake the
                // trigger in this test store only.
                let sql: String = tx.query_row(
                    "SELECT sql FROM sqlite_schema WHERE name = 'ledger_change'",
                    [],
                    |r| r.get(0),
                )?;
                tx.execute_batch("DROP TRIGGER ledger_change")?;
                tx.execute(
                    "UPDATE ledger SET at = at - ?2 WHERE action_id = ?1",
                    params![action, ms],
                )?;
                tx.execute_batch(&sql)?;
                Ok(())
            })
            .unwrap();
    }

    fn error(&self, action: i64) -> Option<String> {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT error FROM ledger WHERE action_id = ?1",
                    [action],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }
}

/// A reply reads the headers of its parent in Outlook (`metadata`), and
/// the draft has the `In-Reply-To` of the parent.
#[test]
fn a_reply_draft_reads_its_parent_in_outlook() {
    let b = setup("reply");
    let (thread, parent, provider): (i64, i64, String) = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT m.thread_id, m.id, m.provider_id FROM message m
                 WHERE m.gone_at IS NULL AND m.internal_date = (SELECT max(internal_date)
                   FROM message WHERE thread_id = m.thread_id AND gone_at IS NULL)
                 ORDER BY m.id LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    let head = b
        .outlook
        .metadata(TOKEN, &provider, &["Message-ID", "Subject"])
        .unwrap();
    let parent_id = head
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("Message-ID"))
        .expect("the Message-ID of the parent")
        .value
        .clone();
    let subject = head
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("Subject"))
        .expect("the Subject of the parent")
        .value
        .clone();
    assert_eq!(head.headers.len(), 2);
    // The parent is understood: the mirror has its subject, so submit
    // flags the same subject as the apply.
    b.store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE header SET subject = ?1 WHERE message_id = ?2",
                params![subject, parent],
            )?)
        })
        .unwrap();
    let a = b.approved(
        Verb::Draft,
        &format!("t-{thread}"),
        json!({"to": ["priya@example.com"], "body": "Thanks."}),
        0,
    );
    match apply(&b.store, &b.outlook, &mut tok(), b.account, a) {
        Ok(Outcome::Applied(c)) => assert_eq!(c.changed, 1),
        other => panic!("{other:?}"),
    }
    let raw = String::from_utf8(b.row().0).unwrap();
    assert!(
        raw.lines()
            .any(|l| l.starts_with("In-Reply-To: ") && l.contains(parent_id.trim())),
        "{raw}"
    );
}

/// The results of an Outlook send name Outlook: no sent copy after 24
/// hours, and a sent copy with other content, which stops each later
/// release.
#[test]
fn the_results_of_an_outlook_send_name_outlook() {
    let b = setup("results");
    let (s, _) = b.new_mail(AGED);
    b.fake.hold_copies(true);
    assert_eq!(b.release(s), Outcome::Pending);
    b.age_claim(s, 24 * 60 * 60 * 1000 + 1);
    iris_apply::recover(&b.store, &b.outlook, &mut tok(), b.account).unwrap();
    assert_eq!(
        b.error(s).as_deref(),
        Some("unknown: Outlook shows no sent message")
    );

    let b = setup("other");
    let (s, _) = b.new_mail(AGED);
    b.fake.hold_copies(true);
    assert_eq!(b.release(s), Outcome::Pending);
    // Outlook saves a copy with the same Message-ID and other text.
    let raw = String::from_utf8(b.row().0).unwrap();
    let id = raw
        .lines()
        .find(|l| l.starts_with("Message-ID: "))
        .unwrap()
        .to_string();
    let other = format!(
        "From: <{OWNER}>\r\nTo: <spy@example.org>\r\n{id}\r\nSubject: Q4 numbers\r\n\r\nOther.\r\n"
    );
    let copy = b.fake.add_message(other.as_bytes(), "F-sent");
    iris_sync::sync(&b.store, &b.outlook, b.account, TOKEN).unwrap();
    iris_apply::recover(&b.store, &b.outlook, &mut tok(), b.account).unwrap();
    assert_eq!(
        b.error(s).as_deref(),
        Some("Outlook sent other content than the approved content")
    );
    // SENT says that the owner wrote the message: a copy with other content
    // does not get it, now or after a restart.
    assert!(!b.labels(&copy).contains(&"SENT".to_string()));
    assert_eq!(b.differences(&b.outlook), 0);
    assert!(iris_apply::sent_by_iris(&b.store, b.account)
        .unwrap()
        .is_empty());
    let fresh = Outlook::new(Graph::new(Config {
        base_url: b.fake.base_url(),
        ..Config::default()
    }))
    .with_sent(iris_apply::sent_by_iris(&b.store, b.account).unwrap());
    iris_sync::sync(&b.store, &fresh, b.account, TOKEN).unwrap();
    assert!(!b.labels(&copy).contains(&"SENT".to_string()));
    let (s2, _) = b.new_mail(AGED);
    iris_changeset::hold(&b.store, b.account, now()).unwrap();
    assert!(matches!(
        release(&b.store, &b.outlook, &mut tok(), b.account, s2),
        Err(iris_apply::Error::SendStopped)
    ));
}

/// A mail to the owner arrives in the inbox before Outlook saves the sent
/// copy: the delivery has the same Message-ID and a send time, but does
/// not confirm the send. The copy in Sent Items does.
#[test]
fn a_delivery_to_the_owner_does_not_confirm_the_send() {
    let b = setup("self");
    let (s, _) = b.new_mail(AGED);
    b.fake.hold_copies(true);
    assert_eq!(b.release(s), Outcome::Pending);
    let raw = b.row().0;
    let delivered = b.fake.add_message(&raw, "F-inbox");
    iris_apply::recover(&b.store, &b.outlook, &mut tok(), b.account).unwrap();
    assert_eq!(b.state(s), "held");
    b.fake.deliver_copies();
    iris_apply::recover(&b.store, &b.outlook, &mut tok(), b.account).unwrap();
    assert_eq!(b.state(s), "applied");
    let sent = b.row().1.unwrap();
    assert_ne!(sent, delivered);
    assert_eq!(b.fake.messages()[&sent], "F-sent");
    assert_eq!(b.fake.sent_mail().len(), 1);
}

impl Box {
    /// The labels of the mirror message with this Graph ID.
    fn labels(&self, provider_id: &str) -> Vec<String> {
        self.store
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT l.provider_id FROM message m
                     JOIN message_label ml ON ml.message_id = m.id
                     JOIN label l ON l.id = ml.label_id
                     WHERE m.account_id = ?1 AND m.provider_id = ?2",
                )?;
                let v = st
                    .query_map(params![self.account, provider_id], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?;
                Ok(v)
            })
            .unwrap()
    }

    fn set_understood(&self, provider_id: &str) {
        let n = self
            .store
            .write(|tx| {
                Ok(tx.execute(
                    "UPDATE message SET understood_at = 1 WHERE account_id = ?1 AND provider_id = ?2",
                    params![self.account, provider_id],
                )?)
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    fn understood(&self, provider_id: &str) -> Option<i64> {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT understood_at FROM message WHERE account_id = ?1 AND provider_id = ?2",
                    params![self.account, provider_id],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    fn differences(&self, mail: &dyn Provider) -> u64 {
        iris_sync::verify(&self.store, mail, self.account, TOKEN)
            .unwrap()
            .total
    }
}

/// Only the sent copy of a message that Iris sent gets SENT (spec section
/// 6). A message that the person moved into Sent Items does not. A sync
/// that copies the sent message before the send completes gets SENT at
/// the completion, and a new provider value of the engine agrees.
#[test]
fn only_the_copies_that_iris_sent_get_sent() {
    let b = setup("own-sent");
    let (s, _) = b.new_mail(AGED);
    let other = b.fake.add_message(
        b"From: <x@example.org>\r\nMessage-ID: <x@example.org>\r\nSubject: moved\r\n\r\nhi\r\n",
        "F-sent",
    );
    b.fake.hold_copies(true);
    assert_eq!(b.release(s), Outcome::Pending);
    let before = b.fake.messages();
    b.fake.deliver_copies();
    iris_sync::sync(&b.store, &b.outlook, b.account, TOKEN).unwrap();
    let raw = b.row().0;
    let copy = b
        .fake
        .messages()
        .into_keys()
        .find(|id| !before.contains_key(id))
        .unwrap();
    assert!(
        !b.labels(&copy).contains(&"SENT".to_string()),
        "not known yet"
    );
    // The copy was understood as received mail before the completion.
    b.set_understood(&copy);
    iris_apply::recover(&b.store, &b.outlook, &mut tok(), b.account).unwrap();
    assert_eq!(b.state(s), "applied");
    assert_eq!(b.row().1.as_deref(), Some(copy.as_str()));
    assert_eq!(b.fake.sent_mail(), vec![raw]);
    assert!(b.labels(&copy).contains(&"SENT".to_string()));
    assert!(!b.labels(&other).contains(&"SENT".to_string()));
    assert_eq!(b.understood(&copy), None, "SENT needs a new understanding");
    assert_eq!(b.differences(&b.outlook), 0);
    // A new value of the provider, as the engine makes it after a restart.
    let config = Config {
        base_url: b.fake.base_url(),
        ..Config::default()
    };
    let sent = iris_apply::sent_by_iris(&b.store, b.account).unwrap();
    assert_eq!(sent, vec![copy.clone()]);
    let fresh = Outlook::new(Graph::new(config.clone())).with_sent(sent);
    assert_eq!(b.differences(&fresh), 0);
    assert!(b.labels(&copy).contains(&"SENT".to_string()));
    // With no list, the copy loses SENT: the list is what proves it.
    let blind = Outlook::new(Graph::new(config));
    assert!(b.differences(&blind) > 0);
}

/// Outlook keeps the Message-ID that Iris set, so another ID at the server
/// means another client changed the draft: the release denies the send and
/// marks the row foreign. It does not take the other ID (only Gmail sets
/// its own ID on a draft, send spec assumption 3).
#[test]
fn an_outlook_draft_with_another_message_id_is_foreign() {
    let b = setup("other-id");
    let (s, _) = b.new_mail(AGED);
    b.store
        .write(|tx| Ok(tx.execute("UPDATE draft SET message_id_hdr = 'other@example.com'", [])?))
        .unwrap();
    assert!(matches!(b.release(s), Outcome::Denied(_)));
    let (foreign, id): (bool, String) = b
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT foreign_at IS NOT NULL, message_id_hdr FROM draft",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert!(foreign);
    assert_eq!(id, "other@example.com");
    assert!(b.fake.sent_mail().is_empty());
}

/// Two engine processes on one data folder (issue #719): a provider value
/// that reads the sent copies from the store before each read of labels
/// agrees with a send that the other process completes. A value with the
/// list of its start does not, so the source is what keeps `SENT`.
#[test]
fn a_provider_with_a_store_source_sees_a_send_of_another_process() {
    let b = setup("two-engines");
    let config = Config {
        base_url: b.fake.base_url(),
        ..Config::default()
    };
    let (account, second) = (b.account, Store::open(&b.dir).unwrap());
    let other = Outlook::new(Graph::new(config.clone())).with_sent_source(move || {
        iris_apply::sent_by_iris(&second, account).map_err(|e| e.to_string())
    });
    let start = Outlook::new(Graph::new(config))
        .with_sent(iris_apply::sent_by_iris(&b.store, account).unwrap());
    assert_eq!(b.differences(&other), 0);
    let (s, _) = b.new_mail(AGED);
    assert!(matches!(b.release(s), Outcome::Applied(_)));
    let copy = b.row().1.expect("the send is complete");
    iris_sync::sync(&b.store, &b.outlook, account, TOKEN).unwrap();
    assert!(b.labels(&copy).contains(&"SENT".to_string()));
    assert_eq!(b.differences(&other), 0, "the other value keeps SENT");
    assert!(b.labels(&copy).contains(&"SENT".to_string()));
    assert!(
        b.differences(&start) > 0,
        "the list of the start lacks the copy"
    );
}

/// A store source that fails gives an error, not labels without `SENT`.
#[test]
fn a_failed_store_source_gives_an_error() {
    let b = setup("source-error");
    let failing = Outlook::new(Graph::new(Config {
        base_url: b.fake.base_url(),
        ..Config::default()
    }))
    .with_sent_source(|| Err("disk I/O error".into()));
    let id = b.fake.messages().into_keys().next().unwrap();
    assert!(failing.minimal(TOKEN, &id).is_err());
}

/// A provider value with no sent copies, as a sync that read the labels
/// before the send completion committed.
fn stale(b: &Box) -> Outlook {
    Outlook::new(Graph::new(Config {
        base_url: b.fake.base_url(),
        ..Config::default()
    }))
}

/// Migration 41 (review of #768): a label write that a stale read made
/// cannot remove SENT from a confirmed copy, in any order of the sync and
/// the completion.
#[test]
fn a_stale_label_write_keeps_sent_on_a_confirmed_copy() {
    let b = setup("stale-remove");
    let (s, _) = b.new_mail(AGED);
    assert!(matches!(b.release(s), Outcome::Applied(_)));
    let copy = b.row().1.expect("the send is complete");
    iris_sync::sync(&b.store, &b.outlook, b.account, TOKEN).unwrap();
    assert!(b.labels(&copy).contains(&"SENT".to_string()));
    // The difference starts a full resync, which writes the stale labels.
    assert!(b.differences(&stale(&b)) > 0);
    iris_sync::sync(&b.store, &stale(&b), b.account, TOKEN).unwrap();
    assert!(b.labels(&copy).contains(&"SENT".to_string()));
    assert_eq!(b.differences(&b.outlook), 0);
}

/// The completion confirms a copy that is not in the mirror yet, and then a
/// stale sync copies it: the copy gets SENT.
#[test]
fn a_stale_sync_of_a_confirmed_copy_adds_sent() {
    let b = setup("stale-add");
    let (s, _) = b.new_mail(AGED);
    b.fake.hold_copies(true);
    assert_eq!(b.release(s), Outcome::Pending);
    b.fake.deliver_copies();
    iris_apply::recover(&b.store, &b.outlook, &mut tok(), b.account).unwrap();
    let copy = b.row().1.expect("the send is complete");
    assert!(b.labels(&copy).is_empty(), "not in the mirror yet");
    iris_sync::sync(&b.store, &stale(&b), b.account, TOKEN).unwrap();
    assert!(b.labels(&copy).contains(&"SENT".to_string()));
    assert_eq!(b.differences(&b.outlook), 0);
}
