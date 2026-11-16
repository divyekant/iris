use std::cell::RefCell;

use iris_model::{Reply, Stop, Usage};
use iris_store::{Body, Header, NewMessage, Store};

use super::*;

/// A model that records each call and gives a fixed answer. `during` runs
/// inside the call, as a sync would.
struct Fake<'a> {
    answer: String,
    stop: Stop,
    calls: RefCell<Vec<(String, String)>>,
    during: Option<Box<dyn Fn() + 'a>>,
}

impl<'a> Fake<'a> {
    fn new(answer: &str) -> Self {
        Fake {
            answer: answer.into(),
            stop: Stop::End,
            calls: RefCell::new(Vec::new()),
            during: None,
        }
    }
}

impl Model for Fake<'_> {
    fn name(&self) -> &str {
        "fake"
    }

    fn ask(&self, instruction: &str, data: &str) -> std::result::Result<Reply, iris_model::Error> {
        self.calls
            .borrow_mut()
            .push((instruction.to_string(), data.to_string()));
        if let Some(f) = &self.during {
            f();
        }
        Ok(Reply {
            text: self.answer.clone(),
            usage: Usage::default(),
            stop: self.stop,
        })
    }
}

struct Box_ {
    store: Store,
    dir: std::path::PathBuf,
}

impl Drop for Box_ {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn open(name: &str) -> Box_ {
    let dir = std::env::temp_dir().join(format!("iris-voice-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("gmail", "me@example.com").unwrap();
    store
        .put_labels(
            account,
            &[
                ("SENT", "SENT", "system"),
                ("INBOX", "INBOX", "system"),
                ("DRAFT", "DRAFT", "system"),
                ("SPAM", "SPAM", "system"),
            ],
        )
        .unwrap();
    Box_ { store, dir }
}

fn msg(
    id: &str,
    thread: &str,
    date: i64,
    label: &str,
    from: &str,
    to: &str,
    text: Option<&str>,
) -> NewMessage {
    NewMessage {
        provider_id: id.into(),
        thread_provider_id: thread.into(),
        internal_date: Some(date),
        labels: vec![label.into()],
        header: Header {
            from_addr: Some(from.into()),
            to_addrs: Some(to.into()),
            subject: Some("Lunch".into()),
            ..Default::default()
        },
        body: text.map(|t| Body {
            subject: "Lunch".into(),
            sender: from.into(),
            text: t.into(),
        }),
        ..Default::default()
    }
}

fn thread_of(store: &Store, provider: &str) -> i64 {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT id FROM thread WHERE provider_id = ?1",
                [provider],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

/// A mailbox with one thread from Ana and sent mail to Ana and to Bo.
fn mailbox(name: &str) -> (Box_, i64) {
    let b = open(name);
    b.store
        .put_messages(
            1,
            &[
                msg(
                    "s1",
                    "old1",
                    100,
                    "SENT",
                    "me@example.com",
                    "bo@example.org",
                    Some("Yo Bo. Later, D"),
                ),
                msg(
                    "s2",
                    "old2",
                    200,
                    "SENT",
                    "me@example.com",
                    "ana@example.org",
                    Some("Hi Ana, sure thing. Best, D"),
                ),
                msg(
                    "s3",
                    "old3",
                    300,
                    "SENT",
                    "me@example.com",
                    "cy@example.org",
                    Some("Cy: done. D"),
                ),
                msg(
                    "in",
                    "t1",
                    400,
                    "INBOX",
                    "ana@example.org",
                    "me@example.com",
                    Some("Can we meet on Friday?"),
                ),
            ],
        )
        .unwrap();
    let t = thread_of(&b.store, "t1");
    (b, t)
}

fn saved(store: &Store, thread: i64) -> Option<(String, Option<String>, i64)> {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT body, intent, samples FROM reply_draft WHERE thread_id = ?1",
                [thread],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?)
        })
        .unwrap()
}

#[test]
fn draft_uses_sent_mail_to_the_same_person_first_and_replaces_the_last_draft() {
    let (b, t) = mailbox("voice");
    let fake = Fake::new(r#"{"body": "Hi Ana, Friday works. Best, D"}"#);
    let s = write(&b.store, t, Some(" say yes "), &fake, 1000).unwrap();
    assert_eq!(s.body, "Hi Ana, Friday works. Best, D");
    assert_eq!(s.samples, 3);
    let calls = fake.calls.borrow();
    let (instruction, data) = &calls[0];
    assert_eq!(instruction, INSTRUCTION);
    assert!(data.contains("Reply to: ana@example.org"));
    assert!(data.contains("Intent of the owner: say yes\n"));
    // The mail to Ana comes first, then the newest.
    let ana = data.find("sure thing").unwrap();
    let cy = data.find("Cy: done").unwrap();
    let bo = data.find("Yo Bo").unwrap();
    assert!(ana < cy && cy < bo, "{data}");
    assert!(data.contains("Can we meet on Friday?"));
    drop(calls);
    assert_eq!(
        saved(&b.store, t),
        Some((
            "Hi Ana, Friday works. Best, D".into(),
            Some("say yes".into()),
            3
        ))
    );

    let fake = Fake::new(r#"{"body": "Hi Ana, no. D"}"#);
    write(&b.store, t, None, &fake, 2000).unwrap();
    assert_eq!(saved(&b.store, t), Some(("Hi Ana, no. D".into(), None, 3)));
}

#[test]
fn hostile_thread_gets_no_call() {
    for how in ["mark", "spam", "sender"] {
        let (b, t) = mailbox(&format!("hostile-{how}"));
        b.store
            .write(|tx| {
                match how {
                    "mark" => tx.execute(
                        "UPDATE message SET hostile = '[\"spoof\"]' WHERE thread_id = ?1",
                        [t],
                    )?,
                    "spam" => tx.execute(
                        "INSERT INTO message_label(message_id, label_id)
                         SELECT m.id, l.id FROM message m, label l
                         WHERE m.thread_id = ?1 AND l.provider_id = 'SPAM'",
                        [t],
                    )?,
                    _ => tx.execute(
                        "INSERT INTO sender(address, kind) VALUES ('ana@example.org', 'hostile')",
                        [],
                    )?,
                };
                Ok(())
            })
            .unwrap();
        let fake = Fake::new(r#"{"body": "x"}"#);
        assert!(
            matches!(write(&b.store, t, None, &fake, 1), Err(Error::Hostile)),
            "{how}"
        );
        assert!(fake.calls.borrow().is_empty());
        assert_eq!(saved(&b.store, t), None);
    }
}

#[test]
fn message_not_read_by_understanding_blocks_the_draft_but_a_gmail_draft_does_not() {
    let (b, t) = mailbox("ready");
    // A Gmail draft of the owner with no clean text: not part of the thread.
    b.store
        .put_messages(
            1,
            &[msg(
                "d",
                "t1",
                500,
                "DRAFT",
                "me@example.com",
                "ana@example.org",
                None,
            )],
        )
        .unwrap();
    let fake = Fake::new(r#"{"body": "ok"}"#);
    write(&b.store, t, None, &fake, 1).unwrap();
    assert_eq!(fake.calls.borrow().len(), 1);

    b.store
        .put_messages(
            1,
            &[msg(
                "new",
                "t1",
                600,
                "INBOX",
                "ana@example.org",
                "me@example.com",
                None,
            )],
        )
        .unwrap();
    let fake = Fake::new(r#"{"body": "ok"}"#);
    assert!(matches!(
        write(&b.store, t, None, &fake, 2),
        Err(Error::NotReady)
    ));
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn a_sync_during_the_call_saves_nothing() {
    let (b, t) = mailbox("changed");
    let store = &b.store;
    let mut fake = Fake::new(r#"{"body": "ok"}"#);
    fake.during = Some(Box::new(move || {
        store
            .put_messages(
                1,
                &[msg(
                    "late",
                    "t1",
                    700,
                    "INBOX",
                    "ana@example.org",
                    "me@example.com",
                    Some("Or Monday?"),
                )],
            )
            .unwrap();
    }));
    assert!(matches!(
        write(store, t, None, &fake, 1),
        Err(Error::Changed)
    ));
    assert_eq!(saved(store, t), None);
}

#[test]
fn invalid_answers_save_nothing() {
    let (b, t) = mailbox("bad");
    let long = format!(r#"{{"body": "{}"}}"#, "a".repeat(MAX_BODY + 1));
    for answer in [
        "not json",
        r#"{"text": "x"}"#,
        r#"{"body": "  "}"#,
        r#"{"body": 3}"#,
        &long,
    ] {
        let fake = Fake::new(answer);
        assert!(
            matches!(write(&b.store, t, None, &fake, 1), Err(Error::BadAnswer(_))),
            "{answer}"
        );
    }
    for stop in [Stop::Truncated, Stop::Refused] {
        let mut fake = Fake::new(r#"{"body": "ok"}"#);
        fake.stop = stop;
        assert!(matches!(
            write(&b.store, t, None, &fake, 1),
            Err(Error::BadAnswer(_))
        ));
    }
    assert_eq!(saved(&b.store, t), None);
}

#[test]
fn bad_input_gets_no_call() {
    let (b, t) = mailbox("input");
    let fake = Fake::new(r#"{"body": "x"}"#);
    let long = "a".repeat(MAX_INTENT + 1);
    assert!(matches!(
        write(&b.store, t, Some(&long), &fake, 1),
        Err(Error::LongIntent)
    ));
    assert!(matches!(
        write(&b.store, 999, None, &fake, 1),
        Err(Error::NoThread(999))
    ));
    assert!(fake.calls.borrow().is_empty());
}

/// For any text, the cut keeps at most `limit` characters of it, keeps a
/// prefix, and never splits a character.
#[test]
fn cut_keeps_a_prefix_of_at_most_the_limit() {
    const PIECES: [&str; 6] = ["a", "é", "日", "😀", " ", "\n"];
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..3000 {
        let mut text = String::new();
        for _ in 0..(x % 40) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            text.push_str(PIECES[(x % PIECES.len() as u64) as usize]);
        }
        let limit = (x % 30) as usize;
        let out = cut(&text, limit);
        let core = out.strip_suffix(CUT_MARK).unwrap_or(&out);
        assert!(core.chars().count() <= limit, "{text:?} {limit}");
        assert!(text.trim().starts_with(core), "{text:?} {limit}");
    }
}
