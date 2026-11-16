//! Unit tests of the header parser: addresses, the auth header, dates and
//! the calendar scan.

use iris_understand::head::{addresses, date};
use iris_understand::{Auth, Head};

fn inv(raw: &[u8]) -> bool {
    Head::parse(raw).invitation
}

#[test]
fn addresses_split_and_lowercase() {
    let a = addresses(r#""Lee, Ana" <Ana@Example.COM>, bob@example.org (Bob, B), <c@x.example>"#);
    assert_eq!(
        a,
        vec![
            ("ana@example.com".into(), Some("Lee, Ana".into())),
            ("bob@example.org".into(), None),
            ("c@x.example".into(), None),
        ]
    );
    assert!(addresses("undisclosed-recipients:;").is_empty());
}

#[test]
fn only_the_top_gmail_auth_header_counts() {
    let raw = b"Authentication-Results: other.example; dmarc=pass\r\n\
Authentication-Results: mx.google.com;\r\n dkim=pass header.i=@Bank.example (comment; x);\r\n spf=pass smtp.mailfrom=a@bank.example;\r\n dmarc=fail (p=REJECT) header.from=bank.example\r\n\
Authentication-Results: mx.google.com; dmarc=pass header.from=bank.example\r\n\
From: a@bank.example\r\n\r\nbody";
    let a = Head::parse(raw).auth.unwrap();
    assert_eq!(a.dmarc.as_deref(), Some("fail"));
    assert_eq!(a.dmarc_from.as_deref(), Some("bank.example"));
    assert!(a.dkim_pass("bank.example"));
    assert_eq!(a.spf.as_deref(), Some("pass"));
}

#[test]
fn dates() {
    assert_eq!(date("Thu, 01 Jan 1970 00:00:00 +0000"), Some(0));
    assert_eq!(date("2 Jan 1970 01:00 +0100"), Some(86_400_000));
    assert_eq!(
        date("Fri, 30 Aug 2024 19:09:01 -0700 (PDT)"),
        Some(1_725_070_141_000)
    );
    assert_eq!(date("not a date"), None);
    assert_eq!(date("31 Feb 2025 00:00:00 +0000"), None);
    assert_eq!(date("29 Feb 2023 00:00:00 +0000"), None);
    assert!(date("29 Feb 2024 00:00:00 +0000").is_some());
    assert_eq!(date("1 Jan 2025 00:00:00 +2400"), None);
    assert_eq!(date("1 Jan 2025 00:00:00 +0160"), None);
}

#[test]
fn a_quoted_printable_calendar_request_is_found() {
    let raw = b"Content-Type: multipart/mixed; boundary=z\r\n\r\n--z\r\nContent-Type: text/calendar\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nBEGIN:VCALENDAR\r\nMETHOD=3AREQUEST\r\nEND:VCALENDAR\r\n--z--\r\n";
    assert!(inv(raw));
    let cancel = String::from_utf8_lossy(raw).replace("REQUEST", "CANCEL");
    assert!(!inv(cancel.as_bytes()));
    // A soft line break inside the method line.
    let split = String::from_utf8_lossy(raw).replace("METHOD=3AREQUEST", "METHOD:RE=\r\nQUEST");
    assert!(inv(split.as_bytes()));
}

#[test]
fn a_huge_content_type_stays_bounded() {
    let long = "x".repeat(4 * 256 * 1024);
    let raw = format!("Content-Type: text/calendar; a={long}\r\n {long}\r\n\r\nMETHOD:REQUEST\r\n");
    // The scan ends and still reads the body.
    assert!(inv(raw.as_bytes()));
}

#[test]
fn groups_comments_and_parent_dkim_domains() {
    let h = Head::parse(b"From: a@mail.example.org\r\nTo: Friends: alice@example.org, Bob <BOB@example.org>;\r\nPrecedence: bulk (list mail)\r\n\r\nx");
    assert_eq!(h.to, vec!["alice@example.org", "bob@example.org"]);
    assert!(h.automated);
    let human = Head::parse(b"From: a@b.c\r\nAuto-Submitted: no (human-generated)\r\n\r\nx");
    assert!(!human.automated);
    let a = Auth {
        dkim: vec![("example.org".into(), "pass".into())],
        ..Default::default()
    };
    assert!(a.dkim_pass("mail.example.org"));
    assert!(a.dkim_pass("example.org"));
    assert!(!a.dkim_pass("badexample.org"));
}

#[test]
fn invitation_needs_calendar_and_request() {
    assert!(inv(b"Content-Type: text/calendar; method=REQUEST\r\n"));
    assert!(!inv(b"Content-Type: text/calendar; method=CANCEL\r\n"));
    assert!(!inv(b"METHOD:REQUEST in plain text"));
    assert!(!inv(
        b"Subject: x\r\n\r\nContent-Type: text/calendar; method=REQUEST\r\nMETHOD:REQUEST\r\n"
    ));
    let mixed = b"Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\n\
Content-Type: text/plain\r\n\r\nMETHOD:REQUEST\r\n--b\r\n\
Content-Type: text/calendar;\r\n charset=utf-8\r\n\r\nBEGIN:VCALENDAR\r\nMETHOD:CANCEL\r\n--b--\r\n";
    assert!(!inv(mixed));
    let ok = String::from_utf8_lossy(mixed).replace("METHOD:CANCEL", "METHOD:REQUEST");
    assert!(inv(ok.as_bytes()));
    assert_eq!(date("1 Jan 9223372036854775807 00:00 +0000"), None);
}

#[test]
fn a_base64_calendar_request_is_found() {
    // "METHOD:REQUEST" in base64, over two lines.
    let raw = b"Content-Type: multipart/mixed; boundary=z\r\n\r\n--z\r\nContent-Type: text/calendar\r\nContent-Transfer-Encoding: base64\r\n\r\nTUVUSE9EOlJF\r\nUVVFU1Q=\r\n--z--\r\n";
    assert!(inv(raw));
}

#[test]
fn named_zones_and_unknown_zones() {
    assert_eq!(
        date("Thu, 25 Jan 2024 06:26:35 EST"),
        date("Thu, 25 Jan 2024 06:26:35 -0500")
    );
    assert_eq!(
        date("Thu, 25 Jan 2024 06:26:35 GMT"),
        date("Thu, 25 Jan 2024 06:26:35 +0000")
    );
    assert_eq!(
        date("Thu, 25 Jan 2024 06:26:35 PDT"),
        date("Thu, 25 Jan 2024 06:26:35 -0700")
    );
    assert_eq!(date("Thu, 25 Jan 2024 06:26:35 XYZ"), None);
}
