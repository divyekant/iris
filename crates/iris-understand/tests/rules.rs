//! Step 6, the fixed rules of specification 1 section 7.3, and step 8, the
//! schema check, on hand-written messages. Pure functions: no store, no model.

use iris_understand::{analyze, rules, validate, By, Context, Draft, Kind};
use serde_json::json;

const NOW: i64 = 1_767_225_600_000;

fn drafts(raw: &str, kind: Kind, labels: &[&str]) -> Option<Vec<Draft>> {
    let labels: Vec<String> = labels.iter().map(|l| l.to_string()).collect();
    let none = |_: &str| Vec::new();
    let cx = Context {
        me: "owner@example.com",
        kind,
        labels: &labels,
        internal_date: Some(NOW - 60_000),
        now_ms: NOW,
        known_person: &none,
        existing: &[],
        thread: &[],
        same_sender: &[],
    };
    let raw = raw.replace('\n', "\r\n");
    let a = analyze(raw.as_bytes(), &cx);
    rules::apply(&a, &cx)
}

/// The draft as a whole item of the open model, with the fields that the
/// store adds, so that the schema check sees a complete item.
fn finished(d: &Draft) -> serde_json::Value {
    let mut v = d.item.clone();
    for key in ["owner", "counterparty", "issuer", "publisher"] {
        if v[key]["kind"] == "sender" {
            v[key]["sender"] = json!("p-1");
        }
    }
    let By::Rule(rule) = d.by else {
        panic!("not a rule")
    };
    v["model"] = json!("iris/0");
    v["id"] = json!(format!("{}-1", &d.form()[..1]));
    v["sources"] = json!([{"message": "m-1", "thread": "t-1", "role": "origin", "quote": d.quote}]);
    v["created"] = json!("2026-01-01T00:00:00Z");
    v["updated"] = json!("2026-01-01T00:00:00Z");
    v["derived"] = json!({"by": "rule", "rule": rule});
    v
}

/// One draft that is valid and whose quote occurs in the message.
fn one(raw: &str, kind: Kind, labels: &[&str]) -> Draft {
    let mut d = drafts(raw, kind, labels).expect("a rule matches");
    assert_eq!(d.len(), 1);
    let d = d.remove(0);
    assert_eq!(validate(&finished(&d)), Ok(()), "{:?}", d.item);
    assert!(
        d.quote == "(no subject)" || raw.contains(&d.quote),
        "{:?}",
        d.quote
    );
    d
}

const LIST: &str = "From: Mail Weekly <news@weekly.example>
List-Id: <weekly.news.example>
Subject: This week in mail standards

Read the news.
";

#[test]
fn a_list_from_a_service_is_a_feed() {
    let d = one(LIST, Kind::Service, &["UNREAD"]);
    assert_eq!(d.by, By::Rule("list"));
    assert_eq!(d.item["form"], "feed");
    assert_eq!(d.item["status"], "new");
    assert_eq!(d.item["list"], "<weekly.news.example>");
    assert_eq!(d.item["published"], "2025-12-31T23:59:00Z");
    // The status follows the UNREAD label.
    assert_eq!(one(LIST, Kind::Service, &[]).item["status"], "read");
    // The same list from a stranger goes to the model.
    assert!(drafts(LIST, Kind::Stranger, &[]).is_none());
}

#[test]
fn a_receipt_word_late_in_a_list_mail_goes_to_the_model() {
    let late = format!("{}{}\nTotal: 12.50 USD\n", LIST, "Line.\n".repeat(200));
    assert!(drafts(&late, Kind::Service, &[]).is_none());
}

#[test]
fn a_code_from_a_service_is_a_signal() {
    let raw = "From: Bank <no-reply@bank.example>
Subject: Sign in

Your verification code:
  7731
It expires soon.
";
    let d = one(raw, Kind::Service, &[]);
    assert_eq!(d.item["form"], "signal");
    assert_eq!(d.item["code"], "7731");
    assert_eq!(d.item["status"], "active");
    assert_eq!(d.quote, "7731");
    // A barcode or a code from a person is no code signal.
    assert!(drafts(
        &raw.replace("verification code", "barcode"),
        Kind::Service,
        &[]
    )
    .is_none());
    assert!(drafts(raw, Kind::Person, &[]).is_none());
}

#[test]
fn an_old_code_is_expired() {
    let raw = "From: Bank <no-reply@bank.example>\nSubject: Your code 482913\n\nUse it now.\n";
    let labels: Vec<String> = Vec::new();
    let none = |_: &str| Vec::new();
    let cx = Context {
        me: "owner@example.com",
        kind: Kind::Service,
        labels: &labels,
        internal_date: Some(NOW - 16 * 60 * 1000),
        now_ms: NOW,
        known_person: &none,
        existing: &[],
        thread: &[],
        same_sender: &[],
    };
    let a = analyze(raw.replace('\n', "\r\n").as_bytes(), &cx);
    let d = rules::apply(&a, &cx).unwrap().remove(0);
    assert_eq!(d.item["status"], "expired");
    assert_eq!(validate(&finished(&d)), Ok(()));
}

const INVITE: &str = "From: Ana Lee <ana@example.org>
Subject: Planning
Content-Type: multipart/alternative; boundary=a

--a
Content-Type: text/plain

Join us.
--a
Content-Type: text/calendar; method=REQUEST

BEGIN:VCALENDAR
METHOD:REQUEST
END:VCALENDAR
--a--
";

#[test]
fn a_calendar_request_is_an_invitation() {
    let d = one(INVITE, Kind::Person, &[]);
    assert_eq!(d.by, By::Rule("invitation"));
    assert_eq!(d.item["kind"], "invitation");
    assert_eq!(d.item["direction"], "you_owe");
    assert_eq!(d.item["title"], "Invitation: Planning");
    assert_eq!(d.quote, "Planning");
    // Your own invitation is not one that you owe.
    assert!(drafts(INVITE, Kind::SelfOwner, &[]).is_none());
}

#[test]
fn a_rule_quote_is_the_matched_line() {
    // No subject: the quote and the title say "(no subject)" (spec 1 section 7.5).
    let raw = INVITE.replace("Subject: Planning\n", "");
    let d = one(&raw, Kind::Person, &[]);
    assert_eq!(d.item["title"], "Invitation: (no subject)");
    assert_eq!(d.quote, "(no subject)");
    let bare = "From: news@weekly.example\nList-Id: <w.example>\n\n";
    assert_eq!(one(bare, Kind::Service, &[]).quote, "(no subject)");
}

#[test]
fn hostile_mail_matches_the_hostile_rule() {
    // A hostile mark (the SPAM label) or a hostile sender: no item, and no
    // model call (Some, not None).
    for (kind, labels) in [(Kind::Person, &["SPAM"][..]), (Kind::Hostile, &[][..])] {
        assert_eq!(drafts(INVITE, kind, labels).map(|d| d.len()), Some(0));
        assert_eq!(drafts(LIST, kind, labels).map(|d| d.len()), Some(0));
    }
}

#[test]
fn a_code_on_the_cue_line_wins_over_a_number_on_the_next_line() {
    let raw = "From: Bank <no-reply@bank.example>
Subject: Your verification code

Expires in 2026
Code: 7731
";
    let d = one(raw, Kind::Service, &[]);
    assert_eq!(d.item["code"], "7731");
    assert_eq!(d.quote, "Code: 7731");
    // A number after a cue line counts only on a line of its own.
    let date = "From: Bank <no-reply@bank.example>\nSubject: Your code\n\nExpires in 2026\n";
    assert!(drafts(date, Kind::Service, &[]).is_none());
}

#[test]
fn the_first_code_in_the_message_wins() {
    let raw = "From: Bank <no-reply@bank.example>
Subject: Your verification code

7731
Verification help updated 2026
";
    let d = one(raw, Kind::Service, &[]);
    assert_eq!(d.item["code"], "7731");
}

/// For generated lines with one planted code, the rule finds that code when
/// no earlier cue line gives a candidate.
#[test]
fn a_planted_code_is_found_in_message_order() {
    const FILL: [&str; 6] = [
        "Hello",
        "Order 123456789",
        "Total 1234.50",
        "See you",
        "x",
        "barcode 5555",
    ];
    let mut x: u64 = 0x0bad_cafe_1234_5678;
    let mut next = |n: usize| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % n as u64) as usize
    };
    for _ in 0..2000 {
        let mut lines: Vec<String> = (0..next(6))
            .map(|_| FILL[next(FILL.len())].to_string())
            .collect();
        let at = next(lines.len() + 1);
        let code = format!("{}", 1000 + next(9_000_000));
        let planted = if next(2) == 0 {
            vec![format!("Your code: {code}")]
        } else {
            vec!["Your verification code".to_string(), code.clone()]
        };
        // A later cue line with another number must not win.
        lines.push("Code help updated 2026".to_string());
        for (k, l) in planted.into_iter().enumerate() {
            lines.insert(at + k, l);
        }
        let raw = format!(
            "From: Bank <no-reply@bank.example>\nSubject: Sign in\n\n{}\n",
            lines.join("\n")
        );
        let d = one(&raw, Kind::Service, &[]);
        assert_eq!(d.item["code"], code.as_str(), "{raw:?}");
    }
}

#[test]
fn the_spec_examples_are_valid_and_a_bad_item_is_not() {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/spec/schema/examples"
    );
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        if !name.ends_with(".json") || name.starts_with("sender") {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(validate(&v), Ok(()), "{name}");
        let mut bad = v.clone();
        bad["title"] = json!("");
        assert!(validate(&bad).is_err(), "{name}");
    }
    assert!(validate(&json!({"form": "other"})).is_err());
}

/// For generated messages from a service, each rule item is valid, its quote
/// occurs in the message, and a code has 4 to 8 digits and is in its quote.
#[test]
fn rule_items_hold_for_generated_messages() {
    const PIECES: [&str; 16] = [
        "code",
        "Code:",
        " ",
        "\n",
        "1234",
        "123456789",
        "12.50",
        "1,234",
        "barcode",
        "verification",
        "one-time",
        "receipt",
        "x",
        "é",
        "\t",
        "99999",
    ];
    const HEADS: [&str; 4] = [
        "From: Bank <no-reply@bank.example>\n",
        "From: no-reply@bank.example\nList-Id: <l.example>\n",
        "List-Unsubscribe: <https://x.example/u>\n",
        "From: \"\" <a@b.example>\nList-Id: <l.example>\n",
    ];
    let mut x: u64 = 0x1234_5678_9abc_def1;
    let mut next = |n: usize| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % n as u64) as usize
    };
    for _ in 0..2000 {
        let mut subject = String::new();
        let mut body = String::new();
        for _ in 0..next(4) {
            subject.push_str(PIECES[next(PIECES.len())].trim_end_matches('\n'));
        }
        for _ in 0..next(20) {
            body.push_str(PIECES[next(PIECES.len())]);
        }
        let raw = format!("{}Subject: {subject}\n\n{body}\n", HEADS[next(HEADS.len())]);
        let Some(ds) = drafts(&raw, Kind::Service, &[]) else {
            continue;
        };
        for d in ds {
            assert_eq!(validate(&finished(&d)), Ok(()), "{raw:?} {:?}", d.item);
            assert!(
                d.quote == "(no subject)" || raw.contains(&d.quote),
                "{raw:?} {:?}",
                d.quote
            );
            if let Some(code) = d.item["code"].as_str() {
                assert!((4..=8).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_digit()));
                assert!(d.quote.contains(code), "{raw:?}");
            }
        }
    }
}
