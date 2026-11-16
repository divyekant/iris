//! Steps 1 to 3 of understanding on hand-written messages: headers, clean
//! text and hostile marks. Pure functions: no store, no model.

use iris_understand::{analyze, Context, Kind, Mark};

const NOW: i64 = 1_767_225_600_000;

fn crlf(raw: &str) -> Vec<u8> {
    raw.replace('\n', "\r\n").into_bytes()
}

/// Analyze `raw` with these labels, known persons and earlier texts.
fn run(raw: &str, labels: &[&str], known: &[(&str, &str)], same: &[&str]) -> Vec<Mark> {
    analysis(raw, labels, known, same).marks
}

fn analysis(
    raw: &str,
    labels: &[&str],
    known: &[(&str, &str)],
    same: &[&str],
) -> iris_understand::Analysis {
    let labels: Vec<String> = labels.iter().map(|l| l.to_string()).collect();
    let same: Vec<String> = same.iter().map(|s| s.to_string()).collect();
    let lookup = |name: &str| {
        known
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, a)| a.to_string())
            .collect()
    };
    let cx = Context {
        me: "owner@example.com",
        kind: Kind::Stranger,
        labels: &labels,
        internal_date: None,
        now_ms: NOW,
        known_person: &lookup,
        existing: &[],
        thread: &[],
        same_sender: &same,
    };
    analyze(&crlf(raw), &cx)
}

const ANA: &[(&str, &str)] = &[("Ana Lee", "ana@example.org")];

#[test]
fn a_dmarc_fail_for_the_from_domain_is_a_mark() {
    let raw = "Authentication-Results: mx.google.com; dkim=fail header.d=shop.example; dmarc=fail (p=REJECT) header.from=shop.example
From: Shop <billing@shop.example>
Subject: Pay

Pay now.
";
    assert_eq!(run(raw, &[], &[], &[]), vec![Mark::Dmarc]);
    // A DMARC result for another domain is not about this From.
    let other = raw.replace("header.from=shop.example", "header.from=other.example");
    assert!(run(&other, &[], &[], &[]).is_empty());
}

#[test]
fn a_forged_lower_auth_header_does_not_count() {
    let raw = "Authentication-Results: mx.google.com; dkim=none; spf=fail; dmarc=fail (p=NONE) header.from=evil.example
Authentication-Results: mx.google.com; dkim=pass header.d=evil.example; dmarc=pass header.from=evil.example
From: Ana Lee <ana@evil.example>
Subject: Hi

Pay this.
";
    assert_eq!(run(raw, &[], ANA, &[]), vec![Mark::Dmarc, Mark::Spoof]);
}

#[test]
fn a_known_name_from_another_address_is_a_spoof_unless_it_passes() {
    let raw = "From: Ana Lee <ana@other.example>
Subject: Hi

Call me.
";
    assert_eq!(run(raw, &[], ANA, &[]), vec![Mark::Spoof]);
    let pass = format!(
        "Authentication-Results: mx.google.com; dkim=pass header.d=other.example; dmarc=pass header.from=other.example\n{raw}"
    );
    assert!(run(&pass, &[], ANA, &[]).is_empty());
    // The known address itself is not a spoof.
    let own = raw.replace("ana@other.example", "Ana@Example.org");
    assert!(run(&own, &[], ANA, &[]).is_empty());
}

#[test]
fn a_shared_name_of_two_known_people_is_not_a_spoof() {
    let known = &[
        ("Ana Lee", "ana@example.org"),
        ("Ana Lee", "ana.lee@example.net"),
    ];
    let raw = "From: Ana Lee <ana.lee@example.net>\nSubject: Hi\n\nCall me.\n";
    assert!(run(raw, &[], known, &[]).is_empty());
}

#[test]
fn an_encoded_known_name_is_compared_after_decoding() {
    let raw = "From: =?utf-8?B?QW5hIExlZQ==?= <ana@evil.example>\nSubject: Hi\n\nPay this.\n";
    let a = analysis(raw, &[], ANA, &[]);
    assert_eq!(a.head.from_name.as_deref(), Some("Ana Lee"));
    assert_eq!(a.marks, vec![Mark::Spoof]);
}

#[test]
fn the_spam_label_is_a_mark() {
    let raw = "From: x@example.net\nSubject: Win\n\nWin now.\n";
    assert_eq!(run(raw, &["INBOX", "SPAM"], &[], &[]), vec![Mark::Spam]);
    assert!(run(raw, &["INBOX"], &[], &[]).is_empty());
}

#[test]
fn a_repeated_end_block_of_the_same_sender_goes_out() {
    let raw = "From: Bo Ren <bo@example.org>\nSubject: Re: Lease\n\nCan you call me today?\n\nBo Ren\nLease Office, 12 High Street\n";
    let earlier = "Here is the plan.\n\nBo Ren\nLease Office, 12 High Street";
    let a = analysis(raw, &[], &[], &[earlier]);
    assert_eq!(a.text.text.trim(), "Can you call me today?");
    // With no earlier text, the block stays.
    let a = analysis(raw, &[], &[], &[]);
    assert!(a.text.text.contains("12 High Street"));
}

#[test]
fn the_text_of_an_attachment_is_kept_with_its_name() {
    let raw = "From: Shop <shop@example.com>
Subject: Your papers
Content-Type: multipart/mixed; boundary=z

--z
Content-Type: text/plain

See the file.
--z
Content-Type: text/plain; name=invoice.txt
Content-Disposition: attachment; filename=invoice.txt

Invoice 77: total 50.00 USD.
--z--
";
    let a = analysis(raw, &[], &[], &[]);
    assert_eq!(a.text.attachments[0].0, "invoice.txt");
    assert!(a.text.attachment_text[0].1.contains("Invoice 77"));
    assert!(!a.text.text.contains("Invoice 77"));
}

#[test]
fn the_date_is_internal_date_then_the_header_then_now() {
    let raw = "From: a@example.org\nDate: Wed, 31 Dec 2025 09:00:00 +0000\nSubject: x\n\nHi.\n";
    let header = 1_767_171_600_000;
    let date = |raw: &str, internal: Option<i64>| {
        let lookup = |_: &str| Vec::new();
        let cx = Context {
            me: "owner@example.com",
            kind: Kind::Stranger,
            labels: &[],
            internal_date: internal,
            now_ms: NOW,
            known_person: &lookup,
            existing: &[],
            thread: &[],
            same_sender: &[],
        };
        analyze(&crlf(raw), &cx).date(&cx)
    };
    assert_eq!(date(raw, Some(42)), 42);
    assert_eq!(date(raw, None), header);
    let no_date = "From: a@example.org\nSubject: x\n\nHi.\n";
    assert_eq!(date(no_date, None), NOW);
    let huge =
        "From: a@example.org\nDate: 1 Jan 9223372036854775807 00:00 +0000\nSubject: x\n\nHi.\n";
    assert_eq!(date(huge, None), NOW);
}

#[test]
fn a_dmarc_result_needs_the_from_domain() {
    // A DMARC fail with no header.from domain is no evidence about From.
    let fail = "Authentication-Results: mx.google.com; dmarc=fail (p=REJECT)\nFrom: Shop <billing@shop.example>\nSubject: Pay\n\nPay now.\n";
    assert!(run(fail, &[], &[], &[]).is_empty());
    // A DMARC pass for another domain does not clear a spoof.
    let other = "Authentication-Results: mx.google.com; dkim=pass header.d=other.example; dmarc=pass header.from=else.example\nFrom: Ana Lee <ana@other.example>\nSubject: Hi\n\nCall me.\n";
    assert_eq!(run(other, &[], ANA, &[]), vec![Mark::Spoof]);
    let none = "Authentication-Results: mx.google.com; dkim=pass header.d=other.example; dmarc=pass\nFrom: Ana Lee <ana@other.example>\nSubject: Hi\n\nCall me.\n";
    assert_eq!(run(none, &[], ANA, &[]), vec![Mark::Spoof]);
}

#[test]
fn every_recipient_is_kept() {
    let to: Vec<String> = (0..250).map(|i| format!("p{i}@example.org")).collect();
    let raw = format!(
        "From: a@example.org\nTo: {}\nSubject: x\n\nHi.\n",
        to.join(", ")
    );
    let a = analysis(&raw, &[], &[], &[]);
    assert_eq!(a.head.to, to);
}

#[test]
fn attachment_text_after_32000_characters_is_kept() {
    let long = "word ".repeat(10_000);
    let raw = format!(
        "From: Shop <shop@example.com>
Subject: Papers
Content-Type: multipart/mixed; boundary=z

--z
Content-Type: text/plain

See the file.
--z
Content-Type: text/plain; name=invoice.txt
Content-Disposition: attachment; filename=invoice.txt

{long}
Total due: 77.00 USD.
--z--
"
    );
    let a = analysis(&raw, &[], &[], &[]);
    assert!(a.text.attachment_text[0]
        .1
        .contains("Total due: 77.00 USD."));
}

#[test]
fn the_cut_mark_stays_after_the_repeated_end_goes_out() {
    let filler: String = (0..4_000)
        .map(|i| format!("line {i} of the report\n"))
        .collect();
    let mk = |first: &str| {
        format!("From: Bo Ren <bo@example.org>\nSubject: Report\n\n{first}\n\n{filler}")
    };
    let earlier = iris_understand::Text::of(&crlf(&mk("The first report."))).text;
    assert!(earlier.ends_with(iris_clean::CUT_MARK));
    let a = analysis(&mk("The second report."), &[], &[], &[&earlier]);
    assert!(a.text.cut);
    assert!(a.text.text.starts_with("The second report."));
    assert!(
        a.text.text.ends_with(iris_clean::CUT_MARK),
        "{}",
        &a.text.text[..200.min(a.text.text.len())]
    );
}
