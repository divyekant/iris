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
fn list_mail_goes_to_the_model() {
    // The list rule is out of the fixed rules (issue #96): a list
    // message can hold a request, a record or a code in any form.
    assert!(drafts(LIST, Kind::Service, &["UNREAD"]).is_none());
    assert!(drafts(LIST, Kind::Stranger, &[]).is_none());
}

#[test]
fn a_rule_item_that_fails_the_schema_goes_to_the_model() {
    let raw = "From: Bank <no-reply@bank.example>\nSubject: Sign in\n\nYour code: 482913\n";
    assert_eq!(one(raw, Kind::Service, &[]).item["code"], "482913");
    // An address that the schema refuses as the issuer.
    let bad = raw.replace("no-reply@bank.example", "a@@b.example");
    assert!(drafts(&bad, Kind::Service, &[]).is_none());
}

#[test]
fn a_receipt_word_late_in_the_mail_goes_to_the_model() {
    let late = format!("{}{}\nTotal: 12.50 USD\n", CODE, "Line.\n".repeat(200));
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
    let raw =
        "From: Bank <no-reply@bank.example>\nSubject: Your code 482913\n\nUse it to sign in now.\n";
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
To: Owner <Owner@Example.com>
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
fn a_calendar_request_goes_to_the_model() {
    // The fixed invitation rule is out of the fixed rules (issue #97).
    assert!(drafts(INVITE, Kind::Person, &[]).is_none());
    assert!(drafts(INVITE, Kind::Service, &[]).is_none());
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
fn another_number_on_a_code_line_goes_to_the_model() {
    // A year or an old code on a line with a code word makes the code not
    // clear (spec 1 section 7.3).
    for line in ["Verification help updated 2026", "Your old code was 5678"] {
        let raw = format!(
            "From: Bank <no-reply@bank.example>\nSubject: Your verification code\n\n7731\n{line}\n"
        );
        assert!(drafts(&raw, Kind::Service, &[]).is_none(), "{line}");
    }
    // The same code again is no other number.
    let same = "From: Bank <no-reply@bank.example>\nSubject: Your verification code\n\n7731\nCode 7731 ends in 10 minutes.\n";
    assert_eq!(one(same, Kind::Service, &[]).item["code"], "7731");
    // A second bare number after the code line makes the code not clear,
    // with another value or the same value (#100).
    for second in ["5678", "1234", "12", "123456789", "123 456"] {
        let two = format!(
            "From: Bank <no-reply@bank.example>\nSubject: Sign in\n\nYour code:\n1234\n{second}\n"
        );
        assert!(drafts(&two, Kind::Service, &[]).is_none(), "{second}");
    }
}

/// For generated lines with one planted code, the rule finds that code. A
/// line with a code word and another number makes the code not clear.
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
        // A later line with a code word and another number.
        let other = next(2) == 0;
        if other {
            lines.push("Code help updated 2026".to_string());
        }
        for (k, l) in planted.into_iter().enumerate() {
            lines.insert(at + k, l);
        }
        let raw = format!(
            "From: Bank <no-reply@bank.example>\nSubject: Sign in\n\n{}\n",
            lines.join("\n")
        );
        if other {
            assert!(drafts(&raw, Kind::Service, &[]).is_none(), "{raw:?}");
        } else {
            let d = one(&raw, Kind::Service, &[]);
            assert_eq!(d.item["code"], code.as_str(), "{raw:?}");
        }
    }
}

#[test]
fn a_year_is_not_a_code() {
    // A sign-in message with no clear code goes to the model, also with a
    // list header.
    let raw = "From: News <news@shop.example>\nList-Id: <n.example>\nSubject: Verification in 2026\n\nRead.\n";
    assert!(drafts(raw, Kind::Service, &[]).is_none());
}

#[test]
fn a_code_late_in_a_long_line_keeps_its_quote() {
    let raw = format!(
        "From: Bank <no-reply@bank.example>\nSubject: Sign in\n\n{} your code is 482913 now\n",
        "word ".repeat(200)
    );
    let d = one(&raw, Kind::Service, &[]);
    assert_eq!(d.item["code"], "482913");
    assert!(
        d.quote.contains("482913") && d.quote.chars().count() <= 500,
        "{}",
        d.quote
    );
}

/// A sign-in message that gives a code signal. Each test below adds one
/// sign of other content, and the message goes to the model.
const CODE: &str = "From: Bank <no-reply@bank.example>\nSubject: Sign in\n\nYour code: 482913\n";

fn with_line(line: &str) -> String {
    format!("{CODE}{line}\n")
}

#[test]
fn the_code_base_gives_a_signal() {
    assert_eq!(one(CODE, Kind::Service, &[]).item["code"], "482913");
}

#[test]
fn a_year_shaped_code_bound_to_its_word_counts() {
    let raw = CODE.replace("Your code: 482913", "Your verification code is 2026");
    assert_eq!(one(&raw, Kind::Service, &[]).item["code"], "2026");
}

#[test]
fn a_second_code_value_goes_to_the_model() {
    assert!(drafts(&with_line("Your code: 5678"), Kind::Service, &[]).is_none());
}

#[test]
fn a_long_line_of_one_code_is_read_in_linear_time() {
    // 25 KB: inside the limit of the clean text, so that the search runs.
    let line = "code 1234 ".repeat(2_500);
    let raw = format!("From: Bank <no-reply@bank.example>\nSubject: Sign in\n\n{line}\n");
    let start = std::time::Instant::now();
    let d = one(&raw, Kind::Service, &[]);
    assert_eq!(d.item["code"], "1234");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn a_long_line_of_codes_is_read_in_linear_time() {
    // 25 KB: inside the limit of the clean text, so that the search runs.
    let line = "code 1234 code 5678 ".repeat(1_250);
    let raw = format!("From: Bank <no-reply@bank.example>\nSubject: Sign in\n\n{line}\n");
    let start = std::time::Instant::now();
    assert!(drafts(&raw, Kind::Service, &[]).is_none());
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn signs_of_other_content_send_a_code_to_the_model() {
    for line in [
        "Pay $42 by Friday.",
        "Invoice 77. Total: 12.50 USD",
        "Flight itinerary: reservation ABC123 for 12 Dec.",
        "Shop our sale.",
    ] {
        assert!(
            drafts(&with_line(line), Kind::Service, &[]).is_none(),
            "{line}"
        );
    }
    // A list header: list mail goes to the model (issue #96).
    let list = format!("List-Id: <bank.example>\n{CODE}");
    assert!(drafts(&list, Kind::Service, &[]).is_none());
    // A promotion code with a sign-in word.
    let promo = "From: Shop <deals@shop.example>\nSubject: Your discount code\n\nUse code 1234 at checkout. Log in to see more.\n";
    assert!(drafts(promo, Kind::Service, &[]).is_none());
}

#[test]
fn a_file_or_a_calendar_request_sends_a_code_to_the_model() {
    let head = "From: Bank <no-reply@bank.example>\nSubject: Sign in\nMIME-Version: 1.0\nContent-Type: multipart/mixed; boundary=a\n\n--a\nContent-Type: text/plain\n\nYour code: 482913\n";
    let plain = format!("{head}--a--\n");
    assert_eq!(one(&plain, Kind::Service, &[]).item["code"], "482913");
    // Any file: a file name or a file without record words can hold a record.
    let file = format!("{head}--a\nContent-Type: text/plain; name=\"doc.txt\"\nContent-Disposition: attachment; filename=\"doc.txt\"\n\nOrder 73, $42\n--a--\n");
    assert!(drafts(&file, Kind::Service, &[]).is_none());
    // A calendar request: an invitation for the model (issue #97).
    let cal = format!("{head}--a\nContent-Type: text/calendar; method=REQUEST\n\nBEGIN:VCALENDAR\nMETHOD:REQUEST\nEND:VCALENDAR\n--a--\n");
    assert!(drafts(&cal, Kind::Service, &[]).is_none());
}

#[test]
fn a_cut_text_sends_a_code_to_the_model() {
    // A record after the limit of the clean text.
    let long = with_line(&format!("{}\nInvoice 77.", "Thanks. ".repeat(5_000)));
    assert!(iris_understand::Text::of(long.replace('\n', "\r\n").as_bytes()).cut);
    assert!(drafts(&long, Kind::Service, &[]).is_none());
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
