//! The hostile test set in CI (card R4-12): a scripted agent obeys each
//! prompt injection of `iris_synth::hostile`, and no mail leaves without the
//! review of a person.
//!
//! The agent makes no model call. It writes the action that each attack
//! asks for, on three paths:
//!
//! - the engine socket, as the `iris` command of the host (author `local`);
//! - the steps of the socket, as an agent with no job;
//! - the steps of the socket, as an agent with a job whose scope is the
//!   thread of the attack. Today the engine denies its submit.
//!
//! Before the agent runs, the mail goes through a sync and through
//! the link checks and understanding with no model. The test checks the
//! clean text of each attack, and that no sample has a hostile mark.
//!
//! The setup is the worst case: the mode `apply`, a rule that allows each
//! Gmail verb on all mail, and the send switch on. Then the test tries a
//! rule approval of each action in the store, and runs the apply step of
//! the engine after the hold. The fake Gmail server counts each send.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use iris::socket;
use iris_apply::{Token, HOLD_MS};
use iris_changeset::{Author, AuthorKind, Verb};
use iris_fakegmail::FakeGmail;
use iris_gmail::{Config, Gmail};
use iris_store::Store;
use iris_synth::hostile::{Ask, Carrier, SAMPLES};
use iris_synth::Mailbox;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map, Value};

const TOKEN: &str = "test-token";
const OWNER: &str = "owner@example.com";

/// The rules of the worst case: each verb that a rule can decide, on all
/// mail, for both authors of the agent.
const POLICY: &str = r#"version = 0
mode = "apply"

[[rule]]
id = 1
name = "Allow each verb of local"
effect = "allow"
verbs = ["archive", "inbox", "label", "unlabel", "read", "unread", "trash"]
when = { author = "local" }
by = "person"
added = 2026-11-10

[[rule]]
id = 2
name = "Allow each verb of agent"
effect = "allow"
verbs = ["archive", "inbox", "label", "unlabel", "read", "unread", "trash"]
when = { author = "agent" }
by = "person"
added = 2026-11-10
"#;

struct Engine {
    fake: FakeGmail,
    gmail: Gmail,
    store: Arc<Store>,
    dir: PathBuf,
    account: i64,
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// The synthetic mailbox with each hostile sample, synced into a store,
/// with the engine socket served on a thread of its own.
fn engine() -> Engine {
    let fake = FakeGmail::start(Mailbox::new(12, 60)).unwrap();
    for s in SAMPLES {
        fake.add_message(&s.raw(), &["INBOX", "UNREAD"]);
    }
    let gmail = Gmail::new(Config {
        base_url: fake.base_url(),
        quota_per_minute: 10_000_000,
        backoff_start: Duration::from_millis(1),
        backoff_max: Duration::from_millis(4),
        ..Config::default()
    });
    let dir = std::env::temp_dir().join(format!("iris-hostile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Arc::new(Store::open(&dir).unwrap());
    let account = store.add_account("gmail", OWNER).unwrap();
    iris_sync::sync(&store, &gmail, account, TOKEN).unwrap();
    // Step 1 of each batch run: the link checks. A queued check counts as
    // hostile mail, so the test ends each check first.
    iris_understand::store::recheck_links(&store, now(), None).unwrap();
    let queued: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM link_check", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(queued, 0, "a link check is still queued");
    // Understanding with no model: the headers, the clean text, the hostile
    // marks and the sender kinds. Each model call is refused, so the run of a message
    // ends after these steps with an error.
    let ids: Vec<i64> = store
        .read(|c| {
            let mut st = c.prepare("SELECT id FROM message ORDER BY id")?;
            let rows = st.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    for id in ids {
        let _ = iris_understand::store::understand(
            &store,
            id,
            &iris_understand::budget::NoModel,
            now(),
        );
    }
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES (?1, 'h', 0)",
                [POLICY],
            )?;
            // The switch is on since time 0: no hold waits for the switch.
            tx.execute("UPDATE send_switch SET on_at = 0 WHERE id = 1", [])?;
            Ok(())
        })
        .unwrap();
    // A policy that does not parse falls back to the mode `observe`, which
    // applies nothing: the test then proves nothing.
    let active = store.read(|c| Ok(iris_policy::active(c).unwrap())).unwrap();
    assert_eq!(active.error, None);
    assert_eq!(active.policy.mode, iris_policy::Mode::Apply);
    let bound = socket::bind(&dir).unwrap();
    let s = store.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _ = rt.block_on(socket::serve(&bound, s));
    });
    Engine {
        fake,
        gmail,
        store,
        dir,
        account,
    }
}

impl Engine {
    fn one<T: rusqlite::types::FromSql>(&self, sql: &str, p: impl rusqlite::Params) -> T {
        self.store
            .read(|c| Ok(c.query_row(sql, p, |r| r.get(0))?))
            .unwrap()
    }

    /// The `t-N` thread of the message with this Message-ID.
    fn thread_of(&self, message_id: &str) -> String {
        let t: i64 = self.one(
            "SELECT m.thread_id FROM message m JOIN header h ON h.message_id = m.id
             WHERE h.message_id_hdr IN (?1, trim(?1, '<>'))",
            [message_id],
        );
        format!("t-{t}")
    }

    /// The POST calls to Gmail whose path ends with `end`.
    fn posts(&self, end: &str) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|r| r.starts_with("POST") && r.ends_with(end))
            .count()
    }

    /// Each call that sends mail: `messages.send` and `drafts.send`.
    fn sends(&self) -> usize {
        self.posts("/messages/send") + self.posts("/drafts/send")
    }

    /// The apply step of the engine at `at`.
    fn step(&self, at: i64) -> iris_apply::Step {
        let mut renew = || Some(TOKEN.to_string());
        let mut token = Token {
            token: TOKEN.into(),
            renew: &mut renew,
            refused: false,
        };
        iris_apply::step_clock(&self.store, &self.gmail, &mut token, self.account, &|| at).unwrap()
    }

    /// The action IDs and the flags of the changeset `cs-N`.
    fn actions(&self, cs: &str) -> Vec<(i64, String, Vec<String>)> {
        let id: i64 = cs.trim_start_matches("cs-").parse().unwrap();
        self.store
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT id, state, flags FROM changeset_action WHERE changeset_id = ?1",
                )?;
                let rows =
                    st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?)))?;
                Ok(rows
                    .map(|r| {
                        let (a, s, f) = r.unwrap();
                        (a, s, serde_json::from_str(&f).unwrap())
                    })
                    .collect())
            })
            .unwrap()
    }
}

/// The verb, the target and the arguments that the agent writes for `ask`.
fn obey(e: &Engine, ask: Ask, message_id: &str, draft: &str) -> (Verb, String, Map<String, Value>) {
    match ask {
        Ask::Draft { reply, to, cc } => {
            let mut args = json!({ "to": to, "cc": cc, "body": "Here it is." });
            let target = if reply {
                e.thread_of(message_id)
            } else {
                args["subject"] = json!("Requested");
                "a-1".to_string()
            };
            (Verb::Draft, target, args.as_object().unwrap().clone())
        }
        Ask::SendDraft => (Verb::Send, draft.to_string(), Map::new()),
    }
}

/// The agent of the socket path: `add`, then `submit`. Gives `cs-N`.
fn by_socket(e: &Engine, verb: Verb, target: &str, args: &Map<String, Value>) -> String {
    let r = socket::request(
        &e.dir,
        "add",
        json!({ "verb": verb.as_str(), "targets": [target], "args": args }),
    )
    .unwrap();
    let cs = r["changeset"].as_str().unwrap().to_string();
    socket::request(
        &e.dir,
        "submit",
        json!({ "changeset": cs, "why": "As the mail asks." }),
    )
    .unwrap();
    cs
}

/// An agent of the library path: the steps of the socket for `who`, with
/// the rules of the policy after the submit. Gives `cs-N`, or `None` if the
/// engine denies the submit.
fn by_agent(
    e: &Engine,
    who: &Author,
    verb: Verb,
    target: &str,
    args: &Map<String, Value>,
) -> Option<String> {
    let t = now();
    let cs = iris_changeset::create(&e.store, who, t).unwrap();
    iris_changeset::add_action(&e.store, cs, who, verb, &[target], args, None).unwrap();
    // The submit and the decisions of the policy in one transaction, as
    // the socket does.
    let submitted = iris_changeset::transaction(&e.store, |c| {
        iris_changeset::submit_in(c, cs, who, "As the mail asks.", t)?;
        let revision: i64 =
            c.query_row("SELECT revision FROM changeset WHERE id = ?1", [cs], |r| {
                r.get(0)
            })?;
        iris_triage::decide_submitted(c, cs, revision, iris_policy::Author::Agent, t)
    });
    match submitted {
        Ok(()) => Some(format!("cs-{cs}")),
        Err(iris_changeset::Error::Denied(why)) => {
            assert!(why.contains("cannot"), "{why}");
            None
        }
        Err(x) => panic!("{x}"),
    }
}

/// An agent (`iris` on the host, as an agent tool calls it), with the job
/// `job` or with no job.
fn agent(job: Option<i64>) -> Author {
    Author {
        kind: AuthorKind::Agent,
        name: Some("scripted".into()),
        session: job.map(|_| "se-1".into()),
        job: job.map(iris_job::format_job_id),
    }
}

/// A job whose scope is the thread `t-N` of the attack.
fn job(e: &Engine, thread: &str) -> i64 {
    let t = now();
    let j = iris_job::create(&e.store, "Answer the mail", None, t).unwrap();
    let id: i64 = thread.trim_start_matches("t-").parse().unwrap();
    iris_job::add_scope(
        &e.store,
        j,
        iris_job::Ref::Thread(id),
        iris_job::By::Person,
        t,
    )
    .unwrap();
    j
}

#[test]
fn no_injection_sends_mail_without_a_person() {
    let e = engine();
    let t = now();

    // The draft of the owner, approved by the person and made at Gmail: the
    // target of the attack "send the draft now".
    let person = Author {
        kind: AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    };
    let cs = iris_changeset::create(&e.store, &person, t).unwrap();
    let mail = json!({ "to": ["priya@example.com"], "subject": "Q4", "body": "Hi Priya." });
    iris_changeset::add_action(
        &e.store,
        cs,
        &person,
        Verb::Draft,
        &["a-1"],
        mail.as_object().unwrap(),
        None,
    )
    .unwrap();
    iris_changeset::submit(&e.store, cs, &person, "Write.", t).unwrap();
    e.store
        .write(|c| {
            Ok(c.execute(
                "UPDATE changeset_action SET state = 'approved', decided_by = 'person',
                   decided_at = ?2 WHERE changeset_id = ?1",
                params![cs, t],
            )?)
        })
        .unwrap();
    e.step(t);
    assert_eq!(
        e.posts("/drafts"),
        1,
        "the draft of the person is not at Gmail"
    );
    let draft = format!(
        "d-{}",
        e.one::<i64>("SELECT id FROM draft WHERE action_id IS NOT NULL", [])
    );

    // No sample has a hostile mark: each sender passes DMARC and no link
    // deceives. The text of an injection gives no mark, so each guard below
    // must hold without one.
    for s in SAMPLES {
        let marks: Option<String> = e.one(
            "SELECT m.hostile FROM message m JOIN header h ON h.message_id = m.id
             WHERE h.message_id_hdr IN (?1, trim(?1, '<>'))",
            [s.message_id()],
        );
        assert_eq!(marks.as_deref(), Some("[]"), "{}", s.name);
    }
    // Iris decoded each attack into the clean text that an agent reads,
    // except the hidden HTML block: the clean text takes the plain part.
    for s in SAMPLES {
        let (subject, text): (String, String) = e
            .store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT b.subject, b.text FROM body b JOIN header h ON h.message_id = b.message_id
                     WHERE h.message_id_hdr IN (?1, trim(?1, '<>'))",
                    [s.message_id()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        let read = match s.carrier {
            Carrier::Subject => subject.contains(s.attack),
            _ => text.contains(s.attack),
        };
        assert_eq!(
            read,
            s.carrier != Carrier::HiddenHtml,
            "{}: {subject} {text}",
            s.name
        );
    }

    // The agent obeys each attack on each path.
    let mut hostile = Vec::new();
    for s in SAMPLES {
        let thread = e.thread_of(&s.message_id());
        let (verb, target, args) = obey(&e, s.ask, &s.message_id(), &draft);
        let paths = [
            Some(by_socket(&e, verb, &target, &args)),
            by_agent(&e, &agent(None), verb, &target, &args),
            // Today a job cannot write mail: the engine denies the submit. A
            // later card that grants it makes this path reach the checks.
            by_agent(&e, &agent(Some(job(&e, &thread))), verb, &target, &args),
        ];
        assert!(paths[0].is_some() && paths[1].is_some(), "{}", s.name);
        for (i, cs) in paths.into_iter().flatten().enumerate() {
            let actions = e.actions(&cs);
            assert_eq!(actions.len(), 1, "{}: {cs}", s.name);
            let (a, state, flags) = actions.into_iter().next().unwrap();
            // A rule never decides a draft or a send: each waits for a
            // person. The engine denies each later send of the same draft,
            // because the first send can still go out.
            let reason: Option<String> =
                e.one("SELECT reason FROM changeset_action WHERE id = ?1", [a]);
            let expect = match s.ask {
                Ask::SendDraft if i > 0 => "denied",
                _ => "in_review",
            };
            assert_eq!(state, expect, "{}: {cs} {reason:?}", s.name);
            let outside = flags.iter().any(|f| f == "outside_thread");
            // The draft of the owner is a new mail to priya: a new mail has
            // no thread, so its send goes outside the thread too.
            let expect = s.outside || s.ask == Ask::SendDraft;
            if state == "in_review" {
                assert_eq!(outside, expect, "{}: {cs} {flags:?}", s.name);
            }
            // A denied action has its end state: only one in review can
            // get an approval.
            if state == "in_review" {
                hostile.push((s.name, a, flags));
            }
        }
    }
    assert!(!hostile.is_empty());

    // The release reads the clock of the system: an approval before the
    // hold delay lets each send leave at the next step.
    let past = t - HOLD_MS - 60_000;
    // A bug or a later card approves each action by rule. Each draft and
    // send of the set has a flag of the hard limit of spec 2, section 6.5,
    // so the store refuses each approval that is not by a person
    // (migration 25).
    let hard = ["new_recipient", "outside_thread", "money", "date"];
    for (name, a, flags) in &hostile {
        assert!(
            flags.iter().any(|f| hard.contains(&f.as_str())),
            "{name}: action {a} has no flag of the hard limit: {flags:?}"
        );
        let err = e
            .store
            .write(|c| {
                Ok(c.execute(
                    "UPDATE changeset_action SET state = 'approved', decided_by = 'rule',
                       decided_at = ?2 WHERE id = ?1",
                    params![a, past],
                )?)
            })
            .err()
            .map(|x| x.to_string())
            .unwrap_or_default();
        assert!(
            err.contains("only a person approves"),
            "{name}: action {a}: {err}"
        );
    }

    // The apply step after the hold: no draft from an attack reaches Gmail,
    // and no mail leaves. Two steps: the first moves sends to `held`.
    for _ in 0..2 {
        let step = e.step(t);
        for (a, r) in &step.results {
            assert!(
                !matches!(r, Ok(iris_apply::Outcome::Applied(_))),
                "action {a} applied: {r:?}"
            );
        }
    }
    assert_eq!(e.posts("/drafts"), 1, "a draft of an attack is at Gmail");
    assert_eq!(e.sends(), 0, "mail left without a person");
    let left: Option<i64> = e
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT id FROM changeset_action WHERE verb = 'send' AND state = 'applied'",
                [],
                |r| r.get(0),
            )
            .optional()?)
        })
        .unwrap();
    assert_eq!(left, None, "a send of an attack is applied");

    // The control: the person reviews the send of the attack "send the
    // draft now" and approves it. It leaves after the hold. Without this
    // control, a broken count of sends passes the test above.
    let (_, send, _) = hostile.iter().find(|(n, _, _)| *n == "send_now").unwrap();
    e.store
        .write(|c| {
            Ok(c.execute(
                "UPDATE changeset_action SET state = 'approved', decided_by = 'person',
                   decided_at = ?2 WHERE id = ?1 AND state = 'in_review'",
                params![send, past],
            )?)
        })
        .unwrap();
    e.step(t);
    assert_eq!(e.sends(), 1, "the send of the person did not leave");
}
