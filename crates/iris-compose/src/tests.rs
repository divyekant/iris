use super::*;

const ME: &str = "owner@example.com";

fn content(to: &[&str], subject: &str, body: &str) -> Content {
    Content {
        to: to.iter().map(|s| s.to_string()).collect(),
        cc: Vec::new(),
        subject: subject.into(),
        body: body.into(),
    }
}

/// A small seeded generator (xorshift), so a failure repeats.
struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    /// A string of up to `max` characters from a pool with the hard cases:
    /// CR, LF, tabs, quotes, `=`, header-like text, non-ASCII and long words.
    fn text(&mut self, max: usize) -> String {
        const POOL: &[&str] = &[
            "a", "Z", "0", " ", "  ", "\r", "\n", "\r\n", "\t", "=", "=?utf-8?q?x?=", "\"", "<", ">",
            ",", ";", ":", "@", "\\", ".", "é", "日本", "😀", "\u{0}", "\u{7f}", "Bcc: x@y.z",
            "\nFrom: a@b.c", "--", "Re:", "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        ];
        let n = self.below(max + 1);
        (0..n).map(|_| POOL[self.below(POOL.len())]).collect()
    }
    fn address(&mut self) -> String {
        const LOCAL: &[&str] = &["a", "b.c", "x+y", "Q_1", "o'n", "a b", "a\r\nb", "é", ""];
        const DOMAIN: &[&str] = &[
            "example.com",
            "ex-1.org",
            "x",
            "a..b",
            "-a.com",
            "ü.de",
            "b.c",
        ];
        format!(
            "{}@{}",
            LOCAL[self.below(LOCAL.len())],
            DOMAIN[self.below(DOMAIN.len())]
        )
    }
}

/// Each content that the checks accept composes and reads back to the
/// same content and hash, with no header that Iris does not write. Each
/// refused content names why.
#[test]
fn checked_content_reads_back_the_same() {
    let mut g = Gen(0x9e37_79b9_7f4a_7c15);
    let mut accepted = 0;
    for i in 0..3000 {
        let mut raw_content = Content {
            to: (0..1 + g.below(3)).map(|_| g.address()).collect(),
            cc: (0..g.below(3)).map(|_| g.address()).collect(),
            subject: g.text(12),
            body: g.text(60),
        };
        // Two in three cases keep only what the checks can accept, so that
        // most cases reach compose; the others test the refusals.
        if i % 3 != 0 {
            raw_content.subject.retain(|c| !c.is_control());
            raw_content.subject.push('s');
            raw_content.body.retain(|c| c != '\0');
            for list in [&mut raw_content.to, &mut raw_content.cc] {
                list.retain(|a| check_address(&a.to_ascii_lowercase()).is_ok());
            }
            if raw_content.to.is_empty() {
                raw_content.to.push("x+y@ex-1.org".into());
            }
        }
        let Ok(c) = raw_content.clone().check() else {
            continue;
        };
        accepted += 1;
        let reply = (i % 2 == 0).then(|| references("<p.1@x.org>", &["r.0@x.org".into()], None));
        let raw = compose(ME, "iris.1@example.com", &c, reply.as_ref(), 1_800_000_000);
        let back = read(&raw, ME)
            .unwrap_or_else(|| panic!("no read back of {c:?}:\n{}", String::from_utf8_lossy(&raw)));
        assert_eq!(back.content, c, "{}", String::from_utf8_lossy(&raw));
        assert_eq!(back.reply, reply);
        assert_eq!(back.message_id, "iris.1@example.com");
        assert_eq!(
            hash(&back.content, back.reply.as_ref()),
            hash(&c, reply.as_ref())
        );
        // No CR or LF of the input reaches a header line of its own.
        let head = &raw[..raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap()];
        for line in String::from_utf8_lossy(head).split("\r\n") {
            let name = line.split(':').next().unwrap().to_ascii_lowercase();
            assert!(
                line.starts_with([' ', '\t']) || HEADERS.contains(&name.as_str()),
                "{line:?}"
            );
        }
    }
    assert!(accepted > 300, "only {accepted} accepted");
}

#[test]
fn checks_refuse_bad_content() {
    let ok = content(&["a@example.com"], "Hi", "Body");
    assert!(ok.clone().check().is_ok());
    for bad in [
        content(&[], "Hi", "x"),
        content(&["a@example.com"], "", "x"),
        content(&["a@example.com"], "  ", "x"),
        content(&["a@example.com"], "a\nBcc: b@c.d", "x"),
        content(&["a@example.com"], "a\rb", "x"),
        content(&["a@example.com"], "=?utf-8?q?x?=", "x"),
        content(&["a@example.com"], "Hi", "a\0b"),
        content(&["a@example.com"], &"s".repeat(MAX_SUBJECT + 1), "x"),
        content(&["a@example.com"], "Hi", &"b".repeat(MAX_BODY + 1)),
        content(&["a b@example.com"], "Hi", "x"),
        content(&["a@example.com\r\nBcc: x@y.z"], "Hi", "x"),
        content(&["<a@example.com>"], "Hi", "x"),
        content(&["a@example.com, b@example.com"], "Hi", "x"),
        content(&["a@localhost"], "Hi", "x"),
        content(&["é@example.com"], "Hi", "x"),
    ] {
        assert!(bad.clone().check().is_err(), "{bad:?}");
    }
    let many: Vec<String> = (0..=MAX_RECIPIENTS)
        .map(|i| format!("p{i}@example.com"))
        .collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    assert!(content(&refs, "Hi", "x").check().is_err());
}

#[test]
fn check_folds_case_and_removes_duplicates() {
    let mut c = content(&["A@Example.com", "a@example.com"], "Hi", "a\r\nb\rc");
    c.cc = vec!["a@EXAMPLE.com".into(), "b@example.com".into()];
    let c = c.check().unwrap();
    assert_eq!(c.to, ["a@example.com"]);
    assert_eq!(c.cc, ["b@example.com"]);
    assert_eq!(c.body, "a\nb\nc");
}

#[test]
fn a_reply_has_the_thread_headers() {
    let r = references(
        "<p@x.org>",
        &["<a@x.org>".into(), "b@x.org".into()],
        Some("<b@x.org>"),
    );
    assert_eq!(r.in_reply_to, "p@x.org");
    assert_eq!(r.references, ["a@x.org", "b@x.org", "p@x.org"]);
    let r = references("p@x.org", &[], Some("<q@x.org>"));
    assert_eq!(r.references, ["q@x.org", "p@x.org"]);
    let r = references("p@x.org", &[], None);
    assert_eq!(r.references, ["p@x.org"]);

    assert_eq!(reply_subject("Q4 numbers"), "Re: Q4 numbers");
    assert_eq!(reply_subject("RE: Q4"), "RE: Q4");
    assert_eq!(reply_subject("a\r\nb"), "Re: a  b");

    let c = content(&["p@x.org"], &reply_subject("Q4"), "ok")
        .check()
        .unwrap();
    let raw = compose(ME, "iris.2@example.com", &c, Some(&r), 1_800_000_000);
    let text = String::from_utf8_lossy(&raw);
    assert!(text.contains("In-Reply-To: <p@x.org>\r\n"), "{text}");
    assert!(text.contains("References: <p@x.org>\r\n"), "{text}");
    assert!(text.contains("Subject: Re: Q4\r\n"), "{text}");
}

#[test]
fn a_draft_that_the_person_changed_does_not_read_back() {
    let c = content(&["p@x.org"], "Hi", "Body").check().unwrap();
    let raw =
        String::from_utf8(compose(ME, "iris.3@example.com", &c, None, 1_800_000_000)).unwrap();
    assert!(read(raw.as_bytes(), ME).is_some());
    assert!(read(raw.as_bytes(), "other@example.com").is_none());
    for (from, to) in [
        ("To: ", "Bcc: spy@x.org\r\nTo: "),
        ("To: ", "Reply-To: spy@x.org\r\nTo: "),
        ("To: ", "X-Other: 1\r\nTo: "),
        ("To: ", "To: spy@x.org\r\nTo: "),
        ("Content-Type: text/plain", "Content-Type: text/html"),
        ("To: ", "References: <z@x.org>\r\nTo: "),
        ("To: ", "In-Reply-To: \r\nTo: "),
    ] {
        let changed = raw.replacen(from, to, 1);
        assert_ne!(changed, raw);
        assert!(read(changed.as_bytes(), ME).is_none(), "{changed}");
    }
    let multi = "From: owner@example.com\r\nTo: p@x.org\r\nSubject: Hi\r\nMessage-ID: <i@x>\r\n\
        MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\n\
        Content-Type: text/plain\r\n\r\nBody\r\n--b\r\nContent-Type: application/pdf\r\n\
        Content-Disposition: attachment; filename=\"a.pdf\"\r\n\r\nxx\r\n--b--\r\n";
    assert!(read(multi.as_bytes(), ME).is_none());
}

#[test]
fn the_hash_changes_with_each_field() {
    let c = content(&["p@x.org"], "Hi", "Body").check().unwrap();
    let base = hash(&c, None);
    let mut other = c.clone();
    other.cc = vec!["q@x.org".into()];
    assert_ne!(hash(&other, None), base);
    let mut other = c.clone();
    other.body.push('.');
    assert_ne!(hash(&other, None), base);
    let r = references("p@x.org", &[], None);
    assert_ne!(hash(&c, Some(&r)), base);
    assert_eq!(base.len(), 64);
}

#[test]
fn a_message_id_has_the_account_domain() {
    assert_eq!(
        message_id([0xab; 16], "me@mail.example.com"),
        format!("iris.{}@mail.example.com", "ab".repeat(16))
    );
}

#[test]
fn a_sent_copy_reads_back_with_transport_headers() {
    let c = content(&["p@x.org"], "Hi", "Body").check().unwrap();
    let raw =
        String::from_utf8(compose(ME, "iris.4@example.com", &c, None, 1_800_000_000)).unwrap();
    let sent = format!("Received: by 10.0.0.1; Mon, 1 Jan 2027\r\nX-Gm-Spam: 0\r\n{raw}");
    assert!(read(sent.as_bytes(), ME).is_none());
    let back = read_sent(sent.as_bytes(), ME).unwrap();
    assert_eq!(hash(&back.content, None), hash(&c, None));
    // A second To, another part or another From still gives no content.
    for (from, to) in [
        ("To: ", "To: spy@x.org\r\nTo: "),
        ("From: <owner@example.com>", "From: <other@example.com>"),
        ("Content-Type: text/plain", "Content-Type: text/html"),
        ("To: ", "Bcc: spy@x.org\r\nTo: "),
        ("To: ", "Reply-To: spy@x.org\r\nTo: "),
        ("To: ", "Resent-To: spy@x.org\r\nTo: "),
        ("To: ", "References: <z@x.org>\r\nTo: "),
    ] {
        let changed = sent.replacen(from, to, 1);
        assert_ne!(changed, sent);
        assert!(read_sent(changed.as_bytes(), ME).is_none(), "{changed}");
    }
}

/// A read back of any changed message does not panic. If it gives a
/// content, the message has no Bcc and no header that Iris does not
/// write, and its To and Cc addresses are the addresses of the content.
#[test]
fn a_changed_message_reads_back_only_as_what_it_sends() {
    const EDITS: &[&str] = &[
        "Bcc: spy@x.org\r\n",
        "X-Other: 1\r\n",
        "To: spy@x.org\r\n",
        "Cc: spy@x.org\r\n",
        "Reply-To: spy@x.org\r\n",
        "References: <z.9@x.org>\r\n",
        "In-Reply-To: <z.9@x.org>\r\n",
        "In-Reply-To: \r\n",
        "Content-Type: text/html\r\n",
        "\r\n",
        " folded\r\n",
        ",spy@x.org",
        "=?utf-8?q?x?=",
        "--b\r\n",
        "\u{0}",
    ];
    let mut g = Gen(0x2545_f491_4f6c_dd1d);
    let c = Content {
        to: vec!["p@x.org".into()],
        cc: vec!["q@x.org".into()],
        subject: "Hi".into(),
        body: "Body\nline 2\n".into(),
    }
    .check()
    .unwrap();
    let reply = references("<p.1@x.org>", &["r.0@x.org".into()], None);
    let bases = [
        compose(ME, "iris.7@example.com", &c, Some(&reply), 1_800_000_000),
        compose(ME, "iris.8@example.com", &c, None, 1_800_000_000),
    ];
    let mut some = 0;
    for i in 0..6000 {
        let mut raw = bases[i % 2].clone();
        for _ in 0..1 + g.below(3) {
            let at = g.below(raw.len() + 1);
            match g.below(3) {
                0 => {
                    let e = EDITS[g.below(EDITS.len())].as_bytes();
                    raw.splice(at..at, e.iter().copied());
                }
                1 => {
                    let end = (at + 1 + g.below(8)).min(raw.len());
                    raw.drain(at..end);
                }
                _ => {
                    if at < raw.len() {
                        raw[at] = b" \r\n:;,<>@=-"[g.below(11)];
                    }
                }
            }
        }
        for strict in [true, false] {
            let Some(back) = read_with(&raw, ME, strict) else {
                continue;
            };
            some += 1;
            let m = MessageParser::default().parse(&raw[..]).unwrap();
            assert!(m.bcc().is_none(), "{}", String::from_utf8_lossy(&raw));
            if strict {
                for h in m.headers() {
                    let name = h.raw_name().to_ascii_lowercase();
                    assert!(HEADERS.contains(&name.as_str()), "{name}");
                }
            }
            let seen = |l: Option<mail_parser::AddressList<'_>>| -> Vec<String> {
                l.map(|l| {
                    l.mailboxes()
                        .filter_map(|b| b.address().map(str::to_ascii_lowercase))
                        .collect()
                })
                .unwrap_or_default()
            };
            assert_eq!(seen(m.to()), back.content.to);
            assert_eq!(seen(m.cc()), back.content.cc);
            // The thread headers are all in the content, so in the hash.
            let ids = |l: Option<mail_parser::TextList<'_>>| -> Vec<String> {
                l.map(|l| {
                    (0..l.len())
                        .filter_map(|i| l.get(i))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
            };
            let header = |n: &str| {
                m.headers()
                    .iter()
                    .any(|h| h.raw_name().eq_ignore_ascii_case(n))
            };
            match &back.reply {
                None => assert!(!header("in-reply-to") && !header("references")),
                Some(r) => {
                    assert_eq!(ids(m.in_reply_to()), std::slice::from_ref(&r.in_reply_to));
                    assert_eq!(ids(m.references()), r.references);
                }
            }
        }
    }
    assert!(some > 100, "only {some} read back");
}

/// The hash is SHA-256 of the canonical JSON of section 1: fixed vectors,
/// computed outside the crate (Python `json.dumps(sort_keys=True,
/// separators=(",", ":"))` and `hashlib.sha256`).
#[test]
fn the_hash_is_the_documented_canonical_json() {
    let c = content(&["p@x.org"], "Hi", "Body").check().unwrap();
    assert_eq!(hash(&c, None), HASH_NEW);
    let r = Reply {
        in_reply_to: "p.1@x.org".into(),
        references: vec!["r.0@x.org".into(), "p.1@x.org".into()],
    };
    assert_eq!(hash(&c, Some(&r)), HASH_REPLY);
}

const HASH_NEW: &str = "5cf35dc73cda62d2c9366a385788df198e432bbcc223a3fb7e20e27683ccfbf6";
const HASH_REPLY: &str = "5aa550b8080a1ff0861877a81017d207d2448c6b3326cd2e54236a94f4104d8c";
