use super::*;

const ME: &str = "owner@example.com";

fn content(to: &[&str], subject: &str, body: &str) -> Content {
    Content {
        to: to.iter().map(|s| s.to_string()).collect(),
        cc: Vec::new(),
        subject: subject.into(),
        body: body.into(),
        ..Default::default()
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
            bcc: (0..g.below(3)).map(|_| g.address()).collect(),
            subject: g.text(12),
            body: g.text(60),
            ..Default::default()
        };
        let files: Vec<Vec<u8>> = (0..g.below(3))
            .map(|_| g.text(20).into_bytes())
            .filter(|f| !f.is_empty())
            .collect();
        for f in &files {
            let name = g.text(8);
            let mime = [
                "application/pdf",
                "text/plain",
                "TEXT/CSV",
                "image/png",
                "text/html",
                "multipart/mixed",
                "x",
            ][g.below(7)];
            raw_content.attachments.push(Attachment::of(&name, mime, f));
        }
        // Two in three cases keep only what the checks can accept, so that
        // most cases reach compose; the others test the refusals.
        if i % 3 != 0 {
            raw_content.subject.retain(|c| !c.is_control());
            raw_content.subject.push('s');
            raw_content.body.retain(|c| c != '\0');
            raw_content.body.push('b');
            for a in &mut raw_content.attachments {
                a.name.retain(|c| !c.is_control() && c != '/' && c != '\\');
                a.name.push('n');
            }
            for list in [
                &mut raw_content.to,
                &mut raw_content.cc,
                &mut raw_content.bcc,
            ] {
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
        let reply =
            (i % 2 == 0).then(|| references("<p.1@x.org>", Some("<r.0@x.org>"), None).unwrap());
        let files: Vec<&[u8]> = c
            .attachments
            .iter()
            .map(|a| files.iter().find(|f| a.is_file(f)).unwrap().as_slice())
            .collect();
        let name = (i % 4 == 1).then_some("Ówner \"O\" Name");
        let raw = compose_with(
            ME,
            name,
            "iris.1@example.com",
            &c,
            reply.as_ref(),
            1_800_000_000,
            &files,
        );
        assert_eq!(
            raw,
            compose_with(
                ME,
                name,
                "iris.1@example.com",
                &c,
                reply.as_ref(),
                1_800_000_000,
                &files
            ),
            "the same input gives the same bytes"
        );
        let back = read(&raw, ME)
            .unwrap_or_else(|| panic!("no read back of {c:?}:\n{}", String::from_utf8_lossy(&raw)));
        assert_eq!(back.content, c, "{}", String::from_utf8_lossy(&raw));
        assert_eq!(back.files, files);
        if reply.is_none() {
            assert!(
                raw.len() as u64 <= c.size_bound(),
                "{} > {}",
                raw.len(),
                c.size_bound()
            );
        }
        assert_eq!(back.bcc_header, !c.bcc.is_empty());
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
        content(&["a@example.com"], "Hi", "a\0b"),
        content(&["a@example.com"], "Hi", ""),
        content(&["a@example.com"], "Hi", " \n\t\r\n "),
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
        Some("<a@x.org>\r\n <b@x.org>"),
        Some("<b@x.org>"),
    )
    .unwrap();
    assert_eq!(r.in_reply_to, "p@x.org");
    assert_eq!(r.references, ["a@x.org", "b@x.org", "p@x.org"]);
    let r = references("p@x.org", None, Some("<q@x.org>")).unwrap();
    assert_eq!(r.references, ["q@x.org", "p@x.org"]);
    let r = references("p@x.org", Some(""), None).unwrap();
    assert_eq!(r.references, ["p@x.org"]);
    assert_eq!(references("", None, None), None);
    assert_eq!(references("<a@x> <b@x>", None, None), None);

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
    // A Bcc that the person adds is in the content, so in the hash.
    let spy = read(
        raw.replacen("To: ", "Bcc: spy@x.org\r\nTo: ", 1).as_bytes(),
        ME,
    )
    .unwrap();
    assert_eq!(spy.content.bcc, ["spy@x.org"]);
    assert_ne!(hash(&spy.content, None), hash(&c, None));
    for (from, to) in [
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
    // A file that the person adds is in the content, so in the hash.
    let added = read(multi.as_bytes(), ME).unwrap();
    assert_eq!(
        added.content.attachments,
        [Attachment::of("a.pdf", "application/pdf", b"xx")]
    );
    assert_eq!(added.files, [b"xx".to_vec()]);
    // Each other form gives no content: HTML, a nested part, a file with
    // no name, a part that is not a file, a first part that is not text.
    for (from, to) in [
        ("Content-Disposition: attachment; filename=\"a.pdf\"", "Content-Disposition: inline; filename=\"a.pdf\""),
        ("Content-Disposition: attachment; filename=\"a.pdf\"", "Content-Disposition: attachment"),
        ("Content-Disposition: attachment; filename=\"a.pdf\"\r\n", ""),
        ("Content-Type: text/plain\r\n\r\nBody", "Content-Type: text/html\r\n\r\nBody"),
        ("Content-Type: text/plain\r\n\r\nBody", "Content-Type: text/plain\r\nContent-Disposition: attachment; filename=b\r\n\r\nBody"),
        (
            "Content-Type: application/pdf\r\n",
            "Content-Type: multipart/mixed; boundary=\"c\"\r\n\r\n--c\r\nContent-Type: text/plain\r\n\r\nx\r\n--c--\r\n",
        ),
        ("Content-Type: application/pdf", "Content-Type: message/rfc822"),
        ("multipart/mixed", "multipart/alternative"),
    ] {
        let changed = multi.replacen(from, to, 1);
        assert_ne!(changed, multi);
        assert!(read(changed.as_bytes(), ME).is_none(), "{changed}");
    }
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
    let r = references("p@x.org", None, None).unwrap();
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
    let back = read_sent(sent.as_bytes(), ME, &[]).unwrap();
    assert_eq!(hash(&back.content, None), hash(&c, None));
    // A second To, another part or another From still gives no content.
    for (from, to) in [
        ("To: ", "To: spy@x.org\r\nTo: "),
        ("From: <owner@example.com>", "From: <other@example.com>"),
        ("Content-Type: text/plain", "Content-Type: text/html"),
        ("To: ", "Reply-To: spy@x.org\r\nTo: "),
        ("To: ", "Resent-To: spy@x.org\r\nTo: "),
        ("To: ", "References: <z@x.org>\r\nTo: "),
    ] {
        let changed = sent.replacen(from, to, 1);
        assert_ne!(changed, sent);
        assert!(
            read_sent(changed.as_bytes(), ME, &[]).is_none(),
            "{changed}"
        );
    }
    // A Bcc of the sent copy is in its content; with no Bcc header the
    // caller takes the approved bcc (send content spec, section 4).
    let bcc = read_sent(
        sent.replacen("To: ", "Bcc: spy@x.org\r\nTo: ", 1)
            .as_bytes(),
        ME,
        &[],
    )
    .unwrap();
    assert!(bcc.bcc_header);
    assert_eq!(bcc.content.bcc, ["spy@x.org"]);
    assert!(!back.bcc_header);
    // The approved bcc: a copy with no Bcc header takes it, and a copy
    // whose Bcc header names other addresses keeps its own.
    let approved = vec!["sam@x.org".to_string()];
    let with = Content {
        bcc: approved.clone(),
        ..c.clone()
    }
    .check()
    .unwrap();
    let omitted = read_sent(sent.as_bytes(), ME, &approved).unwrap();
    assert!(!omitted.bcc_header);
    assert_eq!(hash(&omitted.content, None), hash(&with, None));
    let other = read_sent(
        sent.replacen("To: ", "Bcc: spy@x.org\r\nTo: ", 1)
            .as_bytes(),
        ME,
        &approved,
    )
    .unwrap();
    assert_eq!(other.content.bcc, ["spy@x.org"]);
    assert_ne!(hash(&other.content, None), hash(&with, None));
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
        ..Default::default()
    }
    .check()
    .unwrap();
    let mut with_file = c.clone();
    with_file.bcc = vec!["b@x.org".into()];
    with_file.attachments = vec![Attachment::of("a.pdf", "application/pdf", b"%PDF-1")];
    let reply = references("<p.1@x.org>", Some("<r.0@x.org>"), None).unwrap();
    let bases = [
        compose(ME, "iris.7@example.com", &c, Some(&reply), 1_800_000_000),
        compose(ME, "iris.8@example.com", &c, None, 1_800_000_000),
        compose_with(
            ME,
            None,
            "iris.9@example.com",
            &with_file,
            None,
            1_800_000_000,
            &[b"%PDF-1"],
        ),
    ];
    let mut some = 0;
    for i in 0..6000 {
        let mut raw = bases[i % 3].clone();
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
            assert_eq!(back.content.clone().check(), Ok(back.content.clone()));
            assert_eq!(seen(m.to()), back.content.to);
            assert_eq!(seen(m.cc()), back.content.cc);
            assert_eq!(seen(m.bcc()), back.content.bcc);
            // Each file of the message is an entry of the content: the hash
            // covers it.
            assert_eq!(back.files.len(), back.content.attachments.len());
            for (a, f) in back.content.attachments.iter().zip(&back.files) {
                assert!(a.is_file(f));
            }
            assert!(m
                .parts()
                .all(|p| !p.is_content_type("text", "html") || p.is_attachment()));
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
    // An empty bcc and an empty attachments list are not in the JSON.
    let mut full = c.clone();
    full.bcc = vec!["b@x.org".into()];
    full.attachments = vec![Attachment::of("a.pdf", "application/pdf", b"x")];
    assert_eq!(hash(&full, None), HASH_FULL);
}

const HASH_NEW: &str = "5cf35dc73cda62d2c9366a385788df198e432bbcc223a3fb7e20e27683ccfbf6";
const HASH_FULL: &str = "3d5d29238a90167e4422140d52fdcff57e0ede467962f8c28e48715c2b1da567";
const HASH_REPLY: &str = "5aa550b8080a1ff0861877a81017d207d2448c6b3326cd2e54236a94f4104d8c";

/// Each parent header value gives reply headers that write and read back
/// the same, so the hash of a reply at Gmail is the approved hash.
#[test]
fn reply_headers_of_any_parent_read_back_the_same() {
    const POOL: &[&str] = &[
        "<a@x.org>",
        "b@x.org",
        " ",
        "\r\n ",
        "\t",
        "<",
        ">",
        "<>",
        "<a b@x>",
        "(c)",
        "<é@x>",
        ",",
        "<p@x.org>",
        "<z.1@y>",
        "\"q\"",
        "<a@x.org>",
    ];
    let mut g = Gen(0x6a09_e667_f3bc_c908);
    let c = content(&["p@x.org"], "Hi", "Body").check().unwrap();
    let mut some = 0;
    for _ in 0..3000 {
        let mut value = |n: usize| -> String {
            (0..g.below(n + 1))
                .map(|_| POOL[g.below(POOL.len())])
                .collect()
        };
        let (id, refs, irt) = (value(3), value(6), value(3));
        let Some(r) = references(&id, Some(&refs), Some(&irt)) else {
            continue;
        };
        some += 1;
        let raw = compose(ME, "iris.5@example.com", &c, Some(&r), 1_800_000_000);
        let back =
            read(&raw, ME).unwrap_or_else(|| panic!("{r:?}:\n{}", String::from_utf8_lossy(&raw)));
        assert_eq!(
            back.reply.as_ref(),
            Some(&r),
            "{}",
            String::from_utf8_lossy(&raw)
        );
    }
    assert!(some > 300, "only {some}");
}

/// A draft that the person changed at Gmail past the limits of a new
/// draft gives no content.
#[test]
fn a_changed_draft_keeps_the_limits() {
    let c = content(&["p@x.org"], "Hi", "Body").check().unwrap();
    let raw =
        String::from_utf8(compose(ME, "iris.6@example.com", &c, None, 1_800_000_000)).unwrap();
    let many: Vec<String> = (0..MAX_RECIPIENTS).map(|i| format!("a{i}@x.org")).collect();
    let crowded = raw.replacen(
        "To: <p@x.org>",
        &format!("To: <p@x.org>, {}", many.join(", ")),
        1,
    );
    assert_ne!(crowded, raw);
    assert!(read(crowded.as_bytes(), ME).is_none());
    assert!(read_sent(crowded.as_bytes(), ME, &[]).is_none());
    let large = format!("{raw}{}\r\n", "x".repeat(MAX_BODY));
    assert!(read(large.as_bytes(), ME).is_none());
    // A duplicate or upper case address that the check would erase.
    for (from, to) in [
        ("To: <p@x.org>", "To: <p@x.org>\r\nCc: <p@x.org>"),
        ("To: <p@x.org>", "To: <p@x.org>, <p@x.org>"),
        ("To: <p@x.org>", "To: <P@x.org>"),
        ("To: <p@x.org>", "To: CEO <p@x.org>"),
        ("To: <p@x.org>", "To: (CEO) <p@x.org>"),
        ("To: <p@x.org>", "To: team: <p@x.org>;"),
    ] {
        let changed = raw.replacen(from, to, 1).replacen("Body", "Other", 1);
        assert_ne!(changed, raw);
        assert!(read(changed.as_bytes(), ME).is_none(), "{changed}");
        assert!(
            read_sent(changed.as_bytes(), ME, &[]).is_none(),
            "{changed}"
        );
    }
    assert!(read(raw.as_bytes(), ME).is_some());
}

/// A change of the white space in the subject at Gmail gives no content,
/// although the checks would collapse it back to the approved subject.
#[test]
fn a_subject_with_changed_spaces_does_not_read_back() {
    let c = content(&["p@x.org"], "Hi there", "Body").check().unwrap();
    let raw =
        String::from_utf8(compose(ME, "iris.8@example.com", &c, None, 1_800_000_000)).unwrap();
    assert!(read(raw.as_bytes(), ME).is_some());
    let changed = raw.replacen("Subject: Hi there", "Subject: Hi  there", 1);
    assert_ne!(changed, raw);
    assert!(read(changed.as_bytes(), ME).is_none());
    assert!(read_sent(changed.as_bytes(), ME, &[]).is_none());
}

/// Spec section 1: `From` is checked by address only, because Gmail can
/// add the display name of the account. A display name reads back with
/// the same hash on both read paths; another address still gives none.
#[test]
fn a_display_name_on_from_reads_back_on_both_paths() {
    let c = content(&["p@x.org"], "Hi", "Body").check().unwrap();
    let raw =
        String::from_utf8(compose(ME, "iris.9@example.com", &c, None, 1_800_000_000)).unwrap();
    let named = raw.replacen(
        "From: <owner@example.com>",
        "From: \"Owner\" <owner@example.com>",
        1,
    );
    assert_ne!(named, raw);
    let sent = format!("Received: by 10.0.0.1; Mon, 1 Jan 2027\r\n{named}");
    for back in [
        read(named.as_bytes(), ME),
        read_sent(sent.as_bytes(), ME, &[]),
    ] {
        assert_eq!(hash(&back.unwrap().content, None), hash(&c, None));
    }
    let other = named.replacen("<owner@example.com>", "<other@example.com>", 1);
    assert!(read(other.as_bytes(), ME).is_none());
}

/// The shape of a real Gmail draft after `drafts.create` (send check of
/// issue #302): Gmail sets its own `Message-ID`, sets a new `Date` and adds
/// a `Received` header. The draft reads back with the ID of Gmail.
#[test]
fn a_gmail_draft_reads_back_with_the_gmail_message_id() {
    let c = content(&[ME], "Iris send test 2", "Body").check().unwrap();
    let raw =
        String::from_utf8(compose(ME, "iris.5@example.com", &c, None, 1_800_000_000)).unwrap();
    let id_line = raw
        .lines()
        .find(|l| l.starts_with("Message-ID:"))
        .unwrap()
        .to_string();
    let date_line = raw
        .lines()
        .find(|l| l.starts_with("Date:"))
        .unwrap()
        .to_string();
    let gmail = format!(
        "Received: from 216739018898 named unknown by gmailapi.google.com with HTTPREST; \
         Tue, 17 Nov 2026 21:40:12 -0800\r\n{}",
        raw.replacen(&id_line, "Message-ID: <CACe_x7Q1fake@mail.gmail.com>", 1)
            .replacen(&date_line, "Date: Tue, 17 Nov 2026 21:40:12 -0800", 1)
    );
    let back = read(gmail.as_bytes(), ME).unwrap();
    assert_eq!(back.message_id, "CACe_x7Q1fake@mail.gmail.com");
    assert_eq!(hash(&back.content, None), hash(&c, None));
    // Two Received headers are transport too.
    let twice = format!("Received: by 10.0.0.1; Tue, 17 Nov 2026\r\n{gmail}");
    assert!(read(twice.as_bytes(), ME).is_some());
    // An addressing header or another unknown header still gives no content.
    for extra in [
        "Reply-To: spy@x.org",
        "Disposition-Notification-To: spy@x.org",
        "X-Gm-Spam: 0",
    ] {
        let changed = format!("{extra}\r\n{gmail}");
        assert!(read(changed.as_bytes(), ME).is_none(), "{changed}");
    }
}

/// Invariant (#270): a subject with `=?` reads back the same, as the
/// content that the hash binds. Also long and non-ASCII ones.
#[test]
fn a_subject_with_an_encoded_word_reads_back_the_same() {
    for subject in [
        "=?utf-8?q?x?=",
        "Re: =?ISO-8859-1?Q?caf=E9?= and more",
        "a =? b",
        &format!("=?{}日本😀", "x".repeat(MAX_SUBJECT - 10)),
    ] {
        let c = content(&["a@example.com"], subject, "Body")
            .check()
            .unwrap();
        let raw = compose(
            "me@example.com",
            "id.1@example.com",
            &c,
            None,
            1_700_000_000,
        );
        let text = String::from_utf8(raw.clone()).unwrap();
        for line in text.lines() {
            assert!(line.len() <= 998, "{line}");
        }
        let back = read(&raw, "me@example.com").expect("the message reads back");
        assert_eq!(back.content.subject, c.subject, "{text}");
    }
}

#[test]
fn checks_refuse_bad_attachments() {
    let ok = Attachment::of("a.pdf", "application/pdf", b"x");
    let with = |a: Attachment| {
        let mut c = content(&["p@x.org"], "Hi", "Body");
        c.attachments = vec![a];
        c.check()
    };
    let normal = with(Attachment {
        name: " a.pdf ".into(),
        mime: "Application/PDF".into(),
        ..ok.clone()
    })
    .unwrap();
    assert_eq!(normal.attachments, std::slice::from_ref(&ok));
    for bad in [
        Attachment {
            name: "".into(),
            ..ok.clone()
        },
        Attachment {
            name: "a\r\nb".into(),
            ..ok.clone()
        },
        Attachment {
            name: "a/b".into(),
            ..ok.clone()
        },
        Attachment {
            name: "a\\b".into(),
            ..ok.clone()
        },
        Attachment {
            name: "..".into(),
            ..ok.clone()
        },
        Attachment {
            name: "x".repeat(256),
            ..ok.clone()
        },
        Attachment {
            mime: "pdf".into(),
            ..ok.clone()
        },
        Attachment {
            mime: "multipart/mixed".into(),
            ..ok.clone()
        },
        Attachment {
            mime: "message/rfc822".into(),
            ..ok.clone()
        },
        Attachment {
            mime: "text/plain; charset=x".into(),
            ..ok.clone()
        },
        Attachment {
            sha256: "AB".repeat(32),
            ..ok.clone()
        },
        Attachment {
            sha256: "ab".into(),
            ..ok.clone()
        },
        Attachment {
            size: 0,
            ..ok.clone()
        },
        Attachment {
            size: MAX_MESSAGE,
            ..ok.clone()
        },
    ] {
        assert!(with(bad.clone()).is_err(), "{bad:?}");
    }
    let mut many = content(&["p@x.org"], "Hi", "Body");
    many.attachments = vec![ok.clone(); MAX_ATTACHMENTS + 1];
    assert!(many.check().is_err());
    let mut fifty = content(&["p@x.org"], "Hi", "Body");
    fifty.bcc = (1..MAX_RECIPIENTS).map(|i| format!("b{i}@x.org")).collect();
    assert!(fifty.clone().check().is_ok());
    fifty.cc.push("c@x.org".into());
    assert!(fifty.check().is_err());
    let mut dup = content(&["p@x.org"], "Hi", "Body");
    dup.cc = vec!["q@x.org".into()];
    dup.bcc = vec!["P@x.org".into(), "q@x.org".into(), "r@x.org".into()];
    assert_eq!(dup.check().unwrap().bcc, ["r@x.org"]);
    assert_eq!(max_message("outlook"), MAX_MESSAGE_OUTLOOK);
    assert_eq!(max_message("gmail"), MAX_MESSAGE);
}

#[test]
fn a_display_name_goes_on_from_and_reads_back_by_address() {
    assert_eq!(from_name("  Dana Diaz "), Some("Dana Diaz".into()));
    assert_eq!(from_name(""), None);
    assert_eq!(from_name("a\r\nBcc: x@y.z"), None);
    // A control character at any place of a name is refused: the trim of
    // the ends takes spaces only (send content spec, sections 2 and 3).
    let ok = Attachment::of("a.pdf", "application/pdf", b"x");
    for c in ['\r', '\n', '\t', '\u{0}', '\u{7f}', '\u{85}', '\u{1b}'] {
        for name in [
            format!("{c}a.pdf"),
            format!("a{c}.pdf"),
            format!("a.pdf{c}"),
            format!(" {c} a.pdf "),
        ] {
            assert_eq!(from_name(&name), None, "{name:?}");
            let mut m = content(&["p@x.org"], "Hi", "Body");
            m.attachments = vec![Attachment {
                name: name.clone(),
                ..ok.clone()
            }];
            assert!(m.check().is_err(), "{name:?}");
        }
    }
    assert_eq!(from_name(&"x".repeat(MAX_NAME + 1)), None);
    let c = content(&["p@x.org"], "Hi", "Body").check().unwrap();
    let raw = compose_with(
        ME,
        Some("Dana Diaz"),
        "iris.2@example.com",
        &c,
        None,
        1,
        &[],
    );
    let m = MessageParser::default().parse(&raw[..]).unwrap();
    let from = m.from().unwrap();
    assert_eq!(from.first().unwrap().name(), Some("Dana Diaz"));
    assert_eq!(hash(&read(&raw, ME).unwrap().content, None), hash(&c, None));
}

/// The bound of section 5 holds for the largest parts: a body of only
/// non-ASCII characters and files of random bytes of many sizes.
#[test]
fn the_size_bound_holds_for_large_content() {
    let mut g = Gen(0x1234_5678_9abc_def1);
    for body in [
        "é".repeat(MAX_BODY / 4),
        "=\u{7f}".repeat(MAX_BODY / 4),
        "a".repeat(MAX_BODY / 2),
    ] {
        let mut c = Content {
            to: (0..20).map(|i| format!("a{i}@x.org")).collect(),
            cc: (0..15).map(|i| format!("c{i}@x.org")).collect(),
            bcc: (0..15).map(|i| format!("b{i}@x.org")).collect(),
            subject: "日本".repeat(499),
            body,
            ..Default::default()
        };
        let files: Vec<Vec<u8>> = (0..MAX_ATTACHMENTS)
            .map(|i| (0..1 + i * 9973).map(|_| g.next() as u8).collect())
            .collect();
        for (i, f) in files.iter().enumerate() {
            c.attachments.push(Attachment::of(
                &format!("ü{i}日本.bin"),
                "application/octet-stream",
                f,
            ));
        }
        let c = c.check().unwrap();
        let refs: Vec<&[u8]> = files.iter().map(Vec::as_slice).collect();
        let raw = compose_with(
            ME,
            Some(&"Ñ".repeat(MAX_NAME)),
            "iris.1@example.com",
            &c,
            None,
            1,
            &refs,
        );
        assert!(
            raw.len() as u64 <= c.size_bound(),
            "{} > {}",
            raw.len(),
            c.size_bound()
        );
        assert_eq!(read(&raw, ME).unwrap().content, c);
    }
}

/// The streamed check of a file gives the answer of the check in memory,
/// for each size around the size of its buffer, and for a wrong size or a
/// wrong hash.
#[test]
fn the_streamed_file_check_agrees_with_the_check_in_memory() {
    let mut seed = 7u64;
    let mut next = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as u8
    };
    for len in [
        0,
        1,
        63,
        FILE_CHUNK - 1,
        FILE_CHUNK,
        FILE_CHUNK + 1,
        3 * FILE_CHUNK + 17,
    ] {
        let bytes: Vec<u8> = (0..len).map(|_| next()).collect();
        let entry = Attachment {
            name: "a.bin".into(),
            mime: "application/octet-stream".into(),
            size: len as u64,
            sha256: sha256(&bytes),
        };
        let other = |e: &Attachment, b: &[u8]| (e.is_file(b), e.is_file_in(b).unwrap());
        assert_eq!(other(&entry, &bytes), (true, true), "len {len}");
        let mut longer = bytes.clone();
        longer.push(1);
        assert_eq!(other(&entry, &longer), (false, false), "len {len} + 1");
        if len > 0 {
            let mut changed = bytes.clone();
            changed[len / 2] ^= 1;
            assert_eq!(
                other(&entry, &changed),
                (false, false),
                "len {len}, one bit"
            );
            assert_eq!(
                other(&entry, &bytes[..len - 1]),
                (false, false),
                "len {len} - 1"
            );
        }
    }
}
