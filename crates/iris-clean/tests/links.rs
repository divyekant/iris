//! The links of the clean text: each HTML link whose text is not its target
//! (`Clean::links`), for the link check of spec 1 section 6.7.

use iris_clean::{browser_href, clean, clean_cut, html_part_in, Link, LINK_BYTES};

fn html(body: &str) -> Vec<u8> {
    format!("From: a@example.com\r\nSubject: x\r\nContent-Type: text/html; charset=utf-8\r\n\r\n{body}\r\n")
        .into_bytes()
}

fn link(text: &str, href: &str) -> Link {
    Link {
        text: text.into(),
        href: href.into(),
    }
}

#[test]
fn each_link_whose_text_is_not_its_target_is_listed() {
    let c = clean(&html(
        "<p><a href=\"https://evil.example.net/\">https://bank.example.com<br>/login</a>\
         <a href=\"https://bank.example.com&#9;@evil.example.net/x\">Bank</a>\
         <a href=\"https://a.example.com/\">https://a.example.com/</a>\
         <a href=\"https://b.example.com/\"><img src=\"x.png\"></a></p>",
    ));
    assert_eq!(
        c.links,
        vec![
            link(
                "https://bank.example.com\n/login",
                "https://evil.example.net/"
            ),
            link("Bank", "https://bank.example.com@evil.example.net/x"),
        ]
    );
    assert!(!c.links_cut);
}

#[test]
fn a_hidden_link_is_not_listed() {
    let c = clean(&html(
        "<p>Hi</p><div style=\"display:none\"><a href=\"https://evil.example.net/\">bank.example.com</a></div>",
    ));
    assert!(c.links.is_empty());
}

#[test]
fn the_html_version_counts_also_when_the_text_uses_another_version() {
    let raw = "From: a@example.com\r\nSubject: x\r\nContent-Type: multipart/alternative; boundary=b\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nHello.\r\n\
--b\r\nContent-Type: text/html\r\n\r\n<a href=\"https://evil.example.net/\">bank.example.com</a>\r\n\
--b--\r\n";
    let c = clean(raw.as_bytes());
    assert_eq!(
        c.links,
        vec![link("bank.example.com", "https://evil.example.net/")]
    );
}

#[test]
fn links_past_the_byte_limit_set_the_cut_flag() {
    let one = format!(
        "<a href=\"https://e.example.net/{}\">x</a> ",
        "a".repeat(1000)
    );
    let c = clean(&html(&one.repeat(LINK_BYTES / 1000 + 10)));
    assert!(c.links_cut);
    let bytes: usize = c.links.iter().map(|l| l.text.len() + l.href.len()).sum();
    assert!(bytes <= LINK_BYTES);
}

#[test]
fn a_web_link_in_any_form_that_a_browser_reads_is_listed() {
    let c = clean(&html(
        "<a href=\"https:evil.example.net\">shop.example.com</a>\
         <a href=\" ht&#9;tps:\\\\evil.example.net\">shop.example.com</a>\
         <a href=\"HTTP://evil.example.net\">shop.example.com</a>",
    ));
    let hrefs: Vec<&str> = c.links.iter().map(|l| l.href.as_str()).collect();
    assert_eq!(
        hrefs,
        [
            "https:evil.example.net",
            "https:\\\\evil.example.net",
            "HTTP://evil.example.net"
        ]
    );
}

/// A message with an HTML part and a large attachment.
fn mixed(first_html: bool) -> Vec<u8> {
    let html = "Content-Type: text/html\r\n\r\n<a href=\"https://evil.example.net/\">bank.example.com</a>\r\n";
    let file = format!(
        "Content-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=a.bin\r\n\r\n{}\r\n",
        "x".repeat(4000)
    );
    let (a, b) = if first_html {
        (html.to_string(), file)
    } else {
        (file, html.to_string())
    };
    format!(
        "From: a@example.com\r\nSubject: x\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\n{a}--b\r\n{b}--b--\r\n"
    )
    .into_bytes()
}

#[test]
fn a_cut_message_cuts_the_links_only_when_an_html_part_can_be_past_the_cut() {
    // The body ends before the cut, in the attachment: the list is whole.
    let raw = mixed(true);
    let c = clean_cut(&raw[..raw.len() - 1000], true);
    assert_eq!(c.links.len(), 1);
    assert!(!c.links_cut);
    // The HTML body is past the cut: the rest names it.
    let raw = mixed(false);
    assert!(!clean_cut(&raw[..2000], true).links_cut);
    assert!(html_part_in(&raw[..2000], &raw[2000..]).unwrap());
    let raw = mixed(true);
    assert!(!html_part_in(&raw[..2000], &raw[2000..]).unwrap());
    // The cut is in the HTML part.
    let raw = mixed(true);
    let end = raw.windows(9).position(|w| w == b"bank.exam").unwrap();
    assert!(clean_cut(&raw[..end], true).links_cut);
}

#[test]
fn an_html_part_past_the_part_limit_cuts_the_links() {
    let body = format!(
        "<p>{}</p><a href=\"https://evil.example.net/\">bank.example.com</a>",
        "a".repeat(iris_clean::PART_LIMIT)
    );
    assert!(clean(&html(&body)).links_cut);
}

/// The message header of a multipart message with the boundary `b`.
const TOP: &[u8] =
    b"From: a@example.com\r\nContent-Type: multipart/mixed;\r\n\tboundary=\"b\"\r\n\r\n";

fn with_top(before: &[u8]) -> Vec<u8> {
    [TOP, before].concat()
}

#[test]
fn html_part_in_finds_an_html_part_header_past_the_cut() {
    for (before, rest) in [
        (&b""[..], &b"--b\r\nContent-Type: text/html\r\n"[..]),
        (b"--b\r\n", b"content-type: TEXT/HTML; charset=utf-8"),
        (b"--b\r\nContent-Type: text", b"\r\n /\r\n html"),
        (b"", b"--B  \r\nContent-Type:\r\n\ttext/html\r\n\r\n<p>"),
        (
            b"--b\r\nContent-Type: text/plain\r\n\r\nQU",
            b"JD\r\n--b\r\nX: y\r\nContent-Type: text/html\r\n\r\n",
        ),
        // The header ends past the cut, so its body is past the cut.
        (b"--b\r\nContent-Type: text/html", b"\r\n\r\n<p>"),
        // A boundary with white space around `=`.
        (
            b"--b\r\nContent-Type: multipart/related; boundary = \"in\"\r\n\r\n",
            b"--in\r\nContent-Type: text/html\r\n\r\n",
        ),
        // A nested multipart declares its own boundary.
        (
            b"--b\r\nContent-Type: multipart/alternative; boundary=in\r\n\r\n",
            b"--in\r\nContent-Type: text/html\r\n\r\n",
        ),
    ] {
        let before = with_top(before);
        assert!(
            html_part_in(&before, rest).unwrap(),
            "{}|{}",
            String::from_utf8_lossy(&before),
            String::from_utf8_lossy(rest)
        );
    }
    for (before, rest) in [
        (&b"--b\r\n"[..], &b"Content-Type: text/plain"[..]),
        (b"--b\r\n", b"text/htm"),
        (b"--b\r\n", b"xxtextext/html"),
        // `text/html` in the body of a plain attachment, also as a line
        // that looks like a header, also after a line with `--`.
        (
            b"--b\r\nContent-Type: text/plain\r\n\r\nAAAA",
            b"AAA\r\nContent-Type: text/html\r\n",
        ),
        (
            b"",
            b"--b\r\nContent-Type: text/plain\r\n\r\nSee text/html.\r\nContent-Type: text/html\r\n",
        ),
        (
            b"--b\r\nContent-Type: text/plain\r\n\r\n",
            b"-- note\r\nContent-Type: text/html\r\n\r\n",
        ),
        (
            b"--b\r\nContent-Type: text/plain\r\n\r\n",
            b"--bb\r\nContent-Type: text/html\r\n\r\n",
        ),
        (
            b"--b\r\nContent-Type: text/plain\r\n\r\n",
            b"--b-- x\r\nContent-Type: text/html\r\n\r\n",
        ),
        (
            b"--b\r\nX-Note: text/html\r\nContent-Type: text/csv\r\n\r\n",
            b"",
        ),
        // An HTML part before the cut does not count.
        (
            b"--b\r\nContent-Type: text/html\r\n\r\n<p>",
            b"more</p>\r\n",
        ),
    ] {
        let before = with_top(before);
        assert!(
            !html_part_in(&before, rest).unwrap(),
            "{}|{}",
            String::from_utf8_lossy(&before),
            String::from_utf8_lossy(rest)
        );
    }
    // With no declared boundary, a `--b` line starts no part header.
    assert!(!html_part_in(
        b"Subject: x\r\n\r\n",
        &b"--b\r\nContent-Type: text/html\r\n\r\n"[..]
    )
    .unwrap());
}

#[test]
fn each_html_version_of_an_alternative_adds_its_links() {
    let raw = "From: a@example.com\r\nSubject: x\r\nContent-Type: multipart/alternative; boundary=b\r\n\r\n\
--b\r\nContent-Type: text/html\r\n\r\n<a href=\"https://evil.example.net/\">bank.example.com</a>\r\n\
--b\r\nContent-Type: text/html\r\n\r\n<p>Hello.</p>\r\n\
--b--\r\n";
    let c = clean(raw.as_bytes());
    assert_eq!(c.text, "Hello.");
    assert_eq!(
        c.links,
        vec![link("bank.example.com", "https://evil.example.net/")]
    );
    assert!(c.attachments.is_empty());
}

/// A small xorshift generator with a fixed seed, so a failure repeats.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// Generated HTML with anchors: each anchor with a web or mail target and a
/// text that is not its target is in the list, in order, with the href as
/// a browser reads it. Nothing else is in the list.
#[test]
fn generated_anchors_give_the_link_list() {
    let hrefs = [
        "https://a.example.net/x",
        "https:b.example.net",
        " HTTP://c.example.net/ ",
        "ht&#9;tps://d.example.net/",
        "https://e.example.net\\@f.example.com/",
        "mailto:g@example.net",
        "javascript:run()",
        "/relative",
        "",
    ];
    let texts = [
        "bank.example.com",
        "Click here",
        "https://h.example.com/a",
        "W\u{fc}rze",
    ];
    let noise = [
        "<p>", "</p>", "<br>", "<b>", "</b>", "word ", "&amp; ", "<div>", "</div>",
    ];
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..300 {
        let mut body = String::new();
        let mut expected = Vec::new();
        for _ in 0..r.below(12) {
            body.push_str(noise[r.below(noise.len())]);
            if r.below(2) == 0 {
                let (h, t) = (hrefs[r.below(hrefs.len())], texts[r.below(texts.len())]);
                body.push_str(&format!("<a href=\"{h}\">{t}</a> "));
                let read = browser_href(&h.replace("&#9;", "\t"));
                let lower = read.to_ascii_lowercase();
                if ["http:", "https:", "mailto:"]
                    .iter()
                    .any(|p| lower.starts_with(p))
                {
                    expected.push(link(t, &read));
                }
            }
        }
        let c = clean(&html(&body));
        assert!(!c.links_cut, "{body}");
        assert_eq!(c.links, expected, "{body}");
    }
}

/// A reader that gives the bytes in chunks of random size, so the type can
/// cross each read boundary.
struct Chunks<'a> {
    bytes: &'a [u8],
    rng: Rng,
}

impl std::io::Read for Chunks<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.bytes.len().min(buf.len()).min(1 + self.rng.below(16));
        buf[..n].copy_from_slice(&self.bytes[..n]);
        self.bytes = &self.bytes[n..];
        Ok(n)
    }
}

/// Generated MIME parts, each with a type in random case and folding, and
/// a body that can name `text/html`, cut at the start of a part or in a
/// body, and read in random chunks: `html_part_in` is true exactly when a
/// part past the cut has the type `text/html` in its header.
#[test]
fn generated_parts_give_the_html_answer_of_their_headers() {
    let types = [
        "text/html",
        "text/plain",
        "application/pdf",
        "text/htm",
        "text/html; charset=utf-8",
    ];
    let bodies = [
        "QUJDRA==",
        "See text/html here.",
        "Content-Type: text/html",
        " text/html",
        "",
        "-- note",
        "--bb",
        "--b-- x",
    ];
    let mut r = Rng(0xD1B5_4A32_D192_ED03);
    let mut found = 0;
    for _ in 0..3000 {
        let tops: [&[u8]; 5] = [
            TOP,
            b"Content-Type: multipart/mixed; boundary=b\r\n\r\n",
            b"Content-Type: multipart/mixed; boundary = \"b\"\r\n\r\n",
            b"Content-Type: Multipart/Mixed;\r\n BOUNDARY=\r\n b ; x=y\r\n\r\n",
            b"Content-Type: multipart/mixed; charset=x;\tboundary =b\r\n\r\n",
        ];
        let mut bytes = tops[r.below(tops.len())].to_vec();
        // For each part: its start, the start of its body, and its type.
        let mut parts = Vec::new();
        for _ in 0..r.below(5) {
            let start = bytes.len();
            bytes.extend_from_slice(b"--b\r\n");
            if r.below(2) == 0 {
                bytes.extend_from_slice(b"Content-Disposition: attachment\r\n");
            }
            let t = types[r.below(types.len())];
            let t: String = t
                .chars()
                .map(|c| {
                    if r.below(2) == 0 {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    }
                })
                .collect();
            let fold = ["", "\r\n ", "\r\n\t", " "][r.below(4)];
            bytes.extend_from_slice(format!("Content-Type:{fold}{t}\r\n\r\n").as_bytes());
            let body = bytes.len();
            for _ in 0..r.below(4) {
                bytes.extend_from_slice(bodies[r.below(bodies.len())].as_bytes());
                bytes.extend_from_slice(b"\r\n");
            }
            parts.push((start, body, t.to_ascii_lowercase().starts_with("text/html")));
        }
        // The cut: the start of part `i`, or a place in the body of part
        // `i - 1` (then part `i - 1` is before the cut).
        let i = r.below(parts.len() + 1);
        let cut = match i.checked_sub(1).map(|p| parts[p]) {
            Some((_, body, _)) if r.below(2) == 0 => {
                let end = parts.get(i).map_or(bytes.len(), |p| p.0);
                body + r.below(end - body + 1)
            }
            _ => parts.get(i).map_or(bytes.len(), |p| p.0),
        };
        let expected = parts[i..].iter().any(|p| p.2);
        found += usize::from(expected);
        let rng = Rng(r.0 | 1);
        let got = html_part_in(
            &bytes[..cut],
            Chunks {
                bytes: &bytes[cut..],
                rng,
            },
        )
        .unwrap();
        assert_eq!(
            got,
            expected,
            "{:?}|{:?}",
            String::from_utf8_lossy(&bytes[..cut]),
            String::from_utf8_lossy(&bytes[cut..])
        );
    }
    assert!(found > 300 && found < 2700, "{found}");
}

#[test]
fn a_shown_link_past_the_html_stack_limit_cuts_the_links() {
    let deep = |open: &str| {
        format!(
            "{open}{}<a href=\"https://evil.example.net/\">bank.example.com</a>",
            "<span>".repeat(600)
        )
    };
    let c = clean(&html(&deep("")));
    assert!(c.links.is_empty());
    assert!(c.links_cut, "a browser shows the link");
    // Deep content that is hidden from the start hides no shown link.
    let c = clean(&html(&deep("<div style=\"display:none\">")));
    assert!(!c.links_cut);
}

#[test]
fn a_limit_of_the_part_scan_counts_as_an_html_part() {
    // A boundary after 1,000 bytes of folded parameters: the scan reads it.
    let folded = |n: usize| {
        let mut head = b"Content-Type: multipart/mixed;\r\n".to_vec();
        for i in 0..n {
            head.extend_from_slice(format!(" x{i}=y;\r\n").as_bytes());
        }
        head.extend_from_slice(b" boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nAAAA");
        head
    };
    let head = folded(150);
    assert!(head.len() > 1000);
    assert!(html_part_in(&head, &b"AAA\r\n--b\r\nContent-Type: text/html\r\n\r\n"[..]).unwrap());
    assert!(!html_part_in(
        &head,
        &b"AAA\r\n--b\r\nContent-Type: text/plain\r\n\r\n"[..]
    )
    .unwrap());
    // Past the field limit: the scan cannot know the boundary.
    let head = folded(1500);
    assert!(head.len() > 8 * 1024);
    assert!(html_part_in(
        &head,
        &b"AAA\r\n--b\r\nContent-Type: text/plain\r\n\r\n"[..]
    )
    .unwrap());
    // One `Content-Type` line past 1,000 bytes.
    let long = format!(
        "Content-Type: multipart/mixed; x=\"{}\"; boundary=b\r\n\r\n",
        "a".repeat(1200)
    );
    assert!(html_part_in(long.as_bytes(), &b"--b\r\n"[..]).unwrap());
    // More boundaries than the scan keeps.
    let mut many = Vec::new();
    for i in 0..70 {
        many.extend_from_slice(
            format!("Content-Type: multipart/mixed; boundary=b{i}\r\n").as_bytes(),
        );
    }
    assert!(html_part_in(&many, &b""[..]).unwrap());
    // A long line in a body does not count.
    let body = format!("{}\r\n", "a".repeat(5000));
    assert!(!html_part_in(
        &with_top(b"--b\r\nContent-Type: text/plain\r\n\r\n"),
        body.as_bytes()
    )
    .unwrap());
}

#[test]
fn a_link_with_an_unknown_named_reference_cuts_the_links() {
    for body in [
        "<a href=\"https://evil.example.net/\">bank&period;example.com</a>",
        "<a href=\"https&colon;//evil.example.net/\">bank.example.com</a>",
        "<a href=\"ht&Tab;tps://evil.example.net/\">bank.example.com</a>",
    ] {
        assert!(clean(&html(body)).links_cut, "{body}");
    }
    // Known references, an `&` with no name and `;`, and an empty anchor
    // (a person sees no link) do not cut.
    for body in [
        "<a href=\"https&colon;//evil.example.net/\"></a><p>Hello</p>",
        "<a href=\"https://shop.example.com/?a=1&amp;b=2\">Terms &amp; rules</a>",
        "<a href=\"https://shop.example.com/?a=1&b=2\">AT&T news</a>",
    ] {
        assert!(!clean(&html(body)).links_cut, "{body}");
    }
}

/// A message with `inner` as an attached `message/rfc822` part.
fn with_attached(inner: &str) -> Vec<u8> {
    format!(
        "From: a@example.com\r\nSubject: x\r\nContent-Type: multipart/mixed; boundary=o\r\n\r\n\
--o\r\nContent-Type: text/plain\r\n\r\nSee the mail below.\r\n\
--o\r\nContent-Type: message/rfc822\r\n\r\n{inner}\r\n--o--\r\n"
    )
    .into_bytes()
}

/// The links of an attached message, also of a message attached to it, are
/// in `attached_links`, not in `links` (issue #372).
#[test]
fn the_links_of_an_attached_message_are_listed() {
    let inner = "From: b@bank.example.com\r\nSubject: y\r\nContent-Type: text/html\r\n\r\n\
<a href=\"https://evil.example.net/\">bank.example.com</a>";
    let c = clean(&with_attached(inner));
    assert!(c.links.is_empty());
    assert_eq!(
        c.attached_links,
        vec![link("bank.example.com", "https://evil.example.net/")]
    );
    assert!(!c.links_cut);
    // One level deeper.
    let deep = format!(
        "From: c@example.org\r\nSubject: z\r\nContent-Type: multipart/mixed; boundary=i\r\n\r\n\
--i\r\nContent-Type: text/plain\r\n\r\nFwd.\r\n\
--i\r\nContent-Type: message/rfc822\r\n\r\n{inner}\r\n--i--"
    );
    let c = clean(&with_attached(&deep));
    assert_eq!(
        c.attached_links,
        vec![link("bank.example.com", "https://evil.example.net/")]
    );
}

/// An attached message past the read limit with an HTML part after the
/// limit cuts the link list. Plain text after the limit does not.
#[test]
fn an_html_part_past_the_limit_of_an_attached_message_cuts_the_links() {
    let pad = "a".repeat(iris_clean::INNER_LIMIT);
    let html = format!(
        "From: b@example.com\r\nSubject: y\r\nContent-Type: multipart/mixed; boundary=i\r\n\r\n\
--i\r\nContent-Type: application/octet-stream\r\n\r\n{pad}\r\n\
--i\r\nContent-Type: text/html\r\n\r\n<a href=\"https://evil.example.net/\">bank.example.com</a>\r\n--i--"
    );
    assert!(clean(&with_attached(&html)).links_cut);
    let plain = html.replace("text/html", "text/plain");
    assert!(!clean(&with_attached(&plain)).links_cut);
}

/// Each link of an attached message that Iris does not read sets
/// `links_cut` (issue #372): a nested message past the read limit of an
/// attached message, a cut of the raw message inside an attached message,
/// and an attached message past the cut.
#[test]
fn an_unread_attached_message_cuts_the_links() {
    let pad = "a".repeat(iris_clean::INNER_LIMIT);
    let nested = format!(
        "From: b@example.com\r\nSubject: y\r\nContent-Type: multipart/mixed; boundary=i\r\n\r\n\
--i\r\nContent-Type: application/octet-stream\r\n\r\n{pad}\r\n\
--i\r\nContent-Type: message/rfc822\r\n\r\n\
From: c@example.com\r\nSubject: z\r\nContent-Type: text/plain\r\n\r\nHi.\r\n--i--"
    );
    assert!(clean(&with_attached(&nested)).links_cut);

    let inner = "From: b@bank.example.com\r\nSubject: y\r\nContent-Type: text/html\r\n\r\n\
<p>Hello</p><a href=\"https://evil.example.net/\">bank.example.com</a>";
    let raw = with_attached(inner);
    let at = raw.windows(5).position(|w| w == b"Hello").unwrap();
    // The cut is inside the attached message.
    assert!(clean_cut(&raw[..at], true).links_cut);
    assert!(!clean_cut(&raw, false).links_cut);
    // The attached message starts past the cut.
    let start = raw
        .windows(14)
        .position(|w| w == b"message/rfc822")
        .unwrap()
        - 14;
    assert!(html_part_in(&raw[..start], &raw[start..]).unwrap());
}

/// Invariant: each attached message of the MIME tree gets its links
/// checked, whichever version of a `multipart/alternative` gives the clean
/// text. A version that the text does not use adds no attachment (review
/// finding on #1122, issue #1140).
#[test]
fn an_attached_message_in_each_version_of_an_alternative_is_checked() {
    let inner = "From: b@bank.example.com\r\nSubject: y\r\nContent-Type: text/html\r\n\r\n\
<a href=\"https://evil.example.net/\">bank.example.com</a>";
    let plain = "Content-Type: text/plain\r\n\r\nSee the mail.\r\n";
    let attached = format!("Content-Type: message/rfc822\r\n\r\n{inner}\r\n");
    // The attached message as the last version (no text, so the text uses
    // the plain version), as the first version (the plain version is used
    // first), and inside a nested multipart of a version that is not used.
    let nested =
        format!("Content-Type: multipart/mixed; boundary=n\r\n\r\n--n\r\n{attached}--n--\r\n");
    for versions in [
        vec![plain.to_string(), attached.clone()],
        vec![attached.clone(), plain.to_string()],
        vec![plain.to_string(), nested.clone()],
        vec![nested.clone(), plain.to_string()],
    ] {
        let body: String = versions.iter().map(|v| format!("--a\r\n{v}")).collect();
        let raw = format!(
            "From: a@example.com\r\nSubject: x\r\n\
Content-Type: multipart/alternative; boundary=a\r\n\r\n{body}--a--\r\n"
        );
        let c = clean(raw.as_bytes());
        assert_eq!(c.text, "See the mail.", "{versions:?}");
        assert_eq!(
            c.attached_links,
            vec![link("bank.example.com", "https://evil.example.net/")],
            "{versions:?}"
        );
        assert!(c.attachments.is_empty(), "{versions:?}");
    }
}

/// One part of a generated MIME tree.
enum Node {
    Plain,
    Html,
    /// An attached message with one deceptive link, number `k`.
    Attached(usize),
    /// A file attachment.
    File,
    Mixed(Vec<Node>),
    Alt(Vec<Node>),
}

impl Node {
    fn gen(r: &mut Rng, depth: usize, next: &mut usize) -> Node {
        let leaf = depth >= 3 || r.below(3) == 0;
        match (leaf, r.below(4)) {
            (true, 0) => Node::Plain,
            (true, 1) => Node::Html,
            (true, 2) => {
                *next += 1;
                Node::Attached(*next)
            }
            (true, _) => Node::File,
            (false, k) => {
                let kids = (0..1 + r.below(3))
                    .map(|_| Node::gen(r, depth + 1, next))
                    .collect();
                if k % 2 == 0 {
                    Node::Mixed(kids)
                } else {
                    Node::Alt(kids)
                }
            }
        }
    }

    fn has_text(&self) -> bool {
        match self {
            Node::Plain | Node::Html => true,
            Node::Attached(_) | Node::File => false,
            Node::Mixed(k) | Node::Alt(k) => k.iter().any(Node::has_text),
        }
    }

    /// Each attached message (`all`), and the attachment names that the
    /// message lists (`shown`): an unused version of an alternative lists
    /// none. The last version with text is the used one.
    fn expect(&self, used: bool, all: &mut Vec<usize>, shown: &mut Vec<String>) {
        match self {
            Node::Plain | Node::Html => {}
            Node::Attached(k) => {
                all.push(*k);
                if used {
                    shown.push(format!("s{k}"));
                }
            }
            Node::File => {
                if used {
                    shown.push("f.bin".into());
                }
            }
            Node::Mixed(kids) => kids.iter().for_each(|n| n.expect(used, all, shown)),
            Node::Alt(kids) => {
                let pick = kids.iter().rposition(Node::has_text);
                for (i, n) in kids.iter().enumerate() {
                    n.expect(used && pick == Some(i), all, shown);
                }
            }
        }
    }

    fn mime(&self, b: &mut usize) -> String {
        match self {
            Node::Plain => "Content-Type: text/plain\r\n\r\nPlain words.\r\n".into(),
            Node::Html => "Content-Type: text/html\r\n\r\n<p>Html words.</p>\r\n".into(),
            Node::Attached(k) => format!(
                "Content-Type: message/rfc822\r\n\r\nFrom: b@example.com\r\nSubject: s{k}\r\n\
Content-Type: text/html\r\n\r\n<a href=\"https://evil{k}.example.net/\">bank{k}.example.com</a>\r\n"
            ),
            Node::File => "Content-Type: application/octet-stream\r\n\
Content-Disposition: attachment; filename=f.bin\r\n\r\nxyz\r\n"
                .into(),
            Node::Mixed(kids) | Node::Alt(kids) => {
                *b += 1;
                let id = *b;
                let sub = if matches!(self, Node::Mixed(_)) {
                    "mixed"
                } else {
                    "alternative"
                };
                let body: String = kids
                    .iter()
                    .map(|n| format!("--b{id}\r\n{}", n.mime(b)))
                    .collect();
                format!("Content-Type: multipart/{sub}; boundary=b{id}\r\n\r\n{body}--b{id}--\r\n")
            }
        }
    }
}

/// Generated trees of `multipart/alternative` and `multipart/mixed` parts:
/// each attached message gets its links checked, in a used version or not,
/// and only the used versions list attachments (issue #1140).
#[test]
fn generated_alternatives_check_each_attached_message() {
    let mut r = Rng(0x1140_2026_1125);
    for round in 0..400 {
        let mut next = 0;
        let tree = Node::Alt(
            (0..1 + r.below(3))
                .map(|_| Node::gen(&mut r, 1, &mut next))
                .collect(),
        );
        let raw = format!("From: a@example.com\r\nSubject: x\r\n{}", tree.mime(&mut 0));
        let (mut all, mut shown) = (Vec::new(), Vec::new());
        tree.expect(true, &mut all, &mut shown);
        let c = clean(raw.as_bytes());
        assert!(!c.links_cut, "{round}: {raw}");
        let mut got: Vec<Link> = c.attached_links.clone();
        got.sort_by(|a, b| a.href.cmp(&b.href));
        let mut want: Vec<Link> = all
            .iter()
            .map(|k| {
                link(
                    &format!("bank{k}.example.com"),
                    &format!("https://evil{k}.example.net/"),
                )
            })
            .collect();
        want.sort_by(|a, b| a.href.cmp(&b.href));
        assert_eq!(got, want, "{round}: {raw}");
        let mut names: Vec<String> = c.attachments.iter().map(|a| a.name.clone()).collect();
        names.sort();
        shown.sort();
        assert_eq!(names, shown, "{round}: {raw}");
    }
}
