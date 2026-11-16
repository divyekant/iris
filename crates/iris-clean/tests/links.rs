//! The links of the clean text: each HTML link whose text is not its target
//! (`Clean::links`), for the link check of spec 1 section 6.7.

use iris_clean::{clean, Link, LINK_BYTES};

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
            link("Bank", "https://bank.example.com\t@evil.example.net/x"),
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
