//! The links of the clean text: each HTML link whose text is not its target
//! (`Clean::links`), for the link check of spec 1 section 6.7.

use iris_clean::{clean, clean_cut, Link, LINK_BYTES};

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
fn a_cut_message_cuts_the_links_only_when_a_body_can_be_past_the_cut() {
    // The body ends before the cut, in the attachment: the list is whole.
    let raw = mixed(true);
    let c = clean_cut(&raw[..raw.len() - 1000], true);
    assert_eq!(c.links.len(), 1);
    assert!(!c.links_cut);
    // The body is past the cut.
    let raw = mixed(false);
    assert!(clean_cut(&raw[..2000], true).links_cut);
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
