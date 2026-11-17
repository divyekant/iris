//! The checks before a send (docs/specs/2026-11-17-send-checks.md): the
//! draft flags of spec 2, section 5.3.

use std::fs;
use std::path::PathBuf;

use iris_changeset::{
    add_action, create, draft_flags, has_attachment, has_date, has_money, submit, write_document,
    Author, AuthorKind, Verb,
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
    // The draft that the apply makes holds the same content.
    let stored = json!({"content": {"to": ["sam@example.org"], "cc": [], "subject": "Pay",
                                    "body": "I owe you $40, see attached."}});
    s.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                               action_id, created_at, updated_at)
             VALUES (1, 'r1', 'iris.1@example.com', ?1, 'h1', x'00', 1, 0, 0)",
            [stored.to_string()],
        )?)
    })
    .unwrap();
    let a = one(Verb::Send, "d-1", &json!({}));
    assert_eq!(
        a["flags"],
        json!(["new_recipient", "money", "attachment"]),
        "{a}"
    );
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
