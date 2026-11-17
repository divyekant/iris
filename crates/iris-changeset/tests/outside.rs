//! Limits for hostile input (docs/specs/2026-11-17-hostile-input-limits.md,
//! card R4-06): the mark, the flag `outside_thread`, and the store limit
//! on the approval of a flagged draft or send.

use std::fs;
use std::path::PathBuf;

use iris_changeset::{add_action, create, submit, Author, AuthorKind, Verb};
use iris_job::{add_scope, By, Ref};
use iris_store::{Header, NewMessage, Store};
use serde_json::{json, Map, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-out-{name}-{}", std::process::id()));
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

/// The addresses of thread 1, in the headers of its messages.
const THREAD: [&str; 3] = ["priya@example.com", "ben@example.org", "cara@example.net"];
/// Addresses that no message of the store names.
const NEW: [&str; 2] = ["eve@evil.example", "x@example.org"];

/// Thread gt1 (t-1): message 1 from priya to the owner, Cc ben; message 2
/// sent by the owner to priya and cara. Thread gt2: message 3, sent by the
/// owner only. Thread gt3: message 4, a received message with a forged From
/// of the owner.
fn store(dir: &TempDir) -> Store {
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", OWNER).unwrap();
    s.put_labels(a, &[("SENT", "SENT", "system")]).unwrap();
    let m = |id: &str, thread: &str, from: &str, to: &str, cc: &str, labels: &[&str]| NewMessage {
        provider_id: id.into(),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        thread_provider_id: thread.into(),
        internal_date: Some(1),
        header: Header {
            from_addr: Some(from.into()),
            to_addrs: Some(to.into()),
            cc_addrs: (!cc.is_empty()).then(|| cc.into()),
            subject: Some("Plan".into()),
            message_id_hdr: Some(format!("<{id}@x.org>")),
            ..Default::default()
        },
        ..Default::default()
    };
    s.put_messages(
        a,
        &[
            m(
                "g1",
                "gt1",
                "priya@example.com",
                OWNER,
                "Ben <BEN@example.org>",
                &[],
            ),
            m(
                "g2",
                "gt1",
                OWNER,
                "\"Priya\" <priya@example.com>, cara@example.net",
                "",
                &["SENT"],
            ),
            m("g3", "gt2", OWNER, "dan@example.com", "", &["SENT"]),
            m("g4", "gt3", OWNER, OWNER, "", &[]),
        ],
    )
    .unwrap();
    s
}

fn author(kind: AuthorKind, job: Option<i64>) -> Author {
    Author {
        kind,
        name: (kind != AuthorKind::Person).then(|| "test".into()),
        session: job.map(|_| "se-1".into()),
        job: job.map(iris_job::format_job_id),
    }
}

/// A job with the scope of `thread`.
fn job(s: &Store, gmail_thread: &str) -> i64 {
    let j = iris_job::create(s, "Answer", None, NOW).unwrap();
    add_scope(s, j, Ref::Thread(thread(s, gmail_thread)), By::Person, NOW).unwrap();
    j
}

/// A reply takes no subject: Iris sets it from the thread.
fn mail(reply: bool, to: &[&str], cc: &[&str]) -> Map<String, Value> {
    let Value::Object(mut m) = json!({"to": to, "cc": cc, "subject": "Plan", "body": "See you."})
    else {
        unreachable!()
    };
    if reply {
        m.remove("subject");
    }
    m
}

/// The row ID of the thread with this Gmail ID.
fn thread(s: &Store, provider: &str) -> i64 {
    s.read(|c| {
        Ok(c.query_row(
            "SELECT id FROM thread WHERE provider_id = ?1",
            [provider],
            |r| r.get(0),
        )?)
    })
    .unwrap()
}

/// Submit one draft of `who` to `target`, and give its action ID and flags.
fn draft(s: &Store, who: &Author, target: &str, to: &[&str], cc: &[&str]) -> (i64, Vec<String>) {
    let cs = create(s, who, NOW).unwrap();
    let n = add_action(
        s,
        cs,
        who,
        Verb::Draft,
        &[target],
        &mail(target.starts_with("t-"), to, cc),
        None,
    )
    .unwrap();
    submit(s, cs, who, "Answer", NOW).unwrap();
    // add_action gives the number of the action in its changeset.
    let (a, flags): (i64, String) = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT id, flags FROM changeset_action WHERE changeset_id = ?1 AND n = ?2",
                [cs, n],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    (a, serde_json::from_str(&flags).unwrap())
}

#[test]
fn the_mark_reads_the_scope_of_the_job() {
    let dir = TempDir::new("mark");
    let s = store(&dir);
    let read = |j| s.read(|c| Ok(iris_job::reads_outside_in(c, j)?)).unwrap();
    // Thread 1 has mail from priya; thread 2 only your sent mail; thread 3
    // a forged From of your address, with no SENT label.
    assert!(read(job(&s, "gt1")));
    assert!(!read(job(&s, "gt2")));
    assert!(read(job(&s, "gt3")));
    // A revoked ref still marks: the agent read the mail before the revoke.
    let j = job(&s, "gt1");
    iris_job::revoke_scope(&s, j, Ref::Thread(thread(&s, "gt1")), By::Person, NOW).unwrap();
    assert!(read(j));
    // A scope message that left the store marks the job (fail closed).
    let j = job(&s, "gt2");
    s.write(|tx| {
        tx.execute_batch(
            "PRAGMA foreign_keys = OFF;
             DELETE FROM message_label WHERE message_id = 3;",
        )?;
        Ok(())
    })
    .unwrap();
    assert!(read(j));
}

#[test]
fn a_reply_of_a_marked_author_outside_the_thread_has_the_flag() {
    let dir = TempDir::new("flag");
    let s = store(&dir);
    let local = author(AuthorKind::Local, None);
    let (_, f) = draft(
        &s,
        &local,
        "t-1",
        &["Priya@Example.com"],
        &["ben@example.org"],
    );
    assert!(!f.contains(&"outside_thread".to_string()), "{f:?}");
    let (_, f) = draft(
        &s,
        &local,
        "t-1",
        &["priya@example.com"],
        &["eve@evil.example"],
    );
    assert!(f.contains(&"outside_thread".to_string()), "{f:?}");
    // The person is never marked.
    let me = author(AuthorKind::Person, None);
    let (_, f) = draft(&s, &me, "t-1", &["eve@evil.example"], &[]);
    assert!(!f.contains(&"outside_thread".to_string()), "{f:?}");
    // A new mail has no thread: each address that is not yours is outside.
    let (_, f) = draft(&s, &local, "a-1", &["priya@example.com"], &[]);
    assert!(f.contains(&"outside_thread".to_string()), "{f:?}");
    let (_, f) = draft(&s, &local, "a-1", &[OWNER], &[]);
    assert!(!f.contains(&"outside_thread".to_string()), "{f:?}");
}

#[test]
fn a_gone_thread_message_gives_no_thread_address() {
    let dir = TempDir::new("gone");
    let s = store(&dir);
    s.write(|tx| Ok(tx.execute("UPDATE message SET gone_at = 1 WHERE id = 1", [])?))
        .unwrap();
    let local = author(AuthorKind::Local, None);
    // Ben is only in message 1, which is gone; priya is in message 2 too.
    let (_, f) = draft(&s, &local, "t-1", &["priya@example.com"], &[]);
    assert!(!f.contains(&"outside_thread".to_string()), "{f:?}");
    let (_, f) = draft(&s, &local, "t-1", &["ben@example.org"], &[]);
    assert!(f.contains(&"outside_thread".to_string()), "{f:?}");
}

/// A job agent: the mark of its changeset is the mark of its job. Submit
/// refuses each draft of a job today (no grant of an account, spec 2
/// section 8.2), so the test reads the mark that submit uses.
#[test]
fn a_job_agent_is_marked_by_its_job() {
    let dir = TempDir::new("job");
    let s = store(&dir);
    let mark = |who: &Author| {
        let cs = create(&s, who, NOW).unwrap();
        s.read(|c| Ok(iris_changeset::marked_in(c, cs).unwrap()))
            .unwrap()
    };
    let marked = author(AuthorKind::Agent, Some(job(&s, "gt1")));
    let clean = author(AuthorKind::Agent, Some(job(&s, "gt2")));
    assert!(mark(&marked));
    assert!(!mark(&clean));
    assert!(mark(&author(AuthorKind::Agent, None)));
    assert!(mark(&author(AuthorKind::Triage, None)));
    assert!(mark(&author(AuthorKind::Local, None)));
    assert!(!mark(&author(AuthorKind::Person, None)));
}

/// A rule approval, through SQL, with a valid rule of the active policy,
/// so only the limit of migration 23 can refuse it.
fn rule_approve(s: &Store, action: i64) -> Result<(), String> {
    s.write(|tx| {
        Ok(tx.execute(
            "UPDATE changeset_action SET state = 'approved', decided_by = 'rule', rule = 1,
               policy = (SELECT max(version) FROM policy_version), reason = 'Rule 1 allows draft.',
               decided_at = ?2
             WHERE id = ?1",
            rusqlite::params![action, NOW],
        )?)
    })
    .map(|_| ())
    .map_err(|e| e.to_string())
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[(self.next() % xs.len() as u64) as usize]
    }
}

/// Acceptance 3: for random authors and recipients, the flag is set
/// exactly when the model of the spec says so, and no rule approval of a
/// draft with a warning flag passes.
#[test]
fn property_no_outside_send_of_a_marked_author_passes_without_review() {
    let dir = TempDir::new("prop");
    let s = store(&dir);
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('', '', 0);
             INSERT INTO policy_rule_id(id) VALUES (1);",
        )?;
        Ok(())
    })
    .unwrap();
    // (author, marked). A job agent cannot submit a draft: see
    // a_job_agent_is_marked_by_its_job.
    let authors = [
        (author(AuthorKind::Person, None), false),
        (author(AuthorKind::Local, None), true),
        (author(AuthorKind::Triage, None), true),
        (author(AuthorKind::Agent, None), true),
    ];
    let mut pool: Vec<String> = THREAD.iter().chain(&NEW).map(|a| a.to_string()).collect();
    pool.push(OWNER.into());
    // A recipient of your sent mail in another thread: not yours.
    pool.push("dan@example.com".into());
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut flagged = 0;
    for _ in 0..300 {
        let (who, marked) = rng.pick(&authors);
        let reply = rng.next().is_multiple_of(2);
        let n = 1 + rng.next() % 3;
        let mut to = Vec::new();
        for _ in 0..n {
            let a = rng.pick(&pool).clone();
            to.push(match rng.next() % 3 {
                0 => a.to_uppercase(),
                _ => a,
            });
        }
        // The first address goes to `to`, the others to `cc`.
        let all: Vec<&str> = to.iter().map(String::as_str).collect();
        let (a, f) = draft(
            &s,
            who,
            if reply { "t-1" } else { "a-1" },
            &all[..1],
            &all[1..],
        );
        let state: Option<String> = s
            .read(|c| {
                Ok(c.query_row(
                    "SELECT state FROM changeset_action WHERE id = ?1",
                    [a],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        // The model: an address that is not yours (the owner is the only
        // From of sent mail) and, for a reply, not in the headers of thread 1.
        let outside = all.iter().any(|a| {
            let a = a.to_ascii_lowercase();
            a != OWNER && !(reply && THREAD.contains(&a.as_str()))
        });
        let want = *marked && outside;
        let has = f.iter().any(|x| x == "outside_thread");
        assert_eq!(has, want, "{who:?} {all:?} reply={reply} {f:?} {state:?}");
        // No rule approves a draft with a warning flag.
        let warned = f
            .iter()
            .any(|x| ["new_recipient", "outside_thread", "money", "date"].contains(&x.as_str()));
        if state.as_deref() == Some("proposed") && warned {
            flagged += 1;
            let e = rule_approve(&s, a).unwrap_err();
            assert!(e.contains("only a person approves"), "{e}");
        }
    }
    assert!(flagged > 20, "{flagged}");
}

/// The limit refuses only a flagged draft: a rule approval of a draft
/// with no warning flag passes, and a person approves a flagged one.
#[test]
fn the_store_limit_refuses_only_a_flagged_non_person_approval() {
    let dir = TempDir::new("limit");
    let s = store(&dir);
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('', '', 0);
             INSERT INTO policy_rule_id(id) VALUES (1);
             INSERT INTO sender_evidence(message_id, address, rule) VALUES (2, 'priya@example.com', 'sent');",
        )?;
        Ok(())
    })
    .unwrap();
    let local = author(AuthorKind::Local, None);
    let (a, f) = draft(&s, &local, "t-1", &["priya@example.com"], &[]);
    assert!(f.is_empty(), "{f:?}");
    rule_approve(&s, a).unwrap();
    let (a, f) = draft(&s, &local, "t-1", &["eve@evil.example"], &[]);
    assert!(!f.is_empty(), "{f:?}");
    assert!(rule_approve(&s, a)
        .unwrap_err()
        .contains("only a person approves"));
}

/// A warning flag that an update adds to a draft that a rule approved: the
/// store limit refuses it too (the trigger reads the new row).
#[test]
fn a_flag_added_after_a_rule_approval_is_refused() {
    let dir = TempDir::new("late");
    let s = store(&dir);
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('', '', 0);
             INSERT INTO policy_rule_id(id) VALUES (1);
             INSERT INTO sender_evidence(message_id, address, rule) VALUES (2, 'priya@example.com', 'sent');",
        )?;
        Ok(())
    })
    .unwrap();
    let local = author(AuthorKind::Local, None);
    let (a, f) = draft(&s, &local, "t-1", &["priya@example.com"], &[]);
    assert!(f.is_empty(), "{f:?}");
    rule_approve(&s, a).unwrap();
    for flag in ["outside_thread", "new_recipient", "money", "date"] {
        let e = s
            .write(|tx| {
                Ok(tx.execute(
                    "UPDATE changeset_action SET flags = json_array(?2) WHERE id = ?1",
                    rusqlite::params![a, flag],
                )?)
            })
            .unwrap_err()
            .to_string();
        assert!(e.contains("only a person approves"), "{flag}: {e}");
    }
    // A flag outside the hard limit still goes on.
    s.write(|tx| {
        Ok(tx.execute(
            "UPDATE changeset_action SET flags = '[\"attachment\"]' WHERE id = ?1",
            [a],
        )?)
    })
    .unwrap();
}

/// The store limit for a `send`: a send of a marked author to a draft with
/// an outside address gets the flag, and no rule approval passes, at the
/// move to approved or by a flag added after a rule approval.
#[test]
fn the_store_limit_holds_for_a_send() {
    let dir = TempDir::new("send");
    let s = store(&dir);
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('', '', 0);
             INSERT INTO policy_rule_id(id) VALUES (1);
             INSERT INTO sender_evidence(message_id, address, rule) VALUES (2, 'priya@example.com', 'sent');",
        )?;
        Ok(())
    })
    .unwrap();
    let local = author(AuthorKind::Local, None);
    // A draft row for each draft action, as the apply writes it.
    let send = |draft_action: i64, to: &str, provider: &str| -> (i64, Vec<String>) {
        let c = iris_compose::Content {
            to: vec![to.into()],
            cc: Vec::new(),
            subject: "Plan".into(),
            body: "See you.".into(),
        }
        .check()
        .unwrap();
        let hdr = format!("iris.{provider}@example.com");
        let raw = iris_compose::compose(OWNER, &hdr, &c, None, 1);
        let hash = iris_compose::hash(&c, None);
        let d: i64 = s
            .write(|tx| {
                tx.execute(
                    "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                                       action_id, created_at, updated_at)
                     VALUES (1, ?1, ?2, '{}', ?3, ?4, ?5, 0, 0)",
                    rusqlite::params![provider, hdr, hash, raw, draft_action],
                )?;
                Ok(tx.last_insert_rowid())
            })
            .unwrap();
        let cs = create(&s, &local, NOW).unwrap();
        let n = add_action(
            &s,
            cs,
            &local,
            Verb::Send,
            &[&format!("d-{d}")],
            &Map::new(),
            None,
        )
        .unwrap();
        submit(&s, cs, &local, "Send", NOW).unwrap();
        s.read(|c| {
            let (a, f): (i64, String) = c.query_row(
                "SELECT id, flags FROM changeset_action WHERE changeset_id = ?1 AND n = ?2",
                rusqlite::params![cs, n],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok((a, serde_json::from_str(&f).unwrap()))
        })
        .unwrap()
    };
    let (d1, _) = draft(&s, &local, "a-1", &["priya@example.com"], &[]);
    let (d2, _) = draft(&s, &local, "a-1", &["priya@example.com"], &[]);
    // A new mail to eve: outside, so flagged, and no rule approves it.
    let (a, f) = send(d1, "eve@evil.example", "r1");
    assert!(f.contains(&"outside_thread".to_string()), "{f:?}");
    assert!(rule_approve(&s, a)
        .unwrap_err()
        .contains("only a person approves"));
    // A send to your own address has no warning flag: a rule approval
    // passes, and a warning flag added later is refused.
    let (a, f) = send(d2, OWNER, "r2");
    assert!(f.is_empty(), "{f:?}");
    rule_approve(&s, a).unwrap();
    let e = s
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE changeset_action SET flags = '[\"outside_thread\"]' WHERE id = ?1",
                [a],
            )?)
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("only a person approves"), "{e}");
}
