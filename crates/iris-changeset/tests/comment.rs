//! Review comments (docs/specs/2026-11-17-review-comments.md, card R4-08).

use std::fs;
use std::path::PathBuf;

use iris_changeset::{
    add_action, comment_in, comments_in, create, revise, submit, transaction, withdraw, Author,
    AuthorKind, Comment, Error, Verb,
};
use iris_job::{add_scope, By, Ref};
use iris_store::{NewMessage, Store};
use serde_json::Map;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-cm-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const NOW: i64 = 1_763_283_422_000;
const KEY: &str = "k-0123456789abcdef";

/// A store with one thread (m-1), job 1 with that thread in its scope, and
/// a submitted changeset of the job that archives m-1.
fn setup(dir: &TempDir) -> (Store, i64, Author) {
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    s.put_messages(
        a,
        &[NewMessage {
            provider_id: "g1".into(),
            thread_provider_id: "gt1".into(),
            header: iris_store::Header {
                from_addr: Some("dana@example.com".into()),
                ..Default::default()
            },
            ..Default::default()
        }],
    )
    .unwrap();
    // Understood, so the grant of the job holds it.
    s.write(|tx| Ok(tx.execute("UPDATE message SET understood_at = 1", [])?))
        .unwrap();
    let j = iris_job::create(&s, "Tidy", None, NOW).unwrap();
    add_scope(&s, j, Ref::Thread(1), By::Person, NOW).unwrap();
    let who = Author {
        kind: AuthorKind::Agent,
        name: Some("test".into()),
        session: Some("se-1".into()),
        job: Some(iris_job::format_job_id(j)),
    };
    let cs = create(&s, &who, NOW).unwrap();
    add_action(&s, cs, &who, Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    submit(&s, cs, &who, "Archive.", NOW).unwrap();
    (s, cs, who)
}

fn put(s: &Store, cs: i64, revision: i64, text: &str, key: &str) -> Result<Comment, Error> {
    transaction(s, |c| comment_in(c, cs, revision, text, key, NOW + 1))
}

fn count(s: &Store) -> i64 {
    s.read(|c| Ok(c.query_row("SELECT count(*) FROM changeset_comment", [], |r| r.get(0))?))
        .unwrap()
}

fn refused(r: Result<Comment, Error>) {
    assert!(matches!(r, Err(Error::Refused(_))), "{r:?}");
}

#[test]
fn a_retry_with_the_same_key_writes_one_comment() {
    let dir = TempDir::new("retry");
    let (s, cs, _) = setup(&dir);
    let first = put(&s, cs, 1, "Keep m-1 in the inbox.", KEY).unwrap();
    assert_eq!(
        put(&s, cs, 1, "Keep m-1 in the inbox.", KEY).unwrap(),
        first
    );
    assert_eq!(count(&s), 1);
    // The same key with another body names another comment.
    refused(put(&s, cs, 1, "Another text.", KEY));
    assert_eq!(count(&s), 1);
    let all = s
        .read(|c| comments_in(c, cs).map_err(|e| iris_store::Error::Query(e.to_string())))
        .unwrap();
    assert_eq!(all, vec![first]);
}

#[test]
fn each_refusal_writes_nothing() {
    let dir = TempDir::new("refuse");
    let (s, cs, who) = setup(&dir);
    for (text, key) in [
        ("", KEY),
        ("   \n", KEY),
        ("a\0b", KEY),
        (&*"x".repeat(4001), KEY),
        ("ok", "short"),
        ("ok", &*"k".repeat(65)),
        ("ok", "k-0123456789abcde!"),
    ] {
        refused(put(&s, cs, 1, text, key));
    }
    assert!(put(&s, cs, 1, &"x".repeat(4000), KEY).is_ok());
    // An old or a future revision.
    refused(put(&s, cs, 2, "ok", "k-1123456789abcdef"));
    // A draft revision.
    revise(&s, cs, &who).unwrap();
    refused(put(&s, cs, 2, "ok", "k-2123456789abcdef"));
    refused(put(&s, cs, 1, "ok", "k-3123456789abcdef"));
    // A missing changeset.
    assert!(matches!(
        put(&s, cs + 9, 1, "ok", "k-4123456789abcdef"),
        Err(Error::NotFound)
    ));
    assert_eq!(count(&s), 1);
}

#[test]
fn only_an_open_changeset_of_an_open_job_takes_a_comment() {
    let dir = TempDir::new("job");
    let (s, cs, who) = setup(&dir);
    // A changeset with no job.
    let host = Author {
        job: None,
        session: Some("se-9".into()),
        ..who.clone()
    };
    let other = create(&s, &host, NOW).unwrap();
    add_action(&s, other, &host, Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    submit(&s, other, &host, "Archive.", NOW).unwrap();
    refused(put(&s, other, 1, "ok", KEY));
    // A withdrawn changeset.
    withdraw(&s, other, Some(&host)).unwrap();
    refused(put(&s, other, 1, "ok", KEY));
    // A done job.
    iris_job::finish(
        &s,
        1,
        iris_job::By::Person,
        iris_job::End::Cancel,
        None,
        NOW,
    )
    .unwrap();
    refused(put(&s, cs, 1, "ok", KEY));
    assert_eq!(count(&s), 0);
}

#[test]
fn a_comment_never_changes() {
    let dir = TempDir::new("keep");
    let (s, cs, _) = setup(&dir);
    put(&s, cs, 1, "ok", KEY).unwrap();
    for sql in [
        "UPDATE changeset_comment SET text = 'no'",
        "DELETE FROM changeset_comment",
    ] {
        assert!(s.write(|tx| Ok(tx.execute(sql, [])?)).is_err(), "{sql}");
    }
    assert_eq!(count(&s), 1);
}

#[test]
fn a_closed_changeset_takes_no_comment() {
    let dir = TempDir::new("closed");
    let (s, cs, _) = setup(&dir);
    s.write(|tx| {
        Ok(tx.execute(
            "UPDATE changeset_action SET state = 'denied', decided_by = 'person', reason = 'No.'",
            [],
        )?)
    })
    .unwrap();
    refused(put(&s, cs, 1, "ok", KEY));
    assert_eq!(count(&s), 0);
}

#[test]
fn a_comment_wakes_a_waiting_job() {
    let dir = TempDir::new("wake");
    let (s, cs, _) = setup(&dir);
    let wait = iris_job::Wait {
        thread: None,
        until_ms: NOW + 86_400_000,
    };
    iris_job::wait(&s, 1, wait, None, NOW).unwrap();
    put(&s, cs, 1, "ok", KEY).unwrap();
    assert_eq!(
        iris_job::get(&s, 1).unwrap().state,
        iris_job::State::Working
    );
}

#[test]
fn a_comment_is_pending_until_a_session_starts_after_its_build() {
    let dir = TempDir::new("pending");
    let (s, cs, _) = setup(&dir);
    let pending = || {
        s.read(|c| Ok(iris_changeset::comment_pending_in(c, 1).unwrap()))
            .unwrap()
    };
    assert!(!pending());
    let first = put(&s, cs, 1, "one", KEY).unwrap();
    assert!(pending());
    // A session that never started has no delivery: still pending.
    let se = iris_job::register(&s, 1, "test", NOW).unwrap();
    assert!(pending());
    // The gate of a session whose build read the first comment.
    s.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO comment_delivery (session_id, comment_id) VALUES (?1, ?2)",
            [se, first.id],
        )?)
    })
    .unwrap();
    assert!(!pending());
    // A comment after that build is pending again.
    put(&s, cs, 1, "two", "k-1123456789abcdef").unwrap();
    assert!(pending());
}
