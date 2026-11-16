//! The invitation check of spec 1 section 7.3: a `text/calendar` part of
//! the top message with the method `REQUEST`.

fn inv(raw: &[u8]) -> bool {
    iris_clean::clean(raw).invitation
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
    // A MIME field over the limit of iris-clean: the message is not parsed
    // (fail closed), so it is no invitation.
    assert!(!inv(raw.as_bytes()));
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
}

#[test]
fn a_base64_calendar_request_is_found() {
    // "METHOD:REQUEST" in base64, over two lines.
    let raw = b"Content-Type: multipart/mixed; boundary=z\r\n\r\n--z\r\nContent-Type: text/calendar\r\nContent-Transfer-Encoding: base64\r\n\r\nTUVUSE9EOlJF\r\nUVVFU1Q=\r\n--z--\r\n";
    assert!(inv(raw));
}

#[test]
fn a_folded_part_header_is_unfolded_like_the_top_header() {
    let raw = b"Content-Type: multipart/mixed; boundary=z\r\n\r\n--z\r\nContent-Type:\r\n text/calendar;\r\n method=REQUEST\r\nContent-Transfer-Encoding:\r\n base64\r\n\r\nTUVUSE9EOlJFUVVFU1Q=\r\n--z--\r\n";
    assert!(inv(raw));
    let top = b"Content-Type: text/calendar\r\nContent-Transfer-Encoding:\r\n base64\r\n\r\nTUVUSE9EOlJFUVVFU1Q=\r\n";
    assert!(inv(top));
}

#[test]
fn the_calendar_type_and_method_parameter_match_whole_tokens() {
    let other =
        b"Content-Type: application/x-text/calendar; method=REQUEST\r\n\r\nMETHOD:REQUEST\r\n";
    assert!(!inv(other));
    let x = b"Content-Type: text/calendar; x-method=REQUEST\r\n\r\nMETHOD:CANCEL\r\n";
    assert!(!inv(x));
    let quoted = b"Content-Type: Text/Calendar; charset=utf-8; Method=\"REQUEST\"\r\n\r\nx\r\n";
    assert!(inv(quoted));
}

#[test]
fn a_closed_inner_boundary_in_a_later_text_part_starts_no_part() {
    let raw = b"Content-Type: multipart/mixed; boundary=outer\r\n\r\n--outer\r\n\
Content-Type: multipart/alternative; boundary=inner\r\n\r\n--inner\r\n\
Content-Type: text/plain\r\n\r\nhi\r\n--inner--\r\n--outer\r\n\
Content-Type: text/plain\r\n\r\n--inner\r\nContent-Type: text/calendar; method=REQUEST\r\n\r\nMETHOD:REQUEST\r\n--outer--\r\n";
    assert!(!inv(raw));
}

#[test]
fn a_calendar_version_of_an_alternative_counts() {
    let raw = b"Content-Type: multipart/alternative; boundary=a\r\n\r\n--a\r\n\
Content-Type: text/plain\r\n\r\nYou are invited.\r\n--a\r\n\
Content-Type: text/calendar; charset=utf-8\r\n\r\nBEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nEND:VCALENDAR\r\n--a--\r\n";
    assert!(inv(raw));
}

#[test]
fn a_calendar_in_an_attached_message_does_not_count() {
    let raw = b"Content-Type: multipart/mixed; boundary=m\r\n\r\n--m\r\n\
Content-Type: text/plain\r\n\r\nSee below.\r\n--m\r\nContent-Type: message/rfc822\r\n\r\n\
Subject: old\r\nContent-Type: text/calendar; method=REQUEST\r\n\r\nMETHOD:REQUEST\r\n--m--\r\n";
    assert!(!inv(raw));
}

#[test]
fn a_calendar_before_the_text_version_of_an_alternative_counts() {
    let raw = b"Content-Type: multipart/alternative; boundary=a\r\n\r\n--a\r\n\
Content-Type: text/calendar; charset=utf-8\r\n\r\nBEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nEND:VCALENDAR\r\n--a\r\n\
Content-Type: text/plain\r\n\r\nYou are invited.\r\n--a--\r\n";
    assert!(inv(raw));
}
