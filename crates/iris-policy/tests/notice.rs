//! Tests of notices by rule (card R5-08, `docs/specs/2026-11-16-notices.md`).

use iris_policy::notice::{self, WINDOW_MS};
use iris_policy::*;
use iris_store::Store;
use rusqlite::params;
use std::path::PathBuf;

const NOW: i64 = 1_800_000_000_000;
const MIN: i64 = 60_000;

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn store(name: &str) -> (Dir, Store) {
    let dir = std::env::temp_dir().join(format!("iris-notice-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO account(id, provider, address) VALUES (1, 'gmail', 'me@x.example');
                 INSERT INTO thread(account_id, provider_id) VALUES (1, 't');
                 INSERT INTO label(id, account_id, provider_id, name, kind) VALUES
                   (1, 1, 'INBOX', 'INBOX', 'system'), (2, 1, 'Label_1', 'Work', 'user'),
                   (3, 1, 'SENT', 'Sent', 'system'), (4, 1, 'SPAM', 'Spam', 'system'),
                   (5, 1, 'TRASH', 'Trash', 'system'), (6, 1, 'UNSENT', 'UNSENT', 'system'),
                   (7, 1, 'SENT_COPY', 'SENT_COPY', 'system');
                 INSERT INTO sender(address, kind) VALUES ('ann@friends.example', 'person'),
                   ('news@shop.example', 'service'), ('x@far.example', 'hostile');",
            )?;
            Ok(())
        })
        .unwrap();
    (Dir(dir), store)
}

fn activate(s: &Store, text: &str) {
    std::fs::write(s.dir().join(FILE), text).unwrap();
    let sha = match file_state(s).unwrap() {
        FileState::Changed(l) => l.sha256,
        other => panic!("{other:?}"),
    };
    assert!(matches!(
        confirm(s, &sha, NOW / 1000).unwrap(),
        Confirm::Done { .. }
    ));
}

/// One mail: what decides if it gets a notice.
#[derive(Clone, Debug)]
struct Mail {
    sender: &'static str,
    /// Minutes before NOW of the fetch, and of the date before the fetch.
    fetched_ago: i64,
    date_before_fetch: i64,
    /// Minutes before NOW of the understanding, if any.
    understood_ago: Option<i64>,
    body: bool,
    gone: bool,
    hostile: bool,
    labels: Vec<i64>,
    forms: Vec<&'static str>,
    subject: String,
}

fn put(s: &Store, m: &Mail, n: usize) -> i64 {
    s.write(|tx| {
        let fetched = NOW - m.fetched_ago * MIN;
        tx.execute(
            "INSERT INTO message(account_id, thread_id, provider_id, internal_date, fetched_at, gone_at, hostile, understood_at)
             VALUES (1, 1, ?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                format!("m{n}"),
                fetched - m.date_before_fetch * MIN,
                fetched,
                m.gone.then_some(1),
                m.hostile.then_some("[\"phishing\"]"),
                m.understood_ago.map(|u| NOW - u * MIN)
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO header(message_id, from_addr, from_name, subject) VALUES (?1, ?2, ?3, ?4)",
            params![id, m.sender, "", m.subject],
        )?;
        if m.body {
            tx.execute(
                "INSERT INTO body(message_id, subject, sender, text) VALUES (?1, '', '', '')",
                [id],
            )?;
        }
        for l in &m.labels {
            tx.execute("INSERT INTO message_label(message_id, label_id) VALUES (?1, ?2)", [id, *l])?;
        }
        for f in &m.forms {
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES (?1, 1, 'new', '{}', 0, 0)",
                [f],
            )?;
            tx.execute(
                "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
                [tx.last_insert_rowid(), id],
            )?;
        }
        Ok(id)
    })
    .unwrap()
}

fn notices(s: &Store) -> Vec<(i64, u32)> {
    s.read(|c| {
        Ok(
            c.prepare("SELECT message_id, rule FROM notice ORDER BY message_id")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?,
        )
    })
    .unwrap()
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[(self.next() % xs.len() as u64) as usize]
    }
    fn coin(&mut self) -> bool {
        self.next().is_multiple_of(2)
    }
}

const SENDERS: [&str; 4] = [
    "ann@friends.example",
    "news@shop.example",
    "x@far.example",
    "who@else.example",
];
const FORMS: [&str; 3] = ["commitment", "feed", "signal"];

/// A rule of the generated policy, as the oracle reads it.
#[derive(Clone, Debug)]
struct Cond {
    id: u32,
    notify: bool,
    sender: Option<&'static str>,
    kind: Option<&'static str>,
    form: Option<&'static str>,
    label: Option<&'static str>,
}

fn kind_of(m: &Mail) -> &'static str {
    match m.sender {
        _ if m.labels.contains(&3) => "self",
        _ if m.hostile => "hostile",
        "ann@friends.example" => "person",
        "news@shop.example" => "service",
        "x@far.example" => "hostile",
        _ => "stranger",
    }
}

/// The oracle of the spec: the rule of the notice of `m`, if any.
fn expected(m: &Mail, rules: &[Cond]) -> Option<u32> {
    let kind = kind_of(m);
    let late = m.understood_ago.is_some_and(|u| u <= 60) && m.fetched_ago <= 24 * 60;
    let fresh = (m.fetched_ago <= 60 || late) && m.date_before_fetch <= 60;
    let junk = m.labels.contains(&4) || m.labels.contains(&5);
    if !fresh || !m.body || m.gone || junk || kind == "hostile" || kind == "self" {
        return None;
    }
    rules
        .iter()
        .filter(|r| r.notify)
        .filter(|r| {
            r.sender.is_none_or(|s| match s.strip_prefix('@') {
                Some(d) => m.sender.ends_with(&format!("@{d}")),
                None => m.sender == s,
            }) && r.kind.is_none_or(|k| k == kind)
                && r.form.is_none_or(|f| m.forms.contains(&f))
                && r.label.is_none_or(|l| {
                    m.labels
                        .iter()
                        .any(|&i| ["INBOX", "Work", "Sent", "Spam", "Trash"][i as usize - 1] == l)
                })
        })
        .map(|r| r.id)
        .min()
}

#[test]
fn no_notice_without_a_rule_that_matches() {
    // Invariant: after a refresh, the notices are exactly the new, safe mails
    // that a notify rule matches, each with the lowest matching rule.
    let mut total = 0;
    for seed in 1..40u64 {
        let (_d, s) = store(&format!("prop{seed}"));
        let mut rng = Rng(seed * 7919);
        let mut rules = Vec::new();
        let mut text = "version = 0\nmode = \"observe\"\n".to_string();
        for id in 1..=1 + rng.next() % 4 {
            let mut c = Cond {
                id: id as u32,
                notify: !rng.next().is_multiple_of(4),
                sender: None,
                kind: None,
                form: None,
                label: None,
            };
            if rng.coin() {
                c.sender =
                    Some(rng.pick(&["ann@friends.example", "@shop.example", "@far.example"]));
            }
            if rng.coin() {
                c.kind = Some(rng.pick(&["person", "service", "stranger", "hostile", "self"]));
            }
            if rng.coin() {
                c.form = Some(rng.pick(&FORMS));
            }
            if rng.next().is_multiple_of(3) {
                c.label = Some(rng.pick(&["INBOX", "Work"]));
            }
            if c.sender.is_none() && c.kind.is_none() && c.form.is_none() && c.label.is_none() {
                c.kind = Some("person");
            }
            let mut when = Vec::new();
            for (k, v) in [
                ("sender", c.sender),
                ("sender_kind", c.kind),
                ("form", c.form),
                ("label", c.label),
            ] {
                if let Some(v) = v {
                    when.push(format!("{k} = \"{v}\""));
                }
            }
            text += &format!(
                "[[rule]]\nid = {id}\nname = \"Rule {id}\"\neffect = \"{}\"\nwhen = {{ {} }}\nby = \"person\"\nadded = 2026-11-10\n",
                if c.notify { "notify" } else { rng.pick(&["hide", "bundle"]) },
                when.join(", ")
            );
            rules.push(c);
        }
        activate(&s, &text);
        let mut want = Vec::new();
        for n in 0..30 {
            let m = Mail {
                sender: rng.pick(&SENDERS),
                fetched_ago: rng.pick(&[0, 5, 30, 59, 60, 61, 600, 1440, 1441]),
                date_before_fetch: rng.pick(&[0, 1, 30, 60, 61, 100_000]),
                understood_ago: rng.pick(&[None, Some(0), Some(60), Some(61), Some(600)]),
                body: !rng.next().is_multiple_of(5),
                gone: rng.next().is_multiple_of(9),
                hostile: rng.next().is_multiple_of(7),
                labels: [1, 2, 3, 4, 5]
                    .into_iter()
                    .filter(|_| rng.next().is_multiple_of(4))
                    .collect(),
                forms: (0..rng.next() % 3).map(|_| rng.pick(&FORMS)).collect(),
                subject: format!("Mail {n}"),
            };
            let id = put(&s, &m, n);
            if let Some(r) = expected(&m, &rules) {
                want.push((id, r));
            }
        }
        let new = notice::refresh(&s, NOW).unwrap();
        assert_eq!(notices(&s), want, "seed {seed}\n{text}");
        assert_eq!(new.len(), want.len());
        // A second pass adds nothing.
        assert!(notice::refresh(&s, NOW + 1).unwrap().is_empty());
        assert_eq!(notices(&s), want);
        total += want.len();
    }
    assert!(total > 20, "the generator gives notices: {total}");
}

fn mail() -> Mail {
    Mail {
        sender: "ann@friends.example",
        fetched_ago: 1,
        date_before_fetch: 1,
        understood_ago: None,
        body: true,
        gone: false,
        hostile: false,
        labels: vec![1],
        forms: vec![],
        subject: "Lunch?".into(),
    }
}

const ANN: &str = "version = 0\nmode = \"observe\"\n[[rule]]\nid = 4\nname = \"From Ann\"\neffect = \"notify\"\nwhen = { sender = \"ann@friends.example\" }\nby = \"person\"\nadded = 2026-11-10\n";

#[test]
fn a_mail_waits_for_its_body_then_gets_one_notice() {
    let (_d, s) = store("body");
    activate(&s, ANN);
    let id = put(
        &s,
        &Mail {
            body: false,
            ..mail()
        },
        0,
    );
    assert!(notice::refresh(&s, NOW).unwrap().is_empty());
    s.write(|tx| {
        tx.execute(
            "INSERT INTO body(message_id, subject, sender, text) VALUES (?1, '', '', '')",
            [id],
        )?;
        Ok(())
    })
    .unwrap();
    let new = notice::refresh(&s, NOW).unwrap();
    assert_eq!(new.len(), 1);
    assert_eq!((new[0].rule, new[0].rule_name.as_str()), (4, "From Ann"));
    assert_eq!(
        (new[0].title.as_str(), new[0].text.as_str()),
        ("ann@friends.example", "Lunch?")
    );
    // After its hour, a mail is not new.
    assert!(notice::refresh(&s, NOW + WINDOW_MS).unwrap().is_empty());
}

#[test]
fn no_notice_without_an_active_policy() {
    let (_d, s) = store("none");
    put(&s, &mail(), 0);
    assert!(notice::refresh(&s, NOW).unwrap().is_empty());
}

#[test]
fn done_hides_a_notice_from_the_list() {
    let (_d, s) = store("done");
    activate(&s, ANN);
    put(&s, &mail(), 0);
    put(&s, &mail(), 1);
    let new = notice::refresh(&s, NOW).unwrap();
    let list = s.read(notice::list).unwrap();
    assert_eq!(
        list.iter().map(|n| n.id).collect::<Vec<_>>(),
        [new[1].id, new[0].id]
    );
    assert!(notice::done(&s, new[0].id, NOW).unwrap());
    assert!(
        notice::done(&s, new[0].id, NOW + 1).unwrap(),
        "done again is fine"
    );
    assert!(!notice::done(&s, 999, NOW).unwrap());
    let list = s.read(notice::list).unwrap();
    assert_eq!(list.iter().map(|n| n.id).collect::<Vec<_>>(), [new[1].id]);
}

#[test]
fn notice_words_are_one_short_line() {
    assert_eq!(notice::line(" a\r\nb\u{0}\tc  "), "a b c");
    let long = notice::line(&"y ".repeat(10_000));
    assert_eq!(long.chars().count(), notice::WORDS_MAX);
    assert!(long.ends_with("..."));
    // Any text: one line, no control character, at most WORDS_MAX characters.
    let mut rng = Rng(99);
    for _ in 0..500 {
        let s: String = (0..rng.next() % 400)
            .map(|_| char::from_u32((rng.next() % 0x3000) as u32).unwrap_or('?'))
            .collect();
        let l = notice::line(&s);
        assert!(l.chars().count() <= notice::WORDS_MAX);
        assert!(!l.chars().any(|c| c.is_control()), "{l:?}");
    }
}

#[test]
fn a_notify_rule_needs_no_verbs_and_decides_no_action() {
    let p = parse(ANN).unwrap();
    assert_eq!(p.rules[0].effect, Effect::Notify);
    let bad = ANN.replace(
        "effect = \"notify\"",
        "effect = \"notify\"\nverbs = [\"archive\"]",
    );
    assert!(parse(&bad).is_err());
}

#[test]
fn a_spam_label_after_the_body_stops_the_notice() {
    let (_d, s) = store("spam");
    activate(&s, ANN);
    let id = put(&s, &mail(), 0);
    s.write(|tx| {
        tx.execute(
            "INSERT INTO message_label(message_id, label_id) VALUES (?1, 4)",
            [id],
        )?;
        Ok(())
    })
    .unwrap();
    assert!(notice::refresh(&s, NOW).unwrap().is_empty());
}

/// An IMAP `\\Draft` flag (the label `UNSENT`) or a move to a Sent folder
/// copy (`SENT_COPY`) after the body stops the notice (IMAP spec section 4).
#[test]
fn a_draft_or_copy_label_after_the_body_stops_the_notice() {
    for label in [6, 7] {
        let (_d, s) = store(&format!("copy-{label}"));
        activate(&s, ANN);
        let id = put(&s, &mail(), 0);
        s.write(|tx| {
            tx.execute(
                "INSERT INTO message_label(message_id, label_id) VALUES (?1, ?2)",
                [id, label],
            )?;
            Ok(())
        })
        .unwrap();
        assert!(notice::refresh(&s, NOW).unwrap().is_empty(), "{label}");
    }
}

#[test]
fn a_notice_keeps_the_rule_of_its_time() {
    let (_d, s) = store("keep");
    activate(&s, ANN);
    put(&s, &mail(), 0);
    let new = notice::refresh(&s, NOW).unwrap();
    activate(&s, "version = 0\nmode = \"observe\"\n");
    let list = s.read(notice::list).unwrap();
    assert_eq!(list, new);
    let version: i64 = s
        .read(|c| Ok(c.query_row("SELECT policy_version FROM notice", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(version, 1);
}

#[test]
fn a_form_rule_waits_for_a_late_understanding() {
    let (_d, s) = store("late");
    activate(
        &s,
        &ANN.replace("sender = \"ann@friends.example\"", "form = \"feed\""),
    );
    put(
        &s,
        &Mail {
            fetched_ago: 180,
            understood_ago: Some(10),
            forms: vec!["feed"],
            ..mail()
        },
        0,
    );
    put(
        &s,
        &Mail {
            fetched_ago: 180,
            understood_ago: Some(70),
            forms: vec!["feed"],
            ..mail()
        },
        1,
    );
    put(
        &s,
        &Mail {
            fetched_ago: 180,
            date_before_fetch: 61,
            understood_ago: Some(10),
            forms: vec!["feed"],
            ..mail()
        },
        2,
    );
    put(
        &s,
        &Mail {
            fetched_ago: 1441,
            understood_ago: Some(10),
            forms: vec!["feed"],
            ..mail()
        },
        3,
    );
    notice::refresh(&s, NOW).unwrap();
    assert_eq!(notices(&s), [(1, 4)]);
}

#[test]
fn a_mail_waits_for_its_link_check_then_gets_one_notice() {
    let (_d, s) = store("link-check");
    activate(&s, ANN);
    let id = put(&s, &mail(), 0);
    // A saved mail from before the mark `link` (migration 20).
    s.write(|tx| {
        tx.execute("INSERT INTO link_check(message_id) VALUES (?1)", [id])?;
        Ok(())
    })
    .unwrap();
    assert!(notice::refresh(&s, NOW).unwrap().is_empty());
    s.write(|tx| {
        tx.execute("DELETE FROM link_check WHERE message_id = ?1", [id])?;
        Ok(())
    })
    .unwrap();
    assert_eq!(notice::refresh(&s, NOW).unwrap().len(), 1);
}

/// Decision D33 (issue 257): a notice whose mail becomes hostile mail after
/// the notice (a model mark, the label SPAM, a hostile sender) is not in
/// the list, the Mac page or the open count.
#[test]
fn a_later_hostile_sign_hides_the_notice() {
    let cases = [
        "UPDATE message SET hostile = '[\"phishing\"]' WHERE id = ?1",
        "INSERT INTO message_label(message_id, label_id) VALUES (?1, 4)",
        "UPDATE sender SET kind = 'hostile' WHERE address =
           (SELECT from_addr FROM header WHERE message_id = ?1)",
    ];
    for (i, sql) in cases.iter().enumerate() {
        let (_d, s) = store(&format!("later-{i}"));
        activate(&s, ANN);
        let id = put(&s, &mail(), 0);
        let new = notice::refresh(&s, NOW).unwrap();
        assert_eq!(new.len(), 1, "case {i}");
        assert!(s.read(|c| notice::is_open(c, new[0].id)).unwrap());
        s.write(|tx| Ok(tx.execute(sql, [id])?)).unwrap();
        // The system notice checks again at the show (it waits in a queue).
        assert!(
            !s.read(|c| notice::is_open(c, new[0].id)).unwrap(),
            "case {i}"
        );
        assert!(s.read(notice::list).unwrap().is_empty(), "case {i}");
        assert!(
            s.read(|c| notice::after(c, 0, 10)).unwrap().is_empty(),
            "case {i}"
        );
        assert_eq!(s.read(notice::counts).unwrap(), (0, new[0].id), "case {i}");
    }
}
