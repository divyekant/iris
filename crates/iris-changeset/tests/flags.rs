//! The checks before a send (docs/specs/2026-11-17-send-checks.md): the
//! draft flags of spec 2, section 5.3.

use std::fs;
use std::path::PathBuf;

use iris_changeset::{
    add_action, create, draft_flags, draft_marks, has_attachment, has_date, has_money, names,
    submit, words, write_document, Author, AuthorKind, Field, Mark, Verb,
};
use iris_compose::Content;
use iris_store::{Body, Header, NewMessage, Store};
use serde_json::{json, Map, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-cf-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const OWNER: &str = "owner@example.com";
const NOW: i64 = 1_763_283_422_000;

/// The text of a message from another person, in the full-text index.
const THEIRS: &str = "Please wire the deposit to the escrow account before the inspector \
     arrives on site and confirm the reference number with our office team";

/// The text of a received message whose From is the account address.
const FORGED: &str = "Reset your mailbox password at the link below within one hour or the \
     account closes for good and all of the mail is lost";

/// Account 1. Message 1 from priya, with clean text. The owner wrote to
/// priya@example.com before, and sent message 2 with the owner's own text.
fn store(dir: &TempDir) -> Store {
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", OWNER).unwrap();
    s.put_labels(a, &[("SENT", "SENT", "system")]).unwrap();
    let m = |id: &str, from: &str, text: &str, labels: &[&str]| NewMessage {
        provider_id: id.into(),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        thread_provider_id: "gt1".into(),
        internal_date: Some(1),
        header: Header {
            from_addr: Some(from.into()),
            subject: Some("Deposit".into()),
            message_id_hdr: Some(format!("<{id}@x.org>")),
            ..Default::default()
        },
        body: Some(Body {
            subject: "Deposit".into(),
            sender: from.into(),
            text: text.into(),
        }),
        ..Default::default()
    };
    s.put_messages(
        a,
        &[
            m("g1", "priya@example.com", THEIRS, &[]),
            m(
                "g2",
                OWNER,
                "My own words that I wrote to many people in the past many times over",
                &["SENT"],
            ),
            // A received message with a forged From of the account: external.
            m("g3", OWNER, FORGED, &[]),
        ],
    )
    .unwrap();
    s.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO sender_evidence(message_id, address, rule) VALUES (2, 'priya@example.com', 'sent')",
            [],
        )?)
    })
    .unwrap();
    s
}

fn content(to: &str, subject: &str, body: &str) -> Content {
    Content {
        to: vec![to.into()],
        cc: Vec::new(),
        subject: subject.into(),
        body: body.into(),
    }
    .check()
    .unwrap()
}

/// Acceptance 1 and 2: each case gives its flag, and only its flag.
#[test]
fn each_case_of_the_test_set_gives_its_flag() {
    let dir = TempDir::new("set");
    let s = store(&dir);
    let known = "priya@example.com";
    let copied = THEIRS.split(' ').take(15).collect::<Vec<_>>().join(" ");
    let cases: Vec<(&str, &str, String, Vec<&str>)> = vec![
        (
            "missing attachment",
            known,
            "See the attached plan.".into(),
            vec!["attachment"],
        ),
        (
            "enclosed",
            known,
            "The form is enclosed.".into(),
            vec!["attachment"],
        ),
        (
            "file kind",
            known,
            "I included the report PDF.".into(),
            vec!["attachment"],
        ),
        (
            "wrong recipient",
            "priya@exmaple.com",
            "Hi.".into(),
            vec!["new_recipient"],
        ),
        (
            "new recipient",
            "sam@example.org",
            "Hi.".into(),
            vec!["new_recipient"],
        ),
        (
            "money sign",
            known,
            "I can pay $1,200 now.".into(),
            vec!["money"],
        ),
        (
            "money code",
            known,
            "The fee is 300 EUR.".into(),
            vec!["money"],
        ),
        ("money joined", known, "Send 50usd.".into(), vec!["money"]),
        (
            "date weekday",
            known,
            "I will send it by Friday.".into(),
            vec!["date"],
        ),
        ("date month", known, "Due on 3 March.".into(), vec!["date"]),
        ("date may", known, "Due on May 3.".into(), vec!["date"]),
        (
            "date iso",
            known,
            "Done on 2026-11-20.".into(),
            vec!["date"],
        ),
        ("date numeric", known, "Done on 20/11.".into(), vec!["date"]),
        (
            "date near",
            known,
            "I will call tomorrow.".into(),
            vec!["date"],
        ),
        (
            "date next",
            known,
            "Next week is fine.".into(),
            vec!["date"],
        ),
        (
            "date end of",
            known,
            "By the end of the day.".into(),
            vec!["date"],
        ),
        (
            "copied text",
            known,
            format!("Note: {copied}. Thanks."),
            vec!["untrusted_content"],
        ),
        (
            "forged sender",
            known,
            FORGED.split(' ').take(16).collect::<Vec<_>>().join(" "),
            vec!["untrusted_content"],
        ),
        ("plain", known, "Thanks, that works for me.".into(), vec![]),
        (
            "own text",
            known,
            "My own words that I wrote to many people in the past".into(),
            vec![],
        ),
        (
            "verb march",
            known,
            "We march on with the plan.".into(),
            vec![],
        ),
        ("may verb", known, "You may call me.".into(), vec![]),
        ("digit alone", known, "Room 12 has 3 chairs.".into(), vec![]),
        ("reattach", known, "Please reattach nothing.".into(), vec![]),
        (
            "short copy",
            known,
            "the escrow account before".into(),
            vec![],
        ),
    ];
    for (name, to, body, want) in cases {
        let got = s
            .read(|c| Ok(draft_flags(c, OWNER, &content(to, "Plan", &body)).unwrap()))
            .unwrap();
        assert_eq!(got, want, "{name}: {body}");
    }
    // The subject counts for the text rules too.
    let got = s
        .read(|c| Ok(draft_flags(c, OWNER, &content(known, "Pay $5 by Monday", "Hi.")).unwrap()))
        .unwrap();
    assert_eq!(got, vec!["money", "date"]);
    // A copied run in the subject counts too.
    let got = s
        .read(|c| Ok(draft_flags(c, OWNER, &content(known, &copied, "Hi.")).unwrap()))
        .unwrap();
    assert_eq!(got, vec!["untrusted_content"]);
}

/// Evidence counts only while its message is a sent message that is not
/// gone (issue #377): stale evidence makes the address new.
#[test]
fn stale_sent_evidence_makes_the_address_new() {
    let flags = |s: &Store| {
        s.read(|c| Ok(draft_flags(c, OWNER, &content("priya@example.com", "Hi", "Hi.")).unwrap()))
            .unwrap()
    };
    let dir = TempDir::new("gone");
    let s = store(&dir);
    assert!(flags(&s).is_empty());
    s.write(|tx| Ok(tx.execute("UPDATE message SET gone_at = 1 WHERE id = 2", [])?))
        .unwrap();
    assert_eq!(flags(&s), vec!["new_recipient"]);
    let dir = TempDir::new("unsent");
    let s = store(&dir);
    s.write(|tx| Ok(tx.execute("DELETE FROM message_label WHERE message_id = 2", [])?))
        .unwrap();
    assert_eq!(flags(&s), vec!["new_recipient"]);
    let dir = TempDir::new("changed");
    let s = store(&dir);
    s.write(|tx| {
        Ok(tx.execute(
            "UPDATE sender_evidence SET content_sha256 = 'other' WHERE message_id = 2",
            [],
        )?)
    })
    .unwrap();
    assert_eq!(flags(&s), vec!["new_recipient"]);
}

/// A store with no `sent` evidence: each address is new.
#[test]
fn no_sent_mail_makes_each_address_new() {
    let dir = TempDir::new("none");
    let s = store(&dir);
    s.write(|tx| Ok(tx.execute("DELETE FROM sender_evidence", [])?))
        .unwrap();
    let got = s
        .read(|c| Ok(draft_flags(c, OWNER, &content("priya@example.com", "Hi", "Hi.")).unwrap()))
        .unwrap();
    assert_eq!(got, vec!["new_recipient"]);
    // The account itself is never new.
    let got = s
        .read(|c| Ok(draft_flags(c, OWNER, &content(OWNER, "Hi", "Hi.")).unwrap()))
        .unwrap();
    assert!(got.is_empty());
}

/// Submit puts the draft flags on a `draft` action and on the `send` of
/// its draft, and the document agrees with the schema.
#[test]
fn submit_flags_a_draft_and_its_send() {
    let dir = TempDir::new("submit");
    let s = store(&dir);
    let who = Author {
        kind: AuthorKind::Agent,
        name: Some("t".into()),
        session: None,
        job: None,
    };
    let mail = json!({"to": ["sam@example.org"], "subject": "Pay", "body": "I owe you $40, see attached."});
    let one = |verb: Verb, target: &str, args: &Value| -> Value {
        let cs = create(&s, &who, NOW).unwrap();
        let args: Map<String, Value> = args.as_object().unwrap().clone();
        add_action(&s, cs, &who, verb, &[target], &args, None).unwrap();
        submit(&s, cs, &who, "Write.", NOW).unwrap();
        let mut out = Vec::new();
        write_document(&s, cs, &mut out).unwrap();
        let d: Value = serde_json::from_slice(&out).unwrap();
        let errors: Vec<String> = iris_spec::validator("changeset")
            .iter_errors(&d)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
        d["actions"][0].clone()
    };
    let a = one(Verb::Draft, "a-1", &mail);
    assert_eq!(
        a["flags"],
        json!(["new_recipient", "money", "attachment"]),
        "{a}"
    );
    // The draft that the apply makes: its raw bytes and their hash. The
    // JSON content of the row is not read (another writer can change it).
    let c = content("sam@example.org", "Pay", "I owe you $40, see attached.");
    let raw = iris_compose::compose(OWNER, "iris.1@example.com", &c, None, 1);
    let hash = iris_compose::hash(&c, None);
    s.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                               action_id, created_at, updated_at)
             VALUES (1, 'r1', 'iris.1@example.com', '{}', ?1, ?2, 1, 0, 0)",
            rusqlite::params![hash, raw],
        )?)
    })
    .unwrap();
    let a = one(Verb::Send, "d-1", &json!({}));
    assert_eq!(
        a["flags"],
        json!(["new_recipient", "money", "attachment"]),
        "{a}"
    );
    // Raw bytes that do not hash to the row hash: no checked content, so
    // submit denies the send.
    s.write(|tx| {
        tx.execute(
            "UPDATE changeset_action SET state = 'denied', decided_by = 'engine',
               decided_at = 0, reason = 'test' WHERE verb = 'send'",
            [],
        )?;
        Ok(tx.execute("UPDATE draft SET hash = 'other' WHERE id = 1", [])?)
    })
    .unwrap();
    let a = one(Verb::Send, "d-1", &json!({}));
    assert_eq!(a["decision"]["state"], "denied", "{a}");
    let why = a["decision"]["reason"].as_str().unwrap();
    assert!(why.contains("no content that Iris can check"), "{why}");
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

    fn text(&mut self, max: usize) -> String {
        const PARTS: [&str; 24] = [
            " ",
            "$",
            "€",
            "12",
            "2026-11-17",
            "17/11",
            "/",
            "-",
            "\"",
            "*",
            "NEAR",
            "AND",
            "^",
            "(",
            "friday",
            "march",
            "usd",
            "attach",
            "é",
            "日本",
            "\n",
            "x",
            "0",
            ":",
        ];
        let n = (self.next() % max as u64) as usize;
        (0..n)
            .map(|_| match self.next() % 4 {
                0 => char::from_u32((self.next() % 0x3000) as u32)
                    .map_or(String::new(), String::from),
                _ => PARTS[(self.next() % PARTS.len() as u64) as usize].to_string(),
            })
            .collect()
    }
}

/// Random text, with FTS5 syntax and Unicode: no panic, no query error,
/// the same flags for the same input, and an amount, a date or an
/// attachment word added always gives its flag.
#[test]
fn draft_flags_hold_for_random_text() {
    let dir = TempDir::new("prop");
    let s = store(&dir);
    let mut g = Gen(0x9e37_79b9_7f4a_7c15);
    for _ in 0..400 {
        let body = g.text(200);
        let c = Content {
            to: vec!["priya@example.com".into()],
            cc: Vec::new(),
            subject: "Hi".into(),
            body: body.clone(),
        };
        let Ok(c) = c.check() else { continue };
        let first = s.read(|x| Ok(draft_flags(x, OWNER, &c).unwrap())).unwrap();
        let again = s.read(|x| Ok(draft_flags(x, OWNER, &c).unwrap())).unwrap();
        assert_eq!(first, again, "{body:?}");
        assert!(has_money(&format!("{body} $5")), "{body:?}");
        assert!(has_date(&format!("{body} friday")), "{body:?}");
        assert!(has_attachment(&format!("{body} attached")), "{body:?}");
    }
    // The largest body: one query for each 8 words, and no error.
    let big = "word ".repeat(iris_compose::MAX_BODY / 5 - 1);
    let c = content("priya@example.com", "Hi", &big);
    let got = s.read(|x| Ok(draft_flags(x, OWNER, &c).unwrap())).unwrap();
    assert!(got.is_empty());
}

/// The text of each mark of a content.
fn marked<'a>(c: &'a Content, m: &Mark) -> &'a str {
    match m.field {
        Field::To(i) => &c.to[i][m.range.clone()],
        Field::Cc(i) => &c.cc[i][m.range.clone()],
        Field::Subject => &c.subject[m.range.clone()],
        Field::Body => &c.body[m.range.clone()],
    }
}

/// Card R4-07: each flag of the test set is on the words that gave it.
#[test]
fn each_mark_of_the_test_set_is_on_its_words() {
    let dir = TempDir::new("marks");
    let s = store(&dir);
    let mut c = content(
        "priya@example.com",
        "Renewal by Friday",
        &format!(
            "The renewal is $48,000 for 12 months, or 5 dollars a day. Sign by Dec 4. \
             The quote is attached.\n\n{THEIRS}."
        ),
    );
    c.cc = vec!["legal@new.example".into()];
    let marks = s.read(|x| Ok(draft_marks(x, OWNER, &c).unwrap())).unwrap();
    let got: Vec<(&str, Field, &str)> = marks
        .iter()
        .filter(|m| m.flag != "untrusted_content")
        .map(|m| (m.flag, m.field, marked(&c, m)))
        .collect();
    assert_eq!(
        got,
        vec![
            ("new_recipient", Field::Cc(0), "legal@new.example"),
            ("money", Field::Body, "$48,000"),
            ("money", Field::Body, "5 dollars"),
            ("date", Field::Subject, "Friday"),
            ("date", Field::Body, "Dec 4"),
            ("attachment", Field::Body, "attached"),
        ]
    );
    let copied: Vec<&Mark> = marks
        .iter()
        .filter(|m| m.flag == "untrusted_content")
        .collect();
    assert_eq!(copied.len(), 2, "{marks:?}");
    for m in copied {
        assert_eq!((m.field, m.source), (Field::Body, Some(1)));
        assert!(THEIRS.contains(marked(&c, m)), "{:?}", marked(&c, m));
    }
    assert_eq!(
        names(&marks),
        s.read(|x| Ok(draft_flags(x, OWNER, &c).unwrap())).unwrap()
    );
}

/// The amounts and dates that a mark covers, from the neighbors of the sign
/// or the word.
#[test]
fn a_mark_covers_the_whole_amount_and_date() {
    let dir = TempDir::new("whole");
    let s = store(&dir);
    let cases: [(&str, &str); 13] = [
        ("pay 100 $ now", "100 $"),
        ("pay $ 1,250.50.", "$ 1,250.50"),
        ("pay €12", "€12"),
        ("pay usd200", "usd200"),
        ("pay 30 USD", "30 USD"),
        ("pay USD 1,200.", "USD 1,200"),
        ("pay 1,200.50 USD", "1,200.50 USD"),
        ("pay 2,500usd", "2,500usd"),
        ("pay $1,200 USD now", "$1,200 USD"),
        ("by 2026-11-17", "2026-11-17"),
        ("by 11/17/2026", "11/17/2026"),
        ("by end of the day", "end of the day"),
        ("by next week", "next week"),
    ];
    for (text, want) in cases {
        let c = Content {
            to: vec!["priya@example.com".into()],
            cc: Vec::new(),
            subject: "Hi".into(),
            body: text.into(),
        };
        let marks = s.read(|x| Ok(draft_marks(x, OWNER, &c).unwrap())).unwrap();
        let got: Vec<&str> = marks.iter().map(|m| &text[m.range.clone()]).collect();
        assert_eq!(got, [want], "{text:?}");
    }
}

/// True if the mark is not a piece of a longer number of its field.
fn whole(body: &str, subject: &str, m: &Mark) -> bool {
    let text = if m.field == Field::Body {
        body
    } else {
        subject
    };
    let num = |c: Option<char>| c.is_some_and(|c| c.is_ascii_digit());
    let mut before = text[..m.range.start].chars().rev();
    let mut after = text[m.range.end..].chars();
    let (b1, b2) = (before.next(), before.next());
    let (a1, a2) = (after.next(), after.next());
    let sep = |c: Option<char>| matches!(c, Some(',' | '.'));
    let first = text[m.range.clone()].chars().next();
    let last = text[m.range.clone()].chars().next_back();
    !(num(b1) && num(first))
        && !(num(a1) && num(last))
        && !(sep(b1) && num(b2) && num(first))
        && !(sep(a1) && num(a2) && num(last))
}

/// Random text: the flags of the marks are the flags of draft_flags, and
/// each mark is a part of its field that gives its flag by itself.
#[test]
fn marks_agree_with_the_flags_for_random_text() {
    let dir = TempDir::new("marks-prop");
    let s = store(&dir);
    let mut g = Gen(0x2545_f491_4f6c_dd1d);
    const MORE: [&str; 9] = [
        " $48,000.",
        "5 dollars",
        "100 $",
        "usd200",
        "Dec 4",
        "next week",
        "end of the day",
        " attached",
        THEIRS,
    ];
    for _ in 0..400 {
        let mut body = g.text(120);
        for _ in 0..g.next() % 3 {
            body.push_str(MORE[(g.next() % MORE.len() as u64) as usize]);
            body.push_str(&g.text(20));
        }
        let subject = format!("Hi {}", g.text(20));
        let to = if g.next().is_multiple_of(2) {
            "priya@example.com"
        } else {
            "new@x.example"
        };
        let Ok(c) = (Content {
            to: vec![to.into()],
            cc: Vec::new(),
            subject,
            body,
        })
        .check() else {
            continue;
        };
        let (marks, flags) = s
            .read(|x| {
                Ok((
                    draft_marks(x, OWNER, &c).unwrap(),
                    draft_flags(x, OWNER, &c).unwrap(),
                ))
            })
            .unwrap();
        assert_eq!(names(&marks), flags, "{c:?}");
        // One mark for each piece of text: no two marks of one flag in one field overlap or touch.
        for (i, a) in marks.iter().enumerate() {
            for b in &marks[i + 1..] {
                let apart = a.range.end < b.range.start || b.range.end < a.range.start;
                assert!(
                    a.flag != b.flag || a.field != b.field || apart,
                    "{a:?} {b:?}"
                );
            }
        }
        for m in &marks {
            assert!(!m.range.is_empty(), "{m:?}");
            let text = marked(&c, m);
            let alone = match m.flag {
                "new_recipient" => text == to,
                // The whole amount: no digit, and no `,` or `.` with a digit, just outside.
                "money" => has_money(text) && whole(&c.body, &c.subject, m),
                "date" => has_date(text),
                "attachment" => has_attachment(text),
                "untrusted_content" => words(text).len() == 8 && matches!(m.source, Some(1 | 3)),
                f => panic!("{f}"),
            };
            assert!(alone, "{m:?} {text:?}");
        }
    }
}
