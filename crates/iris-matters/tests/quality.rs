//! The quality of matter proposals (issue #290): a hand-written set of
//! items with the true matters, and a pair score of the proposals.
//!
//! - The oracle run gives the true groups from the data block of the call.
//!   It measures the engine part (the candidate lines, the checks of the
//!   answer, the write), not a model. Its score must be 1.
//! - A real run: set `IRIS_EVAL_LOCAL_MODEL` to `<base URL>|<model>` of a
//!   local OpenAI-compatible server (for example the model server of the
//!   owner's Mac). The test then asks that model and prints the scores. It
//!   asserts no target: spec R6-01 sets none before the first measure.
//!
//! The report goes to stderr.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use iris_eval::{pair_scores, Count};
use iris_matters::{propose, Gate, Outcome};
use iris_model::{Error as ModelError, Reply, Stop, Usage};
use iris_store::{NewMessage, Store};
use iris_understand::budget::ModelRow;
use iris_understand::Model;
use rusqlite::params;
use serde_json::{json, Value};

const NOW: i64 = 2_000_000_000_000;

/// One item of the set: (true matter or "", form, thread key, doc).
type Case = (&'static str, &'static str, &'static str, Value);

/// The set: four matters over several threads each, and items of no
/// matter that look near one (the same city, the same person, a booking).
fn set() -> Vec<Case> {
    let rec = |title: &str, kind: &str, loc: &str, start: &str, with: &str| {
        json!({"title": title, "type": kind,
               "fields": {"location": loc, "start": start},
               "issuer": {"kind": "sender", "name": with}})
    };
    let talk = |title: &str, with: &str, at: &str| {
        json!({"title": title, "latest": {"at": at},
               "participants": [{"kind": "sender", "name": with}]})
    };
    let owe = |title: &str, with: &str, due: &str| {
        json!({"title": title, "due": {"date": due},
               "counterparty": {"kind": "sender", "name": with}})
    };
    vec![
        // Tokyo trip.
        (
            "tokyo",
            "record",
            "air",
            rec(
                "Flight SFO to Tokyo Haneda",
                "booking",
                "Tokyo",
                "2026-12-10",
                "Sky Air",
            ),
        ),
        (
            "tokyo",
            "record",
            "hotel",
            rec(
                "Hotel Shinjuku, 4 nights",
                "booking",
                "Tokyo",
                "2026-12-10",
                "Shinjuku Inn",
            ),
        ),
        (
            "tokyo",
            "conversation",
            "dana",
            talk("Dinner in Tokyo on the 12th?", "Dana Ito", "2026-11-20"),
        ),
        (
            "tokyo",
            "commitment",
            "visa",
            owe(
                "Send passport copy for the Japan trip",
                "Sky Air",
                "2026-11-30",
            ),
        ),
        // Apartment move.
        (
            "move",
            "record",
            "lease",
            rec(
                "Lease for 12 Oak Street",
                "contract",
                "12 Oak Street",
                "2027-01-01",
                "Oak Rentals",
            ),
        ),
        (
            "move",
            "commitment",
            "movers",
            owe(
                "Confirm moving date with Swift Movers",
                "Swift Movers",
                "2026-12-20",
            ),
        ),
        (
            "move",
            "conversation",
            "power",
            talk(
                "Electricity account for 12 Oak Street",
                "City Power",
                "2026-11-18",
            ),
        ),
        // Hire of a designer.
        (
            "hire",
            "conversation",
            "cand",
            talk(
                "Design role: interview with Sam Lee",
                "Sam Lee",
                "2026-11-19",
            ),
        ),
        (
            "hire",
            "commitment",
            "offer",
            owe("Send the offer letter to Sam Lee", "Sam Lee", "2026-11-28"),
        ),
        (
            "hire",
            "conversation",
            "recr",
            talk("Sam Lee: references from Hire Co", "Hire Co", "2026-11-17"),
        ),
        // Wedding of a friend.
        (
            "wedding",
            "record",
            "rsvp",
            rec(
                "RSVP: Maya and Raj wedding",
                "event",
                "Lisbon",
                "2027-05-02",
                "Maya Rao",
            ),
        ),
        (
            "wedding",
            "record",
            "lisbon",
            rec(
                "Flight to Lisbon",
                "booking",
                "Lisbon",
                "2027-04-30",
                "Sky Air",
            ),
        ),
        (
            "wedding",
            "commitment",
            "gift",
            owe(
                "Buy the wedding gift for Maya and Raj",
                "Maya Rao",
                "2027-04-20",
            ),
        ),
        // No matter.
        (
            "",
            "record",
            "rcpt1",
            rec(
                "Receipt: coffee beans",
                "receipt",
                "",
                "2026-11-15",
                "Bean Shop",
            ),
        ),
        (
            "",
            "conversation",
            "dana2",
            talk("Dana: book club next month", "Dana Ito", "2026-11-16"),
        ),
        (
            "",
            "record",
            "dent",
            rec(
                "Dentist appointment",
                "booking",
                "Main Street Clinic",
                "2026-12-03",
                "Main Street Clinic",
            ),
        ),
        (
            "",
            "commitment",
            "tax",
            owe("File the quarterly tax return", "Tax Office", "2027-01-15"),
        ),
        (
            "",
            "conversation",
            "ana",
            talk("Ana: notes from the team meeting", "Ana Ruiz", "2026-11-21"),
        ),
        (
            "",
            "record",
            "tokyo2",
            rec(
                "Tokyo Ramen: order 4471",
                "order",
                "",
                "2026-11-14",
                "Tokyo Ramen",
            ),
        ),
    ]
}

/// A store with a local model row and the items of `set`: one thread for
/// each thread key, one message in each thread.
fn setup(dir: &std::path::Path, set: &[Case]) -> Store {
    let _ = std::fs::remove_dir_all(dir);
    let store = Store::open(dir).unwrap();
    let a = store.add_account("gmail", "owner@example.com").unwrap();
    let msgs: Vec<NewMessage> = set
        .iter()
        .map(|(_, _, t, _)| NewMessage {
            provider_id: format!("g-{t}"),
            thread_provider_id: format!("gt-{t}"),
            ..Default::default()
        })
        .collect();
    let ids = store.put_messages(a, &msgs).unwrap();
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO model_setting(id, provider, base_url, model, version, max_tokens, timeout_s)
                 VALUES (1, 'local', 'http://127.0.0.1:1', 'eval', 1, 4000, 300)",
                [],
            )?;
            for ((_, form, _, doc), m) in set.iter().zip(&ids) {
                tx.execute(
                    "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                     VALUES (?1, ?2, 'open', ?3, ?4, ?4)",
                    params![form, a, doc.to_string(), NOW],
                )?;
                tx.execute(
                    "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
                    params![tx.last_insert_rowid(), m],
                )?;
            }
            Ok(())
        })
        .unwrap();
    store
}

/// Answers with the true groups: it finds the ID of each item in the data
/// block by its title.
struct Oracle(Vec<Case>);

impl Model for Oracle {
    fn name(&self) -> &str {
        "oracle"
    }
    fn ask(&self, _: &str, data: &str) -> Result<Reply, ModelError> {
        let id: HashMap<String, String> = data
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| Some((v["title"].as_str()?.into(), v["id"].as_str()?.into())))
            .collect();
        let mut groups: Vec<(&str, Vec<String>)> = Vec::new();
        for (m, _, _, doc) in self.0.iter().filter(|c| !c.0.is_empty()) {
            let Some(i) = doc["title"].as_str().and_then(|t| id.get(t)) else {
                continue;
            };
            match groups.iter_mut().find(|g| g.0 == *m) {
                Some(g) => g.1.push(i.clone()),
                None => groups.push((m, vec![i.clone()])),
            }
        }
        let matters: Vec<Value> = groups
            .into_iter()
            .map(|(m, items)| json!({"title": m, "items": items}))
            .collect();
        Ok(Reply {
            text: json!({ "matters": matters }).to_string(),
            usage: Usage::default(),
            stop: Stop::End,
        })
    }
}

/// What one call sent and gave back.
#[derive(Default)]
struct Call {
    /// The data block of the call.
    data: String,
    /// The reply ended (`Stop::End`) and is JSON with a `matters` array.
    usable: bool,
}

/// Forwards to `inner`, and keeps the data block and the kind of reply.
struct Watch {
    inner: Box<dyn Model>,
    call: Rc<RefCell<Call>>,
}

impl Model for Watch {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn ask(&self, instruction: &str, data: &str) -> Result<Reply, ModelError> {
        let reply = self.inner.ask(instruction, data);
        *self.call.borrow_mut() = Call {
            data: data.to_string(),
            usable: reply.as_ref().is_ok_and(usable),
        };
        reply
    }
    fn request_bytes(&self, instruction: &str, data: &str) -> u64 {
        self.inner.request_bytes(instruction, data)
    }
}

/// A reply that gives a score: a complete answer with a `matters` array.
fn usable(r: &Reply) -> bool {
    r.stop == Stop::End
        && serde_json::from_str::<Value>(&r.text).is_ok_and(|v| v["matters"].is_array())
}

/// Runs the step with `model`, and gives the outcome, the call, and the
/// pair score of the proposals against the true matters. It prints
/// nothing: the caller prints the score ([`print`]) only after the answer
/// check, so a failed run shows no score (issue #810).
fn measure(name: &str, model: impl Fn() -> Box<dyn Model>) -> (Outcome, Call, Count) {
    let dir = std::env::temp_dir().join(format!("iris-quality-{name}-{}", std::process::id()));
    let cases = set();
    let store = setup(&dir, &cases);
    let call = Rc::new(RefCell::new(Call::default()));
    let connect = |_: &ModelRow| -> Result<Box<dyn Model>, String> {
        Ok(Box::new(Watch {
            inner: model(),
            call: call.clone(),
        }))
    };
    let out = propose(&store, &connect, Gate::Now, NOW).unwrap();
    // Item row i + 1 is case i.
    let truth = groups(
        cases
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.0.is_empty())
            .map(|(i, c)| (c.0.to_string(), i as i64 + 1)),
    );
    let got = store
        .read(|c| {
            let mut st = c.prepare("SELECT matter_id, item_id FROM matter_item WHERE kept = 1")?;
            let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?.to_string(), r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<(String, i64)>>>()?)
        })
        .unwrap();
    let got = groups(got.into_iter());
    let score = pair_scores(&truth, &got);
    let _ = std::fs::remove_dir_all(&dir);
    let call = std::mem::take(&mut *call.borrow_mut());
    (out, call, score)
}

/// Prints the score of a run that passed the answer check.
fn print(name: &str, out: &Outcome, score: &Count) {
    eprintln!(
        "matters {name}: {out:?}\n  pairs {score:?} precision {:?} recall {:?}",
        score.precision(),
        score.recall()
    );
}

/// Each item of the set went to the model, the items of no matter too, so
/// a score counts the decoys.
fn assert_all_sent(call: &Call) {
    let titles: Vec<String> = call
        .data
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v["title"].as_str().map(String::from))
        .collect();
    for (_, _, _, doc) in set() {
        let t = doc["title"].as_str().unwrap();
        assert!(titles.iter().any(|x| x == t), "not sent: {t}");
    }
}

/// The members of each group key, in order of the first member.
fn groups(pairs: impl Iterator<Item = (String, i64)>) -> Vec<Vec<i64>> {
    let mut out: Vec<(String, Vec<i64>)> = Vec::new();
    for (k, i) in pairs {
        match out.iter_mut().find(|g| g.0 == k) {
            Some(g) => g.1.push(i),
            None => out.push((k, vec![i])),
        }
    }
    out.into_iter().map(|g| g.1).collect()
}

/// The engine keeps each true group of the oracle: precision and recall 1.
/// Red if the candidate lines lose an item or a title, or the checks drop a
/// valid matter.
#[test]
fn the_oracle_scores_one() {
    let (out, call, s) = measure("oracle", || Box::new(Oracle(set())));
    assert_all_sent(&call);
    assert!(call.usable);
    print("oracle", &out, &s);
    assert_eq!(s.expected, 6 + 3 + 3 + 3);
    assert_eq!((s.precision(), s.recall()), (Some(1.0), Some(1.0)), "{s:?}");
}

/// A real model of a local server, when `IRIS_EVAL_LOCAL_MODEL` names one.
#[test]
fn a_local_model_gives_a_score() {
    let Ok(v) = std::env::var("IRIS_EVAL_LOCAL_MODEL") else {
        eprintln!("matters local: IRIS_EVAL_LOCAL_MODEL is not set, no run");
        return;
    };
    let (url, model) = v.split_once('|').expect("<base URL>|<model>");
    let (url, model) = (url.to_string(), model.to_string());
    let (out, call, s) = measure("local", || {
        Box::new(iris_understand::Named {
            client: iris_model::Client::new(iris_model::Config {
                provider: iris_model::Provider::OpenAiCompatible,
                base_url: url.clone(),
                model: model.clone(),
                api_key: None,
                max_tokens: 4000,
                timeout: Duration::from_secs(300),
                price: None,
            }),
            name: model.clone(),
        })
    });
    // A call with no complete answer gives no score of the model.
    assert_all_sent(&call);
    assert!(
        call.usable,
        "the model gave no complete answer of the format"
    );
    print("local", &out, &s);
}

/// A cut, refused or empty answer is not usable: the local test fails on
/// it and does not print a score as if the model grouped nothing.
#[test]
fn a_cut_or_refused_reply_is_not_usable() {
    let reply = |stop, text: &str| Reply {
        text: text.into(),
        usage: Usage::default(),
        stop,
    };
    let good = r#"{"matters": []}"#;
    assert!(usable(&reply(Stop::End, good)));
    assert!(!usable(&reply(Stop::Truncated, good)));
    assert!(!usable(&reply(Stop::Refused, good)));
    assert!(!usable(&reply(Stop::End, r#"{"body": "Hello"}"#)));
    assert!(!usable(&reply(Stop::End, "{\"matters\": [")));
}
