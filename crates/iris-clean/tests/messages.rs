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

#[test]
fn a_big_plain_version_does_not_use_the_budget_of_the_html_version() {
    let plain = "p ".repeat(1_200_000);
    let raw = format!(
        "Subject: Alt\nContent-Type: multipart/alternative; boundary=a\n\n--a\n\
         Content-Type: text/plain\n\n{plain}\n--a\nContent-Type: text/html\n\n<p>Shown</p>\n--a--\n"
    );
    let c = clean(raw.as_bytes());
    assert_eq!(c.text, "Shown");
    assert!(!c.cut);
}

#[test]
fn a_reply_keeps_the_quotes_of_a_forwarded_message() {
    let raw = crlf(
        "Subject: Re: Fwd: Plan
In-Reply-To: <a@example.org>

See below.

---------- Forwarded message ---------
From: Ana
> the old plan
The new plan.
",
    );
    assert_eq!(
        clean(&raw).text,
        "See below.\n\n---------- Forwarded message ---------\nFrom: Ana\n> the old plan\nThe new plan."
    );
}

#[test]
fn quoted_printable_attachment_size_is_decoded() {
    let raw = crlf(
        "Subject: QP
Content-Type: multipart/mixed; boundary=m

--m
Content-Type: application/octet-stream; name=a.bin
Content-Transfer-Encoding: quoted-printable

a=3Db
--m--
",
    );
    assert_eq!(clean(&raw).attachments[0].size, 3);
}

#[test]
fn a_simple_pdf_attachment_gives_text() {
    let page = b"BT /F1 12 Tf 72 720 Td (Invoice 2025-07 for Avel Torun) Tj 0 -14 Td (Total: 120.00 EUR) Tj ET";
    let data = miniz_oxide::deflate::compress_to_vec_zlib(page, 6);
    let mut pdf = format!(
        "%PDF-1.4\n4 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
        data.len()
    )
    .into_bytes();
    pdf.extend(data);
    pdf.extend_from_slice(b"\nendstream\nendobj\n%%EOF\n");
    let with_notes = |notes: usize| {
        let mut raw =
            b"Subject: Invoice\r\nContent-Type: multipart/mixed; boundary=m\r\n\r\n--m\r\n\
            Content-Type: text/plain\r\n\r\nSee the PDF.\r\n--m\r\n"
                .to_vec();
        if notes > 0 {
            raw.extend_from_slice(b"Content-Type: text/plain; name=n.txt\r\n\r\n");
            raw.extend(std::iter::repeat_n(b'n', notes));
            raw.extend_from_slice(b"\r\n--m\r\n");
        }
        raw.extend_from_slice(
            b"Content-Type: application/pdf; name=\"i.pdf\"\r\nContent-Transfer-Encoding: binary\r\n\r\n",
        );
        raw.extend(&pdf);
        raw.extend_from_slice(b"\r\n--m--\r\n");
        clean(&raw)
    };
    let c = with_notes(0);
    assert_eq!(c.text, "See the PDF.");
    assert_eq!(
        c.attachments[0].text.as_deref(),
        Some("Invoice 2025-07 for Avel Torun\nTotal: 120.00 EUR")
    );
    // With 2 bytes of budget left, the PDF keeps 2 bytes and the cut mark.
    let c = with_notes(iris_clean::ATTACHMENT_TEXT_LIMIT - 2);
    assert_eq!(
        c.attachments[1].text.as_deref(),
        Some(format!("In{}", iris_clean::CUT_MARK).as_str())
    );
}

#[test]
fn a_wrapped_base64_attachment_below_the_limit_is_complete() {
    let size = iris_clean::ATTACHMENT_TEXT_LIMIT - 1000;
    let text: String = "abcdefghij\n".repeat(size / 11 + 1)[..size].to_string();
    let mut b64 = String::new();
    for chunk in text.as_bytes().chunks(57) {
        b64.push_str(&encode(chunk));
        b64.push_str("\r\n");
    }
    let raw = format!(
        "Subject: Log\r\nContent-Type: multipart/mixed; boundary=m\r\n\r\n--m\r\n\
         Content-Type: text/plain; name=log.txt\r\nContent-Transfer-Encoding: base64\r\n\r\n{b64}--m--\r\n"
    );
    let c = clean(raw.as_bytes());
    assert_eq!(c.attachments[0].size, size as u64);
    assert_eq!(c.attachments[0].text.as_deref(), Some(text.as_str()));
}

fn encode(chunk: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for g in chunk.chunks(3) {
        let n = (g[0] as u32) << 16
            | (*g.get(1).unwrap_or(&0) as u32) << 8
            | *g.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            out.push(if k <= g.len() {
                A[(n >> (18 - 6 * k) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

#[test]
fn an_html_version_without_visible_text_falls_back_to_plain_text() {
    for html in ["", "<div style=\"display:none\">hidden</div>"] {
        let raw = format!(
            "Subject: Alt\nContent-Type: multipart/alternative; boundary=a\n\n--a\n\
             Content-Type: text/plain\n\nPlain words.\n--a\nContent-Type: text/html\n\n{html}\n--a--\n"
        );
        assert_eq!(clean(raw.as_bytes()).text, "Plain words.");
    }
}

#[test]
fn a_multipart_attachment_stays_an_attachment() {
    let raw = crlf(
        "Subject: Saved page
Content-Type: multipart/mixed; boundary=m

--m
Content-Type: text/plain

See the page.
--m
Content-Type: multipart/related; boundary=r
Content-Disposition: attachment; filename=page.mht

--r
Content-Type: text/html

<p>Page text</p>
--r--
--m--
",
    );
    let c = clean(&raw);
    assert_eq!(c.text, "See the page.");
    assert_eq!(c.attachments.len(), 1);
    assert_eq!(c.attachments[0].name, "page.mht");
    assert_eq!(c.attachments[0].mime_type, "multipart/related");
}

#[test]
fn deep_quotes_with_many_lines_are_cut() {
    let html = format!("{}{}", "<blockquote>".repeat(500), "<br>x".repeat(300_000));
    let raw = format!("Subject: Deep\nContent-Type: text/html\n\n{html}");
    let c = clean(raw.as_bytes());
    assert!(c.cut);
}

#[test]
fn an_attached_message_does_not_use_the_budget_of_the_outer_message() {
    let inner_text = "i ".repeat(1_100_000);
    let raw = format!(
        "Subject: Fwd\nContent-Type: multipart/mixed; boundary=m\n\n--m\n\
         Content-Type: message/rfc822\n\nSubject: Inner\n\n{inner_text}\n--m\n\
         Content-Type: text/plain\n\nOuter words.\n--m--\n"
    );
    let c = clean(raw.as_bytes());
    assert_eq!(c.text, "Outer words.");
    assert!(!c.cut);
    assert!(c.attachments[0].text.as_ref().unwrap().ends_with(CUT_MARK));
}

#[test]
fn quoted_printable_soft_breaks_do_not_use_the_budget() {
    let body = format!("{}Visible end.", "=\n".repeat(iris_clean::PART_LIMIT));
    let raw = format!("Subject: QP\nContent-Transfer-Encoding: quoted-printable\n\n{body}");
    let c = clean(raw.as_bytes());
    assert_eq!(c.text, "Visible end.");
    assert!(!c.cut);
}

#[test]
fn alternatives_past_the_part_limit_mark_the_cut() {
    let mut raw =
        String::from("Subject: Many\nContent-Type: multipart/alternative; boundary=a\n\n");
    for k in 0..iris_clean::MAX_PARTS + 1 {
        raw.push_str(&format!("--a\nContent-Type: text/plain\n\nVersion {k}\n"));
    }
    raw.push_str("--a--\n");
    assert!(clean(raw.as_bytes()).cut);
}

#[test]
fn quotes_in_one_part_do_not_remove_a_later_part() {
    let raw = crlf(
        "Subject: Re: Plan
In-Reply-To: <a@example.org>
Content-Type: multipart/mixed; boundary=m

--m
Content-Type: text/plain

Yes.

On Mon, Ana wrote:
> Can you?
--m
Content-Type: text/plain

New details: room 4.
--m--
",
    );
    assert_eq!(clean(&raw).text, "Yes.\n\nNew details: room 4.");
}

#[test]
fn a_clipped_attached_message_lists_no_partial_attachments() {
    let file = "f".repeat(3 * 1024 * 1024);
    let raw = format!(
        "Subject: Fwd\nContent-Type: multipart/mixed; boundary=o\n\n--o\n\
         Content-Type: message/rfc822\n\nSubject: Inner\nContent-Type: multipart/mixed; boundary=i\n\n\
         --i\nContent-Type: text/plain\n\nShort text.\n--i\n\
         Content-Type: application/octet-stream; name=big.bin\n\n{file}\n--i--\n--o--\n"
    );
    let c = clean(raw.as_bytes());
    assert_eq!(c.attachments.len(), 1);
    assert_eq!(c.attachments[0].mime_type, "message/rfc822");
    assert_eq!(
        c.attachments[0].text.as_deref(),
        Some(format!("Short text.{CUT_MARK}").as_str())
    );
}

#[test]
fn a_big_hidden_html_version_gives_its_budget_back() {
    let hidden = "<div style=\"display:none\">x</div>".repeat(100_000);
    let raw = format!(
        "Subject: Alt\nContent-Type: multipart/alternative; boundary=a\n\n--a\n\
         Content-Type: text/plain\n\nPlain words.\n--a\nContent-Type: text/html\n\n{hidden}\n--a--\n"
    );
    assert!(hidden.len() > iris_clean::PART_LIMIT);
    let c = clean(raw.as_bytes());
    assert_eq!(c.text, "Plain words.");
    assert!(!c.cut);
}

#[test]
fn an_html_version_with_only_quoted_history_falls_back_to_plain_text() {
    let raw = crlf(
        "Subject: Re: Plan
In-Reply-To: <a@example.org>
Content-Type: multipart/alternative; boundary=a

--a
Content-Type: text/plain

Yes, Friday.
--a
Content-Type: text/html

<div>On Mon, Ana wrote:</div><blockquote>Can you?</blockquote>
--a--
",
    );
    assert_eq!(clean(&raw).text, "Yes, Friday.");
}

#[test]
fn a_discarded_alternative_lists_no_attachments() {
    let raw = crlf(
        "Subject: Alt
Content-Type: multipart/alternative; boundary=a

--a
Content-Type: text/plain

Plain words.
--a
Content-Type: multipart/related; boundary=r

--r
Content-Type: text/html

<div hidden>x</div>
--r
Content-Type: text/plain; name=notes.txt

Notes.
--r--
--a--
",
    );
    let c = clean(&raw);
    assert_eq!(c.text, "Plain words.");
    assert!(c.attachments.is_empty());
}

#[test]
fn an_outlook_forward_keeps_the_forwarded_message() {
    let raw = crlf(
        "Subject: FW: Plan
Content-Type: text/plain

FYI

-----Original Message-----
From: Ana Bel
Sent: Monday
To: Avel
Subject: Plan

The plan is ready.
",
    );
    let c = clean(&raw);
    assert!(c.text.starts_with("FYI"), "{}", c.text);
    assert!(c.text.contains("The plan is ready."), "{}", c.text);
}

#[test]
fn an_attached_message_keeps_its_body_before_its_attachment_text() {
    let notes = "n".repeat(1_000_000);
    let raw = crlf(&format!(
        "Subject: Fwd\nContent-Type: multipart/mixed; boundary=m\n\n--m\nContent-Type: text/plain\n\nSee below.\n\
         --m\nContent-Type: message/rfc822\n\nSubject: Inner\nContent-Type: multipart/mixed; boundary=i\n\n\
         --i\nContent-Type: text/plain\n\nThe inner body.\n--i\nContent-Type: text/plain; name=notes.txt\n\n{notes}\n--i--\n--m--\n"
    ));
    let c = clean(&raw);
    let inner = c.attachments[0].text.as_deref().unwrap_or_default();
    assert!(inner.starts_with("The inner body."), "{inner:.80}");
    let total: usize = c
        .attachments
        .iter()
        .filter_map(|a| a.text.as_ref())
        .map(String::len)
        .sum();
    assert!(total <= iris_clean::ATTACHMENT_TEXT_LIMIT + 64, "{total}");
    assert!(c.attachments[1]
        .text
        .as_deref()
        .is_some_and(|t| t.ends_with("[Iris cut the text here.]")));
}

#[test]
fn long_quoted_history_leaves_room_for_a_later_part() {
    let history = "> old line of the thread\n".repeat(30_000);
    let raw = crlf(&format!(
        "Subject: Re: Plan\nIn-Reply-To: <a@example.org>\nContent-Type: multipart/mixed; boundary=m\n\n\
         --m\nContent-Type: text/plain\n\nShort reply.\n\nOn Mon, Ana wrote:\n{history}\
         --m\nContent-Type: text/plain\n\nThe new part.\n--m--\n"
    ));
    let c = clean(&raw);
    assert_eq!(c.text, "Short reply.\n\nThe new part.");
}

#[test]
fn a_named_multipart_is_an_attachment() {
    let raw = crlf(
        "Subject: Page
Content-Type: multipart/mixed; boundary=m

--m
Content-Type: text/plain

See the page.
--m
Content-Type: multipart/related; boundary=r; name=page.mht

--r
Content-Type: text/html

<p>Saved page text</p>
--r--
--m--
",
    );
    let c = clean(&raw);
    assert_eq!(c.text, "See the page.");
    assert_eq!(c.attachments.len(), 1);
    assert_eq!(c.attachments[0].name, "page.mht");
}

#[test]
fn a_capped_attachment_text_shows_the_cut() {
    let big = "w".repeat(1_200_000);
    let raw = crlf(&format!(
        "Subject: Big\nContent-Type: multipart/mixed; boundary=m\n\n--m\nContent-Type: text/plain\n\nHi\n\
         --m\nContent-Type: text/plain; name=big.txt\n\n{big}\n--m--\n"
    ));
    let c = clean(&raw);
    let t = c.attachments[0].text.as_deref().unwrap_or_default();
    assert!(
        t.ends_with("[Iris cut the text here.]"),
        "{}",
        &t[t.len() - 40..]
    );
}

#[test]
fn an_uppercase_url_scheme_loses_its_tracking() {
    let raw = crlf(
        "Subject: Link\nContent-Type: text/html\n\n<a href=\"HTTPS://example.org/?utm_source=mail\">Shop</a>\n",
    );
    assert_eq!(clean(&raw).text, "[Shop](HTTPS://example.org/)");
}

#[test]
fn history_past_the_body_budget_leaves_room_for_a_later_part() {
    let history = "> old line of the thread\n".repeat(100_000);
    let raw = crlf(&format!(
        "Subject: Re: Plan\nIn-Reply-To: <a@example.org>\nContent-Type: multipart/mixed; boundary=m\n\n\
         --m\nContent-Type: text/plain\n\nShort reply.\n\nOn Mon, Ana wrote:\n{history}\
         --m\nContent-Type: text/plain\n\nThe new part.\n--m--\n"
    ));
    // The first part is longer than its decode limit, so the cut mark shows.
    let c = clean(&raw);
    assert!(
        c.text.starts_with("Short reply.\n\nThe new part."),
        "{}",
        c.text
    );
}

#[test]
fn a_reply_subject_without_reply_headers_is_a_reply() {
    let raw = crlf(
        "Subject: Re: Plan\nContent-Type: text/html\n\n<p>Yes.</p><div class=\"gmail_quote\">On Mon Ana wrote:<blockquote>old</blockquote></div>\n",
    );
    assert_eq!(clean(&raw).text, "Yes.");
    let raw = crlf(
        "Subject: Fwd: Plan\nContent-Type: text/html\n\n<p>FYI</p><div class=\"gmail_quote\">Forwarded text</div>\n",
    );
    assert!(clean(&raw).text.contains("Forwarded text"));
}

#[test]
fn a_link_target_keeps_its_last_character() {
    let raw = crlf("Subject: L\nContent-Type: text/html\n\n<a href=\"https://example.org/p!\">Open</a> See https://example.org/q.\n");
    assert_eq!(
        clean(&raw).text,
        "[Open](https://example.org/p!) See https://example.org/q."
    ); // A link text with lines: the target keeps its last character too.
    let raw = crlf("Subject: L\nContent-Type: text/html\n\n<a href=\"https://example.org/p!?utm_source=x\"><p>One</p><p>Two</p></a>\n");
    assert_eq!(clean(&raw).text, "One\n\nTwo (<https://example.org/p!>)");
}

#[test]
fn a_spent_budget_still_lists_the_attachments_of_an_attached_message() {
    let notes = "n".repeat(1_000_000);
    let raw = crlf(&format!(
        "Subject: Two\nContent-Type: multipart/mixed; boundary=m\n\n--m\nContent-Type: text/plain\n\nHi\n\
         --m\nContent-Type: text/plain; name=notes.txt\n\n{notes}\n\
         --m\nContent-Type: message/rfc822\n\nSubject: Inner\nContent-Type: multipart/mixed; boundary=i\n\n\
         --i\nContent-Type: text/plain\n\nInner body.\n--i\nContent-Type: application/zip; name=a.zip\n\nPK\n--i--\n--m--\n"
    ));
    let c = clean(&raw);
    let names: Vec<&str> = c.attachments.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["notes.txt", "Inner", "a.zip"]);
    assert_eq!(c.attachments[1].text, None);
}

#[test]
fn the_joined_text_stays_inside_the_limit() {
    let part = "w ".repeat(10_000);
    let mut body = String::from("Subject: Many\nContent-Type: multipart/mixed; boundary=m\n\n");
    for _ in 0..5 {
        body.push_str(&format!("--m\nContent-Type: text/plain\n\n{part}\n"));
    }
    body.push_str("--m--\n");
    let c = clean(&crlf(&body));
    let without = c.text.strip_suffix(iris_clean::CUT_MARK).unwrap_or(&c.text);
    assert!(c.cut);
    assert_eq!(without.chars().count(), iris_clean::TEXT_LIMIT);
}

#[test]
fn an_attached_message_has_one_whole_cut_mark() {
    // 16 bytes of budget are left for an attached message with the text "x"
    // and a cut (its parts are nested deeper than the limit).
    let notes = "n".repeat(iris_clean::ATTACHMENT_TEXT_LIMIT - 16);
    let mut deep = String::from("Content-Type: text/plain\n\ndeep\n");
    for k in 0..12 {
        deep = format!("Content-Type: multipart/mixed; boundary=d{k}\n\n--d{k}\n{deep}--d{k}--\n");
    }
    let raw = crlf(&format!(
        "Subject: Two\nContent-Type: multipart/mixed; boundary=m\n\n--m\nContent-Type: text/plain\n\nHi\n\
         --m\nContent-Type: text/plain; name=notes.txt\n\n{notes}\n\
         --m\nContent-Type: message/rfc822\n\nSubject: Inner\nContent-Type: multipart/mixed; boundary=i\n\n\
         --i\nContent-Type: text/plain\n\nx\n--i\n{deep}--i--\n--m--\n"
    ));
    let c = clean(&raw);
    let t = c.attachments[1].text.as_deref().unwrap_or_default();
    assert_eq!(t, format!("x{}", iris_clean::CUT_MARK));
}

#[test]
fn a_hidden_part_after_a_full_text_is_no_cut() {
    let part = "w".repeat(iris_clean::TEXT_LIMIT);
    let raw = crlf(&format!(
        "Subject: Full\nContent-Type: multipart/mixed; boundary=m\n\n--m\nContent-Type: text/plain\n\n{part}\n\
         --m\nContent-Type: text/html\n\n<div hidden>secret</div>\n--m--\n"
    ));
    let c = clean(&raw);
    assert!(!c.cut);
    assert_eq!(c.text, part);
}

#[test]
fn an_attached_message_at_the_depth_limit_is_cut() {
    let mut part = String::from("Content-Type: message/rfc822\n\nSubject: Deep\n\nToo deep.\n");
    // The top message is depth 0. Each multipart adds one level.
    for k in 0..MAX_DEPTH {
        part = format!("Content-Type: multipart/mixed; boundary=d{k}\n\n--d{k}\n{part}--d{k}--\n");
    }
    let raw = crlf(&format!("Subject: Top\n{part}"));
    let c = clean(&raw);
    let inner = c
        .attachments
        .iter()
        .find(|a| a.mime_type == "message/rfc822")
        .expect("listed");
    assert_eq!(inner.text.as_deref(), Some(CUT_MARK));
}

#[test]
fn a_part_with_too_many_lines_is_cut() {
    let lines = "a\n".repeat(iris_clean::MAX_LINES + 10);
    let c = clean(&crlf(&format!("Subject: Lines\n\n{lines}")));
    assert!(c.cut);
}
