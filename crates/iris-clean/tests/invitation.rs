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

#[test]
fn a_method_line_after_64_kib_counts_in_each_encoding() {
    let pad = "DESCRIPTION:x\r\n".repeat(10_000);
    let cal = format!("BEGIN:VCALENDAR\r\n{pad}  method:request \r\nEND:VCALENDAR\r\n");
    let plain = format!("Content-Type: text/calendar\r\n\r\n{cal}");
    assert!(inv(plain.as_bytes()));
    let b64 = base64_lines(cal.as_bytes());
    let raw =
        format!("Content-Type: text/calendar\r\nContent-Transfer-Encoding: base64\r\n\r\n{b64}");
    assert!(inv(raw.as_bytes()));
    let qp = format!(
        "Content-Type: text/calendar\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n{pad}METHOD=3AREQ=\r\nUEST\r\n"
    );
    assert!(inv(qp.as_bytes()));
    // A longer line or a second word does not count.
    for bad in ["METHOD:REQUESTX", "METHOD:REQUEST X", "X METHOD:REQUEST"] {
        let raw = format!("Content-Type: text/calendar\r\n\r\n{pad}{bad}\r\n");
        assert!(!inv(raw.as_bytes()), "{bad}");
    }
}

/// Base64 in lines of 76 characters.
fn base64_lines(data: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out.as_bytes()
        .chunks(76)
        .map(|l| format!("{}\r\n", std::str::from_utf8(l).unwrap()))
        .collect()
}

/// A METHOD line inside one long quoted-printable line (with encoded line
/// ends), at each offset around the chunk size of 64 KiB.
#[test]
fn a_method_line_across_each_quoted_printable_chunk_boundary_counts() {
    for pad in 65_500..65_560 {
        let fill = "x".repeat(pad);
        for method in [
            "METHOD:REQUEST",
            "=4D=45=54=48=4F=44=3A=52=45=51=55=45=53=54",
        ] {
            let raw = format!(
                "Content-Type: text/calendar\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n{fill}=0D=0A{method}=0D=0AEND:VCALENDAR"
            );
            assert!(inv(raw.as_bytes()), "pad {pad} {method}");
            let cancel = raw
                .replace("REQUEST", "CANCEL")
                .replace("=52=45=51=55=45=53=54", "=43=41=4E=43=45=4C");
            assert!(!inv(cancel.as_bytes()), "pad {pad} cancel");
        }
    }
}

#[test]
fn every_calendar_part_must_be_a_request() {
    // The form of a common calendar service: an inline request and the same
    // request as an `application/ics` file.
    let google = b"Content-Type: multipart/mixed; boundary=m\r\n\r\n--m\r\n\
Content-Type: multipart/alternative; boundary=a\r\n\r\n--a\r\n\
Content-Type: text/plain\r\n\r\nJoin.\r\n--a\r\n\
Content-Type: text/calendar; method=REQUEST\r\n\r\nBEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nEND:VCALENDAR\r\n--a--\r\n--m\r\n\
Content-Type: application/ics; name=\"invite.ics\"\r\n\
Content-Disposition: attachment; filename=\"invite.ics\"\r\n\r\n\
BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nEND:VCALENDAR\r\n--m--\r\n";
    assert!(inv(google));
    // Another calendar file next to the request: not only a request.
    let publish = String::from_utf8_lossy(google).replacen(
        "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nEND:VCALENDAR\r\n--m--",
        "BEGIN:VCALENDAR\r\nMETHOD:PUBLISH\r\nEND:VCALENDAR\r\n--m--",
        1,
    );
    assert_ne!(publish.as_bytes(), &google[..]);
    assert!(!inv(publish.as_bytes()));
    let text_cal = publish.replace("application/ics", "text/calendar");
    assert!(!inv(text_cal.as_bytes()));
}

#[test]
fn calendar_text_is_given_and_bounded() {
    let raw = b"Content-Type: text/calendar; method=REQUEST\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nBEGIN:VCALENDAR\r\nDESCRIPTION:Invoice=2077\r\nEND:VCALENDAR\r\n";
    let c = iris_clean::clean(raw);
    assert!(c.invitation && !c.calendar_cut);
    assert!(c.calendar_text.contains("DESCRIPTION:Invoice 77"));
    let long = format!(
        "Content-Type: text/calendar; method=REQUEST\r\n\r\nBEGIN:VCALENDAR\r\nDESCRIPTION:{}\r\nEND:VCALENDAR\r\n",
        "x".repeat(iris_clean::CALENDAR_TEXT_LIMIT)
    );
    let c = iris_clean::clean(long.as_bytes());
    assert!(c.calendar_cut);
    assert!(c.calendar_text.len() <= iris_clean::CALENDAR_TEXT_LIMIT);
}

#[test]
fn calendar_text_stays_inside_its_limit_in_bytes() {
    // A two-byte character across the byte limit.
    let pad = "x".repeat(iris_clean::CALENDAR_TEXT_LIMIT - 3);
    let raw = format!(
        "Content-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\r\n{pad}\u{e9}\u{e9}\r\n"
    );
    let c = iris_clean::clean(raw.as_bytes());
    assert!(c.calendar_cut);
    assert!(c.calendar_text.len() <= iris_clean::CALENDAR_TEXT_LIMIT);
}

#[test]
fn a_calendar_in_an_unused_alternative_counts() {
    let raw = b"Content-Type: multipart/alternative; boundary=a\r\n\r\n--a\r\nContent-Type: text/plain\r\n\r\nEvents.\r\n--a\r\nContent-Type: text/calendar; method=PUBLISH\r\n\r\nBEGIN:VCALENDAR\r\nMETHOD:PUBLISH\r\nEND:VCALENDAR\r\n--a--\r\n";
    let c = iris_clean::clean(raw);
    assert!(c.calendar && !c.invitation);
    assert!(!iris_clean::clean(b"Subject: x\r\n\r\nHello\r\n").calendar);
}
