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
fn a_year_after_the_code_does_not_compete() {
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
fn a_year_is_not_a_code_and_sign_in_mail_is_no_feed() {
    // A sign-in message with no clear code goes to the model, also with a
    // list header.
    let raw = "From: News <news@shop.example>\nList-Id: <n.example>\nSubject: Verification in 2026\n\nRead.\n";
    assert!(drafts(raw, Kind::Service, &[]).is_none());
    let news = raw.replace("Verification in 2026", "News of 2026");
    assert_eq!(one(&news, Kind::Service, &[]).item["form"], "feed");
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

#[test]
fn a_long_list_id_gives_a_feed_with_no_list() {
    let raw = LIST.replace(
        "<weekly.news.example>",
        &format!("<{}.example>", "w".repeat(600)),
    );
    let d = one(&raw, Kind::Service, &[]);
    assert_eq!(d.item["form"], "feed");
    assert!(d.item.get("list").is_none());
}

#[test]
fn a_year_shaped_code_bound_to_its_word_counts() {
    let raw = "From: Bank <no-reply@bank.example>\nList-Id: <b.example>\nSubject: Sign in\n\nYour verification code is 2026\n";
    let d = one(raw, Kind::Service, &[]);
    assert_eq!(d.item["form"], "signal");
    assert_eq!(d.item["code"], "2026");
}

#[test]
fn an_unclear_code_in_a_list_message_goes_to_the_model() {
    let raw = LIST.replace(
        "Read the news.",
        "Your sign-in code is 1234\nYour code: 5678",
    );
    assert_ne!(raw, LIST);
    assert!(drafts(&raw, Kind::Service, &[]).is_none());
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
fn a_payment_notice_in_a_list_message_goes_to_the_model() {
    let raw = LIST
        .replace("This week in mail standards", "Your payment is due")
        .replace("Read the news.", "Pay $42 by Friday.");
    assert!(drafts(&raw, Kind::Service, &[]).is_none());
}

#[test]
fn a_promotion_code_is_no_sign_in_code() {
    let raw = "From: Shop <deals@shop.example>\nSubject: Your discount code\n\nUse code 1234 at checkout. Log in to see more.\n";
    assert!(drafts(raw, Kind::Service, &[]).is_none());
    // In a list message, it is a feed item when it has no sign-in word.
    let promo = raw.replace(" Log in to see more.", "");
    let list = format!("List-Id: <deals.shop.example>\n{promo}");
    assert_eq!(one(&list, Kind::Service, &[]).item["form"], "feed");
    // Sign-in mail with a promotion word is no feed item: the model reads it.
    let mixed = "From: Shop <no-reply@shop.example>\nList-Unsubscribe: <https://shop.example/u>\nSubject: Sign in\n\nYour code: 482913\nShop our sale.\n";
    assert!(drafts(mixed, Kind::Service, &[]).is_none());
}

#[test]
fn an_invoice_only_in_an_attachment_goes_to_the_model() {
    let raw = "From: Mail Weekly <news@weekly.example>
List-Id: <weekly.news.example>
Subject: Your documents
MIME-Version: 1.0
Content-Type: multipart/mixed; boundary=\"b\"

--b
Content-Type: text/plain

See the file.
--b
Content-Type: text/plain; name=\"doc.txt\"
Content-Disposition: attachment; filename=\"doc.txt\"

Invoice 77. Amount 12.50 USD.
--b--
";
    assert!(drafts(raw, Kind::Service, &[]).is_none());
    // List mail with any file goes to the model: a file name or a file
    // without record words can hold a receipt too.
    let plain = raw.replace("Invoice 77. Amount 12.50 USD.", "Order 73, $42");
    assert!(drafts(&plain, Kind::Service, &[]).is_none());
}

#[test]
fn an_invitation_for_another_person_goes_to_the_model() {
    let other = INVITE.replace(
        "To: Owner <Owner@Example.com>",
        "To: Team <team@example.org>",
    );
    assert_ne!(other, INVITE);
    assert!(drafts(&other, Kind::Person, &[]).is_none());
    // A list header does not change it: the request is not a feed item.
    let list = format!("List-Id: <team.example.org>\n{other}");
    assert!(drafts(&list, Kind::Service, &[]).is_none());
    let cc = INVITE.replace(
        "To: Owner <Owner@Example.com>",
        "To: team@example.org\nCc: owner@example.com",
    );
    assert_eq!(one(&cc, Kind::Person, &[]).item["kind"], "invitation");
}

#[test]
fn an_invitation_or_a_code_with_a_file_goes_to_the_model() {
    let file = "--a\nContent-Type: application/pdf; name=\"booking.pdf\"\nContent-Disposition: attachment; filename=\"booking.pdf\"\nContent-Transfer-Encoding: base64\n\nAAAA\n--a--";
    let mixed = INVITE.replace("multipart/alternative", "multipart/mixed");
    // In a mixed message, the calendar part alone is no file.
    assert_eq!(one(&mixed, Kind::Person, &[]).item["kind"], "invitation");
    let invite = mixed.replacen("--a--", file, 1);
    assert_ne!(invite, mixed);
    assert!(drafts(&invite, Kind::Person, &[]).is_none());
    let code = "From: Bank <no-reply@bank.example>\nSubject: Sign in\nMIME-Version: 1.0\nContent-Type: multipart/mixed; boundary=a\n\n--a\nContent-Type: text/plain\n\nYour code: 482913\n".to_string() + file + "\n";
    assert!(drafts(&code, Kind::Service, &[]).is_none());
    // The same mail with no file gives the code.
    let plain = code.replace(file, "--a--");
    assert_eq!(one(&plain, Kind::Service, &[]).item["code"], "482913");
}

#[test]
fn a_cut_text_or_a_calendar_file_goes_to_the_model() {
    // A record after the limit of the clean text.
    let long = LIST.replace(
        "Read the news.",
        &format!("{}\nInvoice 77.", "Read the news. ".repeat(4_000)),
    );
    assert!(iris_understand::Text::of(long.replace('\n', "\r\n").as_bytes()).cut);
    assert!(drafts(&long, Kind::Service, &[]).is_none());
    // A calendar file that is no request is a file.
    let publish = "From: Mail Weekly <news@weekly.example>\nList-Id: <weekly.news.example>\nSubject: Events\nMIME-Version: 1.0\nContent-Type: multipart/mixed; boundary=a\n\n--a\nContent-Type: text/plain\n\nThe events.\n--a\nContent-Type: text/calendar; method=PUBLISH; name=\"e.ics\"\nContent-Disposition: attachment; filename=\"e.ics\"\n\nBEGIN:VCALENDAR\nMETHOD:PUBLISH\nEND:VCALENDAR\n--a--\n";
    assert!(drafts(publish, Kind::Service, &[]).is_none());
    // A request with a second calendar file that is no request.
    let both = INVITE
        .replace("multipart/alternative", "multipart/mixed")
        .replacen("--a--", "--a\nContent-Type: text/calendar; name=\"e.ics\"\nContent-Disposition: attachment; filename=\"e.ics\"\n\nBEGIN:VCALENDAR\nMETHOD:PUBLISH\nEND:VCALENDAR\n--a--", 1);
    assert!(drafts(&both, Kind::Person, &[]).is_none());
    // A payment request only in the text of the calendar request.
    let pay = INVITE.replace(
        "METHOD:REQUEST\n",
        "METHOD:REQUEST\nDESCRIPTION:Invoice 77\\; payment due.\n",
    );
    assert_ne!(pay, INVITE);
    assert!(drafts(&pay, Kind::Person, &[]).is_none());
}

#[test]
fn a_booking_in_a_list_message_goes_to_the_model() {
    let raw = LIST
        .replace("This week in mail standards", "Flight itinerary")
        .replace("Read the news.", "Reservation ABC123 for 12 Dec.");
    assert!(drafts(&raw, Kind::Service, &[]).is_none());
    // An invitation to a meeting is still an invitation.
    let meeting = INVITE.replace("Planning", "Team meeting");
    assert_eq!(one(&meeting, Kind::Person, &[]).item["kind"], "invitation");
}

#[test]
fn an_invoice_with_a_code_goes_to_the_model() {
    let raw = "From: Shop <billing@shop.example>\nSubject: Your invoice\n\nVerification code: 482913\nTotal: 12.50 USD\n";
    assert!(drafts(raw, Kind::Service, &[]).is_none());
}

#[test]
fn an_invoice_with_a_calendar_request_goes_to_the_model() {
    let raw = INVITE.replace("Join us.", "Invoice 77. Total: 12.50 USD");
    assert_ne!(raw, INVITE);
    assert!(drafts(&raw, Kind::Service, &[]).is_none());
    assert!(drafts(&raw, Kind::Person, &[]).is_none());
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
