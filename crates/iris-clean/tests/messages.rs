//! Messages from common mail clients, written by hand, end to end.

use iris_clean::{clean, CUT_MARK, MAX_DEPTH, TEXT_LIMIT};

fn crlf(s: &str) -> Vec<u8> {
    s.replace('\n', "\r\n").into_bytes()
}

#[test]
fn html_reply_with_quote_signature_and_tracking() {
    let raw = crlf(
        "From: Ana <ana@example.org>
Subject: Re: Plan
In-Reply-To: <a@example.org>
Content-Type: multipart/alternative; boundary=\"b1\"

--b1
Content-Type: text/plain; charset=utf-8

Yes, Friday works.

On Mon, Ana wrote:
> Can you?
--b1
Content-Type: text/html; charset=utf-8
Content-Transfer-Encoding: quoted-printable

<div dir=3D\"ltr\">Yes, Friday works. See <a href=3D\"https://docs.example.com/p?id=3D7&amp;utm_source=3Dmail\">the plan</a>.\
<img src=3D\"https://t.example.net/open.gif\" width=3D\"1\" height=3D\"1\"></div>
<div class=3D\"gmail_signature\">-- <br>Ana Bel<br>Sales</div>
<div class=3D\"gmail_quote\"><div class=3D\"gmail_attr\">On Mon, Ana wrote:</div>
<blockquote class=3D\"gmail_quote\">Can you?</blockquote></div>
--b1--
",
    );
    let c = clean(&raw);
    assert_eq!(c.subject, "Re: Plan");
    assert_eq!(
        c.text,
        "Yes, Friday works. See [the plan](https://docs.example.com/p?id=7)."
    );
    assert!(!c.cut && c.attachments.is_empty());
}

#[test]
fn outlook_plain_reply() {
    let raw = crlf(
        "Subject: RE: Lease
References: <x@example.org>

Signed and sent back.

Bo
________________________________
From: Ana Bel <ana@example.org>
Sent: Monday, 3 February 2025 10:00
To: Bo <bo@example.net>
Subject: Lease

Please sign the lease.
",
    );
    assert_eq!(clean(&raw).text, "Signed and sent back.\n\nBo");
}

#[test]
fn attachments_with_text_and_names() {
    let raw = crlf(
        "Subject: Files
Content-Type: multipart/mixed; boundary=m

--m
Content-Type: text/plain

See the files.
--m
Content-Type: text/plain; charset=iso-8859-1
Content-Disposition: attachment; filename*=iso-8859-1''notes%20caf%E9.txt
Content-Transfer-Encoding: base64

Q2Fm6SBhdCA5Lg==
--m
Content-Type: message/rfc822

Subject: =?utf-8?q?Old_n=C3=B6te?=
Content-Type: text/html

<p>Inner <b>text</b></p>
--m
Content-Type: image/png; name=\"logo.png\"
Content-Transfer-Encoding: base64

iVBORw0KGgo=
--m--
",
    );
    let c = clean(&raw);
    assert_eq!(c.text, "See the files.");
    let names: Vec<(&str, &str, u64, Option<&str>)> = c
        .attachments
        .iter()
        .map(|a| {
            (
                a.name.as_str(),
                a.mime_type.as_str(),
                a.size,
                a.text.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        names,
        vec![
            ("notes café.txt", "text/plain", 10, Some("Café at 9.")),
            ("Old nöte", "message/rfc822", 87, Some("Inner text")),
            ("logo.png", "image/png", 8, None),
        ]
    );
}

#[test]
fn long_text_is_cut_and_marked() {
    let body = "word ".repeat(20_000);
    let c = clean(format!("Subject: Long\n\n{body}").as_bytes());
    assert!(c.cut);
    let text = c.text.strip_suffix(CUT_MARK).expect("the cut mark");
    assert_eq!(text.chars().count(), TEXT_LIMIT);
}

#[test]
fn deep_nesting_stops_at_the_limit() {
    let mut raw = String::from("deep text");
    for level in 0..MAX_DEPTH + 20 {
        raw = format!(
            "Content-Type: multipart/mixed; boundary=\"l{level}\"\n\n--l{level}\n{}\n--l{level}--\n",
            if raw == "deep text" { format!("\n{raw}") } else { raw }
        );
    }
    let c = clean(format!("Subject: Deep\n{raw}").as_bytes());
    assert!(c.cut);
    assert!(!c.text.contains("deep text"));
}

#[test]
fn bytes_that_are_not_mail_do_not_panic() {
    for raw in [
        &b""[..],
        b"\r\n\r\n",
        b"no header",
        b"Content-Type: multipart/mixed\n\nno boundary",
        b"Content-Type: multipart/mixed; boundary=\"\"\n\n--\n--",
        b"Content-Type: text/html\n\n<<<>>><a href=",
        b"Subject: =?utf-8?B?\xff\xfe?=\n\n\xff\xfe\x00",
        b"Content-Transfer-Encoding: base64\n\n====",
    ] {
        clean(raw);
    }
}
