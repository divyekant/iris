//! The wake rule of R5-04 (docs/specs/2026-11-17-wait.md): which waiting
//! job wakes, why, and what a reply adds to the grant.

use std::fs;
use std::path::PathBuf;

use iris_job::{
    add_scope, create, events, get, register, revoke_scope, wait, woken, By, Ref, State, Wait,
};
use iris_store::Store;
use iris_workspace::wake_due;

const NOW: i64 = 1_763_283_422_000;
const DAY: i64 = 86_400_000;

struct Fixture {
    dir: PathBuf,
    store: Store,
    job: i64,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A job with the thread 5 in its scope (message 1 granted), that waits for
/// a reply on thread 5 until NOW + 3 days.
fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("iris-wake-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    sql(
        &store,
        "INSERT INTO account (id, provider, address) VALUES (1, 'gmail', 'me@example.com');
         INSERT INTO thread (id, account_id, provider_id) VALUES (5, 1, 'a');
         INSERT INTO message (id, account_id, thread_id, provider_id, fetched_at, understood_at)
           VALUES (1, 1, 5, 'm1', 1, 1);
         INSERT INTO header (message_id, from_addr) VALUES (1, 'dana@example.com');
         INSERT INTO label (id, account_id, provider_id, name, kind)
           VALUES (1, 1, 'SENT', 'SENT', 'system'), (2, 1, 'DRAFT', 'DRAFT', 'system');",
    );
    let job = create(&store, "Get the contract.", None, NOW).unwrap();
    add_scope(&store, job, Ref::Thread(5), By::Person, NOW).unwrap();
    register(&store, job, "claude-code", NOW).unwrap();
    let w = Wait {
        thread: Some(5),
        until_ms: NOW + 3 * DAY,
    };
    wait(&store, job, w, None, NOW + 1).unwrap();
    Fixture { dir, store, job }
}

fn sql(store: &Store, text: &str) {
    store.write(|tx| Ok(tx.execute_batch(text)?)).unwrap();
}

/// Adds the message `id` to thread 5, fetched and understood. `extra` is
/// SQL after the insert.
fn message(f: &Fixture, id: i64, extra: &str) {
    sql(
        &f.store,
        &format!(
            "INSERT INTO message (id, account_id, thread_id, provider_id, fetched_at, understood_at)
               VALUES ({id}, 1, 5, 'm{id}', 1, 1);
             INSERT INTO header (message_id, from_addr) VALUES ({id}, 'dana@example.com');
             {extra}"
        ),
    );
}

fn last_why(f: &Fixture) -> Option<String> {
    events(&f.store, f.job).unwrap().pop().unwrap().why
}

#[test]
fn no_wake_before_the_end_with_no_reply() {
    let f = fixture("none");
    // Mail that is not a reply: sent by you, a draft, gone, not understood
    // yet, marked hostile, or queued for the link check.
    message(&f, 2, "INSERT INTO message_label VALUES (2, 1);");
    message(&f, 3, "INSERT INTO message_label VALUES (3, 2);");
    message(&f, 5, "UPDATE message SET gone_at = 1 WHERE id = 5;");
    message(
        &f,
        6,
        "UPDATE message SET understood_at = NULL WHERE id = 6;",
    );
    message(
        &f,
        7,
        r#"UPDATE message SET hostile = '["injection"]' WHERE id = 7;"#,
    );
    message(&f, 8, "INSERT INTO link_check (message_id) VALUES (8);");
    // The source of a signal (it can hold a security code), and mail from a
    // hostile sender: the workspace cannot read them.
    message(
        &f,
        9,
        "INSERT INTO item (id, form, account_id, status, doc, created_at, updated_at)
           VALUES (1, 'signal', 1, 'open', '{}', 1, 1);
         INSERT INTO item_source VALUES (1, 9, 'evidence');",
    );
    message(
        &f,
        10,
        "UPDATE header SET from_addr = 'eve@example.net' WHERE message_id = 10;
         INSERT INTO sender (address, kind) VALUES ('eve@example.net', 'hostile');",
    );
    assert!(wake_due(&f.store, NOW + 3 * DAY - 1).unwrap().is_empty());
    assert_eq!(get(&f.store, f.job).unwrap().state, State::Waiting);
    assert!(woken(&f.store).unwrap().is_empty());
}

#[test]
fn the_end_of_the_wait_wakes_the_job_once() {
    let f = fixture("time");
    assert_eq!(wake_due(&f.store, NOW + 3 * DAY).unwrap(), vec![f.job]);
    assert_eq!(get(&f.store, f.job).unwrap().state, State::Working);
    assert_eq!(last_why(&f).as_deref(), Some("time"));
    assert!(wake_due(&f.store, NOW + 4 * DAY).unwrap().is_empty());
    // The job waits for its session, with the agent of the last session.
    let w = woken(&f.store).unwrap();
    assert_eq!(w, vec![(f.job, NOW + 3 * DAY, "claude-code".to_string())]);
    // The session of the wait (row 1) ended at the wait; after its reap, a
    // new session registers.
    iris_job::reap(&f.store, 1, NOW + 2).unwrap();
    register(&f.store, f.job, "claude-code", NOW + 3 * DAY).unwrap();
    assert!(woken(&f.store).unwrap().is_empty());
}

#[test]
fn a_reply_wakes_the_job_and_only_the_reply_joins_the_grant() {
    let f = fixture("reply");
    // Mail that you sent and a signal on the thread come with the reply.
    message(&f, 2, "INSERT INTO message_label VALUES (2, 1);");
    message(
        &f,
        3,
        "INSERT INTO item (id, form, account_id, status, doc, created_at, updated_at)
           VALUES (1, 'signal', 1, 'open', '{}', 1, 1);
         INSERT INTO item_source VALUES (1, 3, 'evidence');",
    );
    message(&f, 4, "");
    assert_eq!(wake_due(&f.store, NOW + 2).unwrap(), vec![f.job]);
    assert_eq!(last_why(&f).as_deref(), Some("reply"));
    let granted: Vec<i64> = f
        .store
        .read(|c| {
            let mut st = c.prepare("SELECT message_id FROM job_scope_message ORDER BY 1")?;
            let ids = st
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(ids)
        })
        .unwrap();
    assert_eq!(granted, vec![1, 4]);
    // Each message that the grant holds, the workspace can read.
    f.store
        .read(|c| {
            for m in &granted {
                assert!(iris_workspace::readable(c, *m)?, "m-{m}");
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_reply_on_a_revoked_thread_does_not_wake_the_job() {
    let f = fixture("revoked");
    revoke_scope(&f.store, f.job, Ref::Thread(5), By::Person, NOW + 2).unwrap();
    message(&f, 2, "");
    assert!(wake_due(&f.store, NOW + 3).unwrap().is_empty());
    // The end of the wait still wakes it, and the reply stays out.
    assert_eq!(wake_due(&f.store, NOW + 3 * DAY).unwrap(), vec![f.job]);
    assert_eq!(last_why(&f).as_deref(), Some("time"));
}

#[test]
fn a_wake_by_the_person_starts_no_session() {
    let f = fixture("person");
    iris_job::wake(&f.store, f.job, By::Person, None, NOW + 2).unwrap();
    assert!(woken(&f.store).unwrap().is_empty());
}
