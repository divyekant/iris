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
            .unwrap_or_else(|| Ok(text(r#"{"items": [], "phishing": false}"#)))
    }
}

fn text(t: &str) -> Reply {
    Reply {
        text: t.to_string(),
        usage: Usage {
            input_tokens: 100,
            output_tokens: 10,
            cost_micro_usd: Some(5),
            known: true,
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
        known: &[],
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
    assert!(d.usage.known, "a known reply gives a known sum");
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
    assert!(d.usage.known);
}

#[test]
fn a_call_with_no_usage_makes_the_sum_unknown() {
    let mut first = text("not json");
    first.usage.known = false;
    let m = Fake::with(vec![Ok(first), answer(task())]);
    let (_, d) = run(ASK, Kind::Person, &[], &[], Some(&m));
    assert!(!d.unwrap().usage.known);
    let m = Fake::with(vec![
        Err(iris_model::Error::BadAnswer("x".into())),
        answer(task()),
    ]);
    let (_, d) = run(ASK, Kind::Person, &[], &[], Some(&m));
    assert!(!d.unwrap().usage.known);
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
    let m = Fake::with(vec![Ok(text(r#"{"items": [], "phishing": true}"#))]);
    let (_, d) = run(ASK, Kind::Stranger, &[], &[], Some(&m));
    let d = d.unwrap();
    assert!(d.phishing && d.hostile && d.drafts.is_empty());
    // Phishing with items is not clear: one more try, and no mark.
    let both = Ok(text(r#"{"items": [{"form": "x"}], "phishing": true}"#));
    let m = Fake::with(vec![both, answer(task())]);
    let d = run(ASK, Kind::Stranger, &[], &[], Some(&m)).1.unwrap();
    assert_eq!((d.calls, d.hostile, d.drafts.len()), (2, false, 1));
}

#[test]
fn a_model_error_is_an_error_and_a_refusal_fails_the_try() {
    // An error that a new try can fix, or a refused key: no try counts.
    for e in [
        ModelError::Transport("lost".into()),
        ModelError::Rejected {
            status: 401,
            message: "key".into(),
        },
    ] {
        let m = Fake::with(vec![Err(e)]);
        assert!(run(ASK, Kind::Person, &[], &[], Some(&m)).1.is_err());
    }
    // A refusal, or a call refused for this data: a failed try, one call.
    let mut no = text("");
    no.stop = Stop::Refused;
    let big = Err(ModelError::Rejected {
        status: 413,
        message: "large".into(),
    });
    for r in [Ok(no), big] {
        let m = Fake::with(vec![r, answer(task())]);
        let d = run(ASK, Kind::Person, &[], &[], Some(&m)).1.unwrap();
        assert_eq!((d.calls, d.drafts.len(), d.failed), (1, 0, true));
    }
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

#[test]
fn an_answer_without_phishing_or_with_a_party_with_no_address_gets_one_more_try() {
    let no_phishing = Ok(text(r#"{"items": []}"#));
    let m = Fake::with(vec![no_phishing, answer(task())]);
    let (_, d) = run(ASK, Kind::Person, &[], &[], Some(&m));
    assert_eq!(d.unwrap().calls, 2);
    let mut bad = task();
    bad[0]["counterparty"] = json!({"kind": "sender"});
    let list = json!([{"form": "record", "type": "contract", "title": "C", "quote": "Can you send the slides",
                       "confidence": 0.5, "fields": {"name": "C", "parties": [{"name": "No address"}]}}]);
    let m = Fake::with(vec![answer(bad), answer(list)]);
    let (_, d) = run(ASK, Kind::Person, &[], &[], Some(&m));
    let d = d.unwrap();
    assert_eq!((d.calls, d.drafts.len(), d.failed), (2, 0, true));
}

#[test]
fn same_as_needs_a_valid_item_of_the_same_form() {
    let existing = [json!({"id": "c-7", "form": "commitment", "title": "Send the slides"})];
    let mut same = task();
    same[0]["same_as"] = json!("c-7");
    // A bare reference, or a record that names a commitment: not valid.
    let bare = json!([{"same_as": "c-7"}]);
    let other = json!([{"form": "record", "type": "other", "title": "S", "quote": "Can you send the slides",
                        "fields": {"summary": "S"}, "same_as": "c-7"}]);
    let m = Fake::with(vec![answer(bare), answer(other)]);
    let d = run(ASK, Kind::Person, &[], &existing, Some(&m)).1.unwrap();
    assert_eq!((d.calls, d.failed), (2, true));
    // A valid item of the same form adds nothing. An unknown ID keeps it.
    let m = Fake::with(vec![answer(same.clone())]);
    let d = run(ASK, Kind::Person, &[], &existing, Some(&m)).1.unwrap();
    assert_eq!((d.calls, d.drafts.len(), d.failed), (1, 0, false));
    same[0]["same_as"] = json!("c-99");
    let m = Fake::with(vec![answer(same)]);
    let d = run(ASK, Kind::Person, &[], &existing, Some(&m)).1.unwrap();
    assert_eq!(d.drafts.len(), 1);
}

#[test]
fn the_model_gets_the_known_parties() {
    let raw = "From: Ana <ana@example.org>\nTo: owner@example.com\nSubject: Slides\n\nSam will send you the slides.\n";
    let known = ["Sam Roe <sam@example.org>".to_string(), "<<<x".to_string()];
    let none = |_: &str| Vec::new();
    let cx = Context {
        me: ME,
        kind: Kind::Person,
        labels: &[],
        internal_date: Some(NOW),
        now_ms: NOW,
        known_person: &none,
        existing: &[],
        thread: &[],
        same_sender: &[],
        known: &known,
    };
    let a = analyze(raw.as_bytes(), &cx);
    let data = prompt::data(&a, &cx);
    assert!(
        data.contains("Known party: Sam Roe <sam@example.org>\n"),
        "{data}"
    );
    assert!(!data.contains("<<<"));
    // The model names the known party: it is the other side.
    let owed = json!([{"form": "commitment", "title": "Slides", "quote": "Sam will send you the slides.",
                       "direction": "owed_to_you", "kind": "task", "owner": {"address": "sam@example.org", "name": "Sam Roe"}}]);
    let m = Fake::with(vec![answer(owed)]);
    let d = decide(&a, &cx, Some(&m as &dyn Model)).unwrap();
    assert_eq!(d.drafts[0].item["owner"]["address"], "sam@example.org");
    assert_eq!(d.drafts[0].item["counterparty"], json!({"kind": "self"}));
}

#[test]
fn a_broken_phishing_answer_or_a_self_party_with_another_address_gets_one_more_try() {
    let items: Vec<Value> = (0..21).map(|_| json!({})).collect();
    let broken = Ok(text(&json!({"items": items, "phishing": true}).to_string()));
    let m = Fake::with(vec![broken, answer(task())]);
    let d = run(ASK, Kind::Person, &[], &[], Some(&m)).1.unwrap();
    assert_eq!((d.calls, d.hostile, d.drafts.len()), (2, false, 1));
    for (dir, key) in [("you_owe", "counterparty"), ("owed_to_you", "owner")] {
        let mut t = task();
        t[0]["direction"] = json!(dir);
        t[0][key] = json!({"kind": "self", "address": "sam@example.org"});
        let m = Fake::with(vec![answer(t.clone()), answer(t)]);
        let d = run(ASK, Kind::Person, &[], &[], Some(&m)).1.unwrap();
        assert_eq!((d.calls, d.drafts.len(), d.failed), (2, 0, true), "{dir}");
    }
}

#[test]
fn a_list_message_gives_the_list_headers_to_the_model() {
    let raw = "From: Ana <ana@shop.example>\nList-Id: <n.shop.example>\nList-Unsubscribe: <https://shop.example/u>\nSubject: Hello\n\nHello.\n";
    let m = Fake::with(vec![answer(json!([]))]);
    let (_, d) = run(raw, Kind::Service, &[], &[], Some(&m));
    assert_eq!(d.unwrap().calls, 1);
    let data = &m.calls.borrow()[0].1;
    assert!(data.contains("List-Id: <n.shop.example>\n"), "{data}");
    assert!(data.contains("List-Unsubscribe: yes\n"), "{data}");
}

#[test]
fn sent_mail_from_an_alias_or_to_bcc_only_keeps_the_owner_and_the_model_parties() {
    // Sent mail from an alias: the issuer of a record is the owner.
    let alias = "From: Me <me@alias.example>\nTo: Ana <ana@example.org>\nSubject: Note\n\nThe summary of the plan.\n";
    let record = json!([{"form": "record", "type": "other", "title": "Plan", "quote": "The summary of the plan.",
                         "fields": {"summary": "The plan"}}]);
    let m = Fake::with(vec![answer(record)]);
    let (_, d) = run(alias, Kind::SelfOwner, &["SENT"], &[], Some(&m));
    let d = d.unwrap();
    assert_eq!(d.drafts.len(), 1, "{:?}", d);
    assert_eq!(d.drafts[0].item["issuer"], json!({"kind": "self"}));
    // The model cannot set the fields that the headers give.
    let feed = "From: News <news@shop.example>\nList-Id: <n.shop.example>\nSubject: News\n\nRead the news.\n";
    let items = json!([
        {"form": "feed", "title": "News", "quote": "Read the news.", "published": "2000-01-01T00:00:00Z",
         "list": "<other.example>", "publisher": {"address": "x@evil.example"}},
        {"form": "record", "type": "other", "title": "N", "quote": "Read the news.",
         "fields": {"summary": "N"}, "issuer": {"address": "x@evil.example"}}
    ]);
    let m = Fake::with(vec![answer(items)]);
    let (_, d) = run(feed, Kind::Service, &[], &[], Some(&m));
    let d = d.unwrap();
    assert_eq!(d.drafts.len(), 2, "{d:?}");
    let f = &d.drafts[0].item;
    assert_eq!(f["list"], "<n.shop.example>");
    assert_eq!(f["publisher"]["address"], "news@shop.example");
    assert_ne!(f["published"], "2000-01-01T00:00:00Z");
    assert_eq!(d.drafts[1].item["issuer"]["address"], "news@shop.example");
    let record = json!([{"form": "record", "type": "other", "title": "Plan", "quote": "The summary of the plan.",
                         "fields": {"summary": "The plan"}, "issuer": {"address": "me@alias.example"}}]);
    let m = Fake::with(vec![answer(record)]);
    let (_, d) = run(alias, Kind::SelfOwner, &["SENT"], &[], Some(&m));
    assert_eq!(d.unwrap().drafts[0].item["issuer"], json!({"kind": "self"}));
    // Sent mail with Bcc only: the Bcc recipient is the other party.
    let bcc = "From: owner@example.com\nBcc: Bo <bo@example.org>\nSubject: Slides\n\nI will send the slides by Friday.\n";
    let promise = json!([{"form": "commitment", "title": "Send", "quote": "I will send the slides",
                          "direction": "you_owe", "kind": "task"}]);
    let m = Fake::with(vec![answer(promise.clone())]);
    let (_, d) = run(bcc, Kind::SelfOwner, &["SENT"], &[], Some(&m));
    let d = d.unwrap();
    assert_eq!((d.calls, d.drafts.len()), (1, 1));
    let c = &d.drafts[0].item;
    assert_eq!(c["owner"], json!({"kind": "self"}));
    assert_eq!(c["counterparty"]["address"], "bo@example.org");
    assert!(m.calls.borrow()[0].1.contains("Bcc: bo@example.org\n"));
    // The model chooses the other side, also a person who is not a
    // recipient. The engine fills it only when the model gives none: the
    // first recipient that is not an owner address (the alias in From too).
    let alias = "From: me@alias.example\nTo: me@alias.example, Owner@example.com, ana@example.org\nSubject: Slides\n\nAlice will send the report.\n";
    let base = json!([{"form": "commitment", "title": "Report", "quote": "Alice will send the report.",
                       "direction": "owed_to_you", "kind": "task"}]);
    let cases = [
        (json!({"address": "alice@example.net"}), "alice@example.net"),
        (json!({"address": "ana@example.org"}), "ana@example.org"),
        (json!({"kind": "self"}), "ana@example.org"),
        (json!({"address": "me@alias.example"}), "ana@example.org"),
    ];
    for (pick, want) in cases {
        let mut p = base.clone();
        p[0]["owner"] = pick.clone();
        let m = Fake::with(vec![answer(p)]);
        let d = run(alias, Kind::SelfOwner, &["SENT"], &[], Some(&m))
            .1
            .unwrap();
        let c = &d.drafts[0].item;
        assert_eq!(c["owner"]["address"], want, "{pick}");
        assert_eq!(c["counterparty"], json!({"kind": "self"}));
    }
    // Mail to owner addresses only, with no party from the model: no item.
    let only_me = "From: me@alias.example\nTo: owner@example.com\nSubject: Slides\n\nI will send the slides by Friday.\n";
    let m = Fake::with(vec![answer(promise.clone()), answer(promise.clone())]);
    let d = run(only_me, Kind::SelfOwner, &["SENT"], &[], Some(&m))
        .1
        .unwrap();
    assert_eq!((d.drafts.len(), d.failed), (0, true));
    // Mail from the mailbox address with no SENT label: the sender is the
    // owner, so no other side and no item.
    let self_note = "From: owner@example.com\nTo: owner@example.com\nSubject: Note\n\nI will send the slides by Friday.\n";
    let m = Fake::with(vec![answer(promise.clone()), answer(promise.clone())]);
    let d = run(self_note, Kind::Person, &[], &[], Some(&m)).1.unwrap();
    assert_eq!((d.drafts.len(), d.failed), (0, true));
    // In received mail too, the other side is never the owner: the sender
    // takes the place of a model party with no address.
    for (dir, key) in [("you_owe", "counterparty"), ("owed_to_you", "owner")] {
        let mut t = task();
        t[0]["direction"] = json!(dir);
        t[0][key] = json!({"kind": "self"});
        let m = Fake::with(vec![answer(t)]);
        let d = run(ASK, Kind::Person, &[], &[], Some(&m)).1.unwrap();
        assert_eq!(d.drafts[0].item[key]["address"], "ana@example.org", "{dir}");
    }
    // With no recipient at all, only the parties of the model count.
    let none = "From: owner@example.com\nSubject: Slides\n\nI will send the slides by Friday.\n";
    let mut named = promise;
    named[0]["counterparty"] = json!({"address": "cy@example.org"});
    let m = Fake::with(vec![answer(named)]);
    let d = run(none, Kind::SelfOwner, &["SENT"], &[], Some(&m))
        .1
        .unwrap();
    assert_eq!(
        d.drafts[0].item["counterparty"]["address"],
        "cy@example.org"
    );
}

/// The data of one model call stays within `prompt::MAX_DATA` for the
/// largest inputs, and it always holds the whole clean text of the message.
#[test]
fn the_model_data_stays_within_its_budget() {
    let body = "x".repeat(iris_clean::TEXT_LIMIT - 100);
    let raw = format!(
        "From: {name} <a@example.org>\nTo: {to}\nSubject: {subject}\n\n{body}\n",
        name = "N".repeat(5000),
        to = (0..500)
            .map(|i| format!("p{i}@example.org"))
            .collect::<Vec<_>>()
            .join(", "),
        subject = "S".repeat(10_000),
    );
    let thread: Vec<String> = (0..50)
        .map(|i| format!("{i}{}", "é".repeat(10_000)))
        .collect();
    let existing: Vec<Value> = (0..200)
        .map(|i| json!({"id": format!("c-{i}"), "title": "t".repeat(400)}))
        .collect();
    let none = |_: &str| Vec::new();
    let cx = Context {
        me: ME,
        kind: Kind::Person,
        labels: &[],
        internal_date: Some(NOW),
        now_ms: NOW,
        known_person: &none,
        existing: &existing,
        thread: &thread,
        same_sender: &[],
        known: &[],
    };
    let mut a = analyze(raw.as_bytes(), &cx);
    // Clean text of the most characters, in 4-byte characters: 128,000 bytes.
    let wide = "😀".repeat(iris_clean::TEXT_LIMIT);
    let text = std::mem::replace(&mut a.text.text, wide);
    let data = prompt::data(&a, &cx);
    assert!(data.len() <= prompt::MAX_DATA, "{}", data.len());
    assert!(data.ends_with("😀\n[cut]"), "{}", &data[data.len() - 20..]);
    a.text.text = text;
    a.text.attachment_text = vec![("big.pdf".into(), "ü".repeat(600_000))];
    let data = prompt::data(&a, &cx);
    assert!(data.len() <= prompt::MAX_DATA, "{}", data.len());
    assert!(data.contains(&body));
    // The block that the client sends stays in the budget, also with `<<<`
    // in each text from the mail.
    for k in 0..4 {
        let name = format!("{}{k}.txt", "<<<".repeat(80));
        a.text.attachment_text = vec![(name.clone(), "<<<".repeat(300_000)); k + 1];
        a.text.attachments = vec![(name, "text/plain".into(), 1); 20];
        let data = prompt::data(&a, &cx);
        let block = iris_model::data_block(&data);
        assert!(block.len() <= prompt::MAX_DATA, "{}", block.len());
        let wrap = 2 + iris_model::DATA_START.len() + iris_model::DATA_END.len();
        assert_eq!(block.len(), data.len() + wrap);
    }
    a.text.attachments.clear();
    a.text.attachment_text = vec![("x.txt".into(), "<<<".repeat(300_000))];
    let data = prompt::data(&a, &cx);
    // Attachment text comes before the thread.
    assert!(data.contains("[cut]") && !data.contains("Earlier message"));
    // Without attachment text, the newest earlier messages fill the rest.
    a.text.attachment_text.clear();
    let data = prompt::data(&a, &cx);
    assert!(data.len() <= prompt::MAX_DATA, "{}", data.len());
    assert!(data.contains("Earlier message in the thread:\n49"));
    assert!(!data.contains("Earlier message in the thread:\n0"));
    // With small context, nothing is cut.
    let small = &thread[..2];
    let cx = Context {
        thread: small,
        existing: &existing[..2],
        ..cx
    };
    a.text.attachment_text = vec![("a.pdf".into(), "Total 5.00".into())];
    let data = prompt::data(&a, &cx);
    // Only the long header values (From, To, Subject) are cut.
    assert_eq!(data.matches("[cut]").count(), 3);
    assert!(data.contains(&thread[0]) && data.contains(&thread[1]) && data.contains("c-1"));
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
        known: &[],
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
        let mut ans = json!({"items": items, "phishing": false});
        match next(10) {
            0 => ans["phishing"] = values[next(values.len())].clone(),
            1 => drop(ans.as_object_mut().unwrap().remove("phishing")),
            _ => {}
        }
        let Some((drafts, phishing)) = prompt::read(&ans, &a, &cx) else {
            continue;
        };
        // An answer is valid only with a boolean `phishing`.
        assert!(ans["phishing"].is_boolean(), "{ans}");
        some += 1;
        if phishing {
            assert!(drafts.is_empty() && ans["items"] == json!([]), "{ans}");
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

/// R3-06: the model can tell an ask that you send from an ask to you. The
/// data of sent mail names the sender kind `self`, and the instruction says
/// what that line means for the direction of a commitment.
#[test]
fn the_instruction_explains_the_sender_kind_of_sent_mail() {
    let none = |_: &str| Vec::new();
    let labels = ["SENT".to_string()];
    let cx = Context {
        me: ME,
        kind: Kind::SelfOwner,
        labels: &labels,
        internal_date: Some(NOW),
        now_ms: NOW,
        known_person: &none,
        existing: &[],
        thread: &[],
        same_sender: &[],
        known: &[],
    };
    let raw = format!("From: {ME}\nTo: ana@example.org\nSubject: Slides\n\nCan you send me the slides by Friday?\n");
    let a = analyze(raw.as_bytes(), &cx);
    let line = format!("Sender kind: {}", Kind::SelfOwner.name());
    assert!(prompt::data(&a, &cx).contains(&format!("{line}\n")));
    let rule = INSTRUCTION
        .lines()
        .find(|l| l.contains(&format!("\"{line}\"")))
        .expect("the instruction names the sender kind of sent mail");
    assert!(
        rule.contains("owed_to_you") && rule.contains("you_owe"),
        "{rule}"
    );
}
