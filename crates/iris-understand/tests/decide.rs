//! Steps 5 to 7 with a fake model: no key and no network.

use std::cell::RefCell;
use std::collections::VecDeque;

use iris_model::{Error as ModelError, Reply, Stop, Usage};
use iris_understand::{analyze, decide, prompt, Analysis, Context, Kind, Model, INSTRUCTION};
use serde_json::{json, Value};

const NOW: i64 = 1_767_225_600_000;
const ME: &str = "owner@example.com";

/// Gives the queued replies in order, and keeps each call.
#[derive(Default)]
struct Fake {
    replies: RefCell<VecDeque<Result<Reply, ModelError>>>,
    calls: RefCell<Vec<(String, String)>>,
}

impl Fake {
    fn with(replies: Vec<Result<Reply, ModelError>>) -> Fake {
        Fake {
            replies: RefCell::new(replies.into()),
            calls: RefCell::default(),
        }
    }
}

impl Model for Fake {
    fn name(&self) -> &str {
        "fake"
    }

    fn ask(&self, instruction: &str, data: &str) -> Result<Reply, ModelError> {
        self.calls
            .borrow_mut()
            .push((instruction.to_string(), data.to_string()));
        self.replies
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok(text(r#"{"items": []}"#)))
    }
}

fn text(t: &str) -> Reply {
    Reply {
        text: t.to_string(),
        usage: Usage {
            input_tokens: 100,
            output_tokens: 10,
            cost_micro_usd: Some(5),
        },
        stop: Stop::End,
    }
}

fn answer(items: Value) -> Result<Reply, ModelError> {
    Ok(text(
        &json!({"items": items, "phishing": false}).to_string(),
    ))
}

const ASK: &str = "From: Ana Lee <ana@example.org>
To: owner@example.com
Subject: Slides

Can you send the slides by Friday?
";

fn run(
    raw: &str,
    kind: Kind,
    labels: &[&str],
    existing: &[Value],
    model: Option<&Fake>,
) -> (Analysis, Result<iris_understand::Decision, ModelError>) {
    let labels: Vec<String> = labels.iter().map(|l| l.to_string()).collect();
    let none = |_: &str| Vec::new();
    let cx = Context {
        me: ME,
        kind,
        labels: &labels,
        internal_date: Some(NOW - 60_000),
        now_ms: NOW,
        known_person: &none,
        existing,
        thread: &[],
        same_sender: &[],
    };
    let a = analyze(raw.replace('\n', "\r\n").as_bytes(), &cx);
    let d = decide(&a, &cx, model.map(|m| m as &dyn Model));
    (a, d)
}

fn task() -> Value {
    json!([{"form": "commitment", "title": "Send the slides", "quote": "Can you send the slides by Friday?",
            "direction": "you_owe", "kind": "task", "confidence": 0.9}])
}

#[test]
fn the_model_gives_a_commitment_and_the_mail_stays_in_the_data() {
    let m = Fake::with(vec![answer(task())]);
    let (_, d) = run(ASK, Kind::Person, &[], &[], Some(&m));
    let d = d.unwrap();
    assert_eq!(d.calls, 1);
    assert_eq!(d.drafts.len(), 1);
    let c = &d.drafts[0].item;
    assert_eq!(c["status"], "open");
    assert_eq!(c["owner"], json!({"kind": "self"}));
    assert_eq!(c["counterparty"]["address"], "ana@example.org");
    assert_eq!(d.usage.input_tokens, 100);
    assert_eq!(d.usage.cost_micro_usd, Some(5));
    let calls = m.calls.borrow();
    // The instruction is fixed text. The mail goes only in the data.
    assert_eq!(calls[0].0, INSTRUCTION);
    assert!(!calls[0].0.contains("slides"));
    assert!(calls[0].1.contains("Can you send the slides by Friday?"));
}

#[test]
fn one_bad_answer_gets_one_more_try() {
    let m = Fake::with(vec![Ok(text("not json")), answer(task())]);
    let (_, d) = run(ASK, Kind::Person, &[], &[], Some(&m));
    let d = d.unwrap();
    assert_eq!((d.calls, d.drafts.len(), d.failed), (2, 1, false));
    assert!(m.calls.borrow()[1].0.ends_with(prompt::RETRY_NOTE));
    assert_eq!(d.usage.input_tokens, 200);
}

#[test]
fn two_bad_answers_fail_with_no_item() {
    let mut cut = text(r#"{"items": ["#);
    cut.stop = Stop::Truncated;
    let m = Fake::with(vec![Ok(cut), Err(ModelError::BadAnswer("x".into()))]);
    let (_, d) = run(ASK, Kind::Person, &[], &[], Some(&m));
    let d = d.unwrap();
    assert_eq!((d.calls, d.drafts.len(), d.failed), (2, 0, true));
}

#[test]
fn a_quote_that_the_mail_does_not_hold_is_not_valid() {
    let made_up = json!([{"form": "commitment", "title": "Pay", "quote": "Pay 5000 USD now",
                          "direction": "you_owe", "kind": "task"}]);
    let m = Fake::with(vec![answer(made_up.clone()), answer(made_up)]);
    let (_, d) = run(ASK, Kind::Person, &[], &[], Some(&m));
    assert!(d.unwrap().failed);
    // Case and white space do not count.
    let loose = json!([{"form": "commitment", "title": "Send", "quote": "can you  SEND\nthe slides",
                        "direction": "you_owe", "kind": "task"}]);
    let m = Fake::with(vec![answer(loose)]);
    let (_, d) = run(ASK, Kind::Person, &[], &[], Some(&m));
    assert_eq!(d.unwrap().drafts.len(), 1);
}

#[test]
fn phishing_is_hostile_with_no_item() {
    let m = Fake::with(vec![Ok(text(
        r#"{"items": [{"form": "x"}], "phishing": true}"#,
    ))]);
    let (_, d) = run(ASK, Kind::Stranger, &[], &[], Some(&m));
    let d = d.unwrap();
    assert!(d.phishing && d.hostile && d.drafts.is_empty());
}

#[test]
fn a_model_error_or_a_refusal_is_an_error() {
    let m = Fake::with(vec![Err(ModelError::Transport("lost".into()))]);
    assert!(matches!(
        run(ASK, Kind::Person, &[], &[], Some(&m)).1,
        Err(ModelError::Transport(_))
    ));
    let mut no = text("");
    no.stop = Stop::Refused;
    let m = Fake::with(vec![Ok(no)]);
    assert!(matches!(
        run(ASK, Kind::Person, &[], &[], Some(&m)).1,
        Err(ModelError::Rejected { .. })
    ));
}

#[test]
fn hostile_mail_and_rules_make_no_model_call() {
    let m = Fake::default();
    let (_, d) = run(ASK, Kind::Person, &["SPAM"], &[], Some(&m));
    assert!(d.unwrap().hostile);
    let code = "From: Bank <no-reply@bank.example>\nSubject: Sign in\n\nYour code: 482913\n";
    let (_, d) = run(code, Kind::Service, &[], &[], Some(&m));
    let d = d.unwrap();
    assert_eq!(d.drafts[0].item["code"], "482913");
    assert!(m.calls.borrow().is_empty());
    // An existing rule item of the same rule adds nothing.
    let old = json!({"form": "signal", "derived": {"by": "rule", "rule": "code"}});
    let (_, d) = run(code, Kind::Service, &[], &[old], Some(&m));
    assert!(d.unwrap().drafts.is_empty());
}

#[test]
fn the_retry_state_holds_only_the_model_call() {
    let (_, d) = run(ASK, Kind::Person, &[], &[], None);
    let d = d.unwrap();
    assert!(d.waiting && d.drafts.is_empty() && d.calls == 0);
    let (_, d) = run(ASK, Kind::Person, &["SPAM"], &[], None);
    assert!(d.unwrap().hostile);
}

#[test]
fn same_as_an_existing_item_adds_nothing() {
    let mut items = task();
    items[0]["same_as"] = json!("c-7");
    let m = Fake::with(vec![answer(items)]);
    let existing = [json!({"id": "c-7", "form": "commitment"})];
    let (_, d) = run(ASK, Kind::Person, &[], &existing, Some(&m));
    let d = d.unwrap();
    assert!(d.drafts.is_empty() && !d.failed);
}

#[test]
fn the_engine_sets_statuses_and_parties() {
    // An old code from the model is expired; a feed follows UNREAD.
    let raw = "From: Bank <no-reply@bank.example>\nList-Id: <b.example>\nSubject: News and a code\n\nRead the news. Code 7731.\n";
    let items = json!([
        {"form": "signal", "title": "Code", "quote": "Code 7731", "kind": "code", "code": "7731",
         "expires": "2000-01-01T00:00:00Z"},
        {"form": "feed", "title": "News", "quote": "Read the news."}
    ]);
    let m = Fake::with(vec![answer(items)]);
    let (_, d) = run(raw, Kind::Service, &["UNREAD"], &[], Some(&m));
    let d = d.unwrap();
    assert_eq!(d.drafts[0].item["status"], "expired");
    assert_eq!(d.drafts[1].item["status"], "new");
    assert_eq!(d.drafts[1].item["list"], "<b.example>");
    // A promise in sent mail: the other party is the first recipient.
    let sent = "From: owner@example.com\nTo: Ana <ana@example.org>\nSubject: Slides\n\nI will send the slides by Friday.\n";
    let promise = json!([{"form": "commitment", "title": "Send", "quote": "I will send the slides",
                          "direction": "owed_to_you", "kind": "task"}]);
    let m = Fake::with(vec![answer(promise)]);
    let (_, d) = run(sent, Kind::SelfOwner, &["SENT"], &[], Some(&m));
    let d = d.unwrap();
    let c = &d.drafts[0].item;
    assert_eq!(c["owner"]["address"], "ana@example.org");
    assert_eq!(c["counterparty"], json!({"kind": "self"}));
}

/// For any JSON answer, `prompt::read` does not panic, and each draft that
/// it gives has a quote from the mail, a form of the open model, and no
/// field that only the engine sets.
#[test]
fn read_holds_for_generated_answers() {
    let none = |_: &str| Vec::new();
    let labels: Vec<String> = Vec::new();
    let cx = Context {
        me: ME,
        kind: Kind::Person,
        labels: &labels,
        internal_date: Some(NOW),
        now_ms: NOW,
        known_person: &none,
        existing: &[],
        thread: &[],
        same_sender: &[],
    };
    let a = analyze(ASK.replace('\n', "\r\n").as_bytes(), &cx);
    let keys = [
        "form",
        "title",
        "quote",
        "direction",
        "kind",
        "id",
        "derived",
        "sources",
        "same_as",
        "owner",
        "code",
        "expires",
        "type",
        "fields",
        "status",
    ];
    let values = [
        json!("commitment"),
        json!("record"),
        json!("feed"),
        json!("signal"),
        json!("conversation"),
        json!("Can you send the slides"),
        json!("made up"),
        json!(""),
        json!(null),
        json!(7),
        json!({"kind": "self"}),
        json!({"address": "OWNER@example.com"}),
        json!([1, 2]),
        json!("you_owe"),
    ];
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = |n: usize| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % n as u64) as usize
    };
    let mut some = 0;
    for _ in 0..5_000 {
        let mut items = Vec::new();
        for _ in 0..next(4) {
            let mut o = serde_json::Map::new();
            for _ in 0..next(8) {
                o.insert(
                    keys[next(keys.len())].into(),
                    values[next(values.len())].clone(),
                );
            }
            items.push(Value::Object(o));
        }
        let mut ans = json!({"items": items});
        if next(10) == 0 {
            ans["phishing"] = values[next(values.len())].clone();
        }
        let Some((drafts, phishing)) = prompt::read(&ans, &a, &cx) else {
            continue;
        };
        some += 1;
        if phishing {
            assert!(drafts.is_empty());
        }
        for d in drafts {
            assert!(
                "can you send the slides by friday?".contains(&d.quote.to_lowercase()),
                "{d:?}"
            );
            assert!(
                ["commitment", "record", "feed", "signal"].contains(&d.form()),
                "{d:?}"
            );
            for k in ["id", "derived", "sources", "same_as"] {
                assert!(d.item.get(k).is_none(), "{d:?}");
            }
        }
    }
    assert!(some > 0);
}
