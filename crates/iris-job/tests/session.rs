//! The sessions of a job (migration 21, docs/specs/2026-11-17-session-launcher.md).

use std::fs;
use std::path::PathBuf;

use iris_job::{
    add_scope, create, end, finish, format_session_id, live_sessions, open_session,
    parse_session_id, reap, register, revoke_scope, session, set_agent, set_helper, start, wait,
    By, End, EndReason, Error, Ref, Wait,
};
use iris_store::Store;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-session-{name}-{}", std::process::id()));
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

fn refused<T: std::fmt::Debug>(r: Result<T, Error>) {
    assert!(matches!(r, Err(Error::Refused(_))), "{r:?}");
}

fn sql(store: &Store, text: &str) -> bool {
    store.write(|tx| Ok(tx.execute_batch(text)?)).is_ok()
}

fn setup(name: &str) -> (TempDir, Store, i64) {
    let dir = TempDir::new(name);
    let store = Store::open(&dir.0).unwrap();
    let job = create(&store, "Get the contract.", None, NOW).unwrap();
    (dir, store, job)
}

#[test]
fn a_session_id_is_se_and_the_row() {
    assert_eq!(format_session_id(7), "se-7");
    assert_eq!(parse_session_id("se-7"), Some(7));
    for bad in [
        "se-",
        "se-0",
        "se-07",
        "se--1",
        "j-7",
        "se-7x",
        "se-99999999999999999999",
    ] {
        assert_eq!(parse_session_id(bad), None, "{bad}");
    }
}

#[test]
fn one_live_session_for_each_job() {
    let (_d, store, job) = setup("live");
    let s = register(&store, job, "claude-code", NOW).unwrap();
    // A second register fails while the first session is live: open, then
    // ended but not reaped.
    refused(register(&store, job, "codex", NOW));
    set_helper(&store, s, 4242).unwrap();
    set_agent(&store, s, 4243).unwrap();
    assert!(start(&store, s, NOW + 1).unwrap());
    refused(register(&store, job, "codex", NOW));
    assert!(end(&store, s, EndReason::Exit, Some(0), NOW + 2).unwrap());
    refused(register(&store, job, "codex", NOW));
    assert_eq!(live_sessions(&store).unwrap().len(), 1);
    reap(&store, s, NOW + 3).unwrap();
    assert!(live_sessions(&store).unwrap().is_empty());
    let got = session(&store, s).unwrap();
    assert_eq!(got.job_id, job);
    assert_eq!(got.agent, "claude-code");
    assert_eq!(got.started_ms, Some(NOW + 1));
    assert_eq!(got.ended_ms, Some(NOW + 2));
    assert_eq!(got.end, Some(EndReason::Exit));
    assert_eq!(got.exit_code, Some(0));
    assert_eq!(got.helper_pid, Some(4242));
    assert_eq!(got.agent_pgid, Some(4243));
    assert_eq!(got.reaped_ms, Some(NOW + 3));
    // After the reap, a new session can start.
    let s2 = register(&store, job, "codex", NOW + 4).unwrap();
    assert!(s2 > s);
    assert_eq!(open_session(&store, job).unwrap().map(|x| x.id), Some(s2));
}

#[test]
fn a_session_starts_only_for_a_working_job_and_a_good_agent_name() {
    let (_d, store, job) = setup("register");
    for bad in ["", "Claude", "a b", "a_b", "é", &"a".repeat(65)] {
        refused(register(&store, job, bad, NOW));
    }
    register(&store, job, &"a".repeat(64), NOW)
        .map(|_| ())
        .unwrap();
    let (_d2, store2, job2) = setup("register-wait");
    wait(
        &store2,
        job2,
        Wait {
            thread: None,
            until_ms: NOW + 10,
        },
        None,
        NOW,
    )
    .unwrap();
    refused(register(&store2, job2, "codex", NOW));
    assert!(matches!(
        register(&store2, 99, "codex", NOW),
        Err(Error::NotFound)
    ));
}

#[test]
fn a_revoke_ends_the_open_session_of_its_job() {
    let (_d, store, job) = setup("revoke");
    assert!(sql(
        &store,
        r#"INSERT INTO account (id, provider, address) VALUES (1, 'gmail', 'me@example.com');
           INSERT INTO thread (id, account_id, provider_id) VALUES (5, 1, 'a');"#
    ));
    add_scope(&store, job, Ref::Thread(5), By::Person, NOW).unwrap();
    let s = register(&store, job, "codex", NOW).unwrap();
    revoke_scope(&store, job, Ref::Thread(5), By::Person, NOW + 5).unwrap();
    let got = session(&store, s).unwrap();
    assert_eq!(got.end, Some(EndReason::Revoke));
    assert_eq!(got.ended_ms, Some(NOW + 5));
    // An ended session does not start, and does not end again.
    set_helper(&store, s, 100).unwrap();
    assert!(!start(&store, s, NOW + 6).unwrap());
    assert!(!end(&store, s, EndReason::Stop, None, NOW + 7).unwrap());
    assert_eq!(session(&store, s).unwrap().end, Some(EndReason::Revoke));
    assert_eq!(session(&store, s).unwrap().started_ms, None);
}

#[test]
fn a_job_move_ends_the_open_session_with_job_state() {
    let (_d, store, job) = setup("move");
    let s = register(&store, job, "codex", NOW).unwrap();
    finish(&store, job, By::Person, End::Cancel, None, NOW + 9).unwrap();
    let got = session(&store, s).unwrap();
    assert_eq!(got.end, Some(EndReason::JobState));
    assert_eq!(got.ended_ms, Some(NOW + 9));
    // A reaped session of another job stays as it was.
    let other = create(&store, "Pay the bill.", None, NOW).unwrap();
    let t = register(&store, other, "codex", NOW).unwrap();
    end(&store, t, EndReason::Stop, None, NOW + 1).unwrap();
    reap(&store, t, NOW + 2).unwrap();
    wait(
        &store,
        other,
        Wait {
            thread: None,
            until_ms: NOW + 99,
        },
        None,
        NOW + 3,
    )
    .unwrap();
    assert_eq!(session(&store, t).unwrap().end, Some(EndReason::Stop));
}

#[test]
fn the_process_ids_are_set_once_and_never_zero_or_one() {
    let (_d, store, job) = setup("pids");
    let s = register(&store, job, "codex", NOW).unwrap();
    // The group of the agent needs the helper first.
    refused(set_agent(&store, s, 500));
    for bad in [i64::MIN, -1, 0, 1, 1 << 31] {
        refused(set_helper(&store, s, bad));
    }
    set_helper(&store, s, 400).unwrap();
    refused(set_helper(&store, s, 401));
    for bad in [-1, 0, 1] {
        refused(set_agent(&store, s, bad));
    }
    set_agent(&store, s, 500).unwrap();
    refused(set_agent(&store, s, 501));
    // An exit code only for `exit`, from 0 to 255.
    refused(end(&store, s, EndReason::Stop, Some(1), NOW));
    refused(end(&store, s, EndReason::Exit, Some(256), NOW));
    refused(end(&store, s, EndReason::Exit, Some(-1), NOW));
    // A reap needs the end.
    refused(reap(&store, s, NOW));
    end(&store, s, EndReason::HelperFailed, None, NOW).unwrap();
    reap(&store, s, NOW + 1).unwrap();
    refused(reap(&store, s, NOW + 2));
    // After the reap no process ID changes.
    let (_d2, store2, job2) = setup("pids-reaped");
    let t = register(&store2, job2, "codex", NOW).unwrap();
    end(&store2, t, EndReason::LaunchFailed, None, NOW).unwrap();
    reap(&store2, t, NOW).unwrap();
    refused(set_helper(&store2, t, 400));
}

#[test]
fn no_insert_replaces_or_skips_a_move_and_no_row_is_deleted() {
    let (_d, store, job) = setup("insert");
    let s = register(&store, job, "codex", NOW).unwrap();
    let ins = |cols: &str, vals: &str| {
        format!("INSERT INTO job_session (job_id, agent, registered_at{cols}) VALUES ({job}, 'x', 1{vals})")
    };
    // Each value that a move sets is NULL at the insert. The first session
    // is reaped, so only the column under test can refuse the insert.
    end(&store, s, EndReason::Stop, None, NOW).unwrap();
    reap(&store, s, NOW).unwrap();
    for col in [
        "helper_pid",
        "agent_pgid",
        "started_at",
        "ended_at",
        "exit_code",
        "reaped_at",
    ] {
        assert!(!sql(&store, &ins(&format!(", {col}"), ", 5")), "{col}");
    }
    assert!(!sql(&store, &ins(", end_reason", ", 'stop'")));
    // No insert replaces a row.
    assert!(!sql(
        &store,
        &format!("INSERT OR REPLACE INTO job_session (id, job_id, agent, registered_at) VALUES ({s}, {job}, 'x', 1)")
    ));
    assert_eq!(session(&store, s).unwrap().reaped_ms, Some(NOW));
    assert!(!sql(
        &store,
        &format!("DELETE FROM job_session WHERE id = {s}")
    ));
    // A raw insert for a job with a live session fails too.
    assert!(sql(&store, &ins("", "")));
    assert!(!sql(&store, &ins("", "")));
}

/// The columns that a move can set, in the order of [`Row`].
const COLS: [&str; 7] = [
    "helper_pid",
    "agent_pgid",
    "started_at",
    "ended_at",
    "end_reason",
    "exit_code",
    "reaped_at",
];

/// A row as the SQL text of its seven columns.
type Row = [Option<&'static str>; 7];

/// The five moves of the spec. `true` if `old` to `new` is one of them.
fn allowed(old: &Row, new: &Row) -> bool {
    let changed: Vec<usize> = (0..7).filter(|&i| old[i] != new[i]).collect();
    let set = |i: usize| old[i].is_none() && new[i].is_some();
    match changed.as_slice() {
        [0] => set(0) && old[6].is_none(),
        [1] => set(1) && old[0].is_some() && old[6].is_none(),
        [2] => set(2) && old[3].is_none(),
        [3, 4] | [3, 4, 5] => set(3) && set(4) && (changed.len() == 2 || set(5)),
        [6] => set(6) && old[3].is_some(),
        _ => false,
    }
}

/// The CHECKs of the table, apart from the moves.
fn valid(r: &Row) -> bool {
    (r[3].is_none() == r[4].is_none())
        && (r[5].is_none() || r[4] == Some("'exit'"))
        && (r[6].is_none() || r[3].is_some())
        && (r[1].is_none() || r[0].is_some())
}

/// Each raw update of the move columns succeeds only for one of the five
/// moves. A random walk over the values of each column checks the trigger
/// against the model of the spec.
#[test]
fn a_raw_update_succeeds_only_for_one_of_the_five_moves() {
    let values: [&[Option<&str>]; 7] = [
        &[None, Some("40"), Some("41")],
        &[None, Some("50"), Some("51")],
        &[None, Some("7"), Some("8")],
        &[None, Some("9"), Some("10")],
        &[None, Some("'exit'"), Some("'stop'")],
        &[None, Some("0"), Some("3")],
        &[None, Some("11"), Some("12")],
    ];
    let (_d, store, _) = setup("model");
    let mut seed: u64 = 0x5eed;
    let mut next = |n: usize| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as usize) % n
    };
    let mut checked = 0;
    for _ in 0..60 {
        // A new job and session for each walk, from the insert state.
        let job = create(&store, "Walk.", None, NOW).unwrap();
        let s = register(&store, job, "codex", NOW).unwrap();
        let mut row: Row = [None; 7];
        for _ in 0..12 {
            let mut new = row;
            // Change one to three columns at once.
            for _ in 0..=next(3) {
                let c = next(7);
                new[c] = values[c][next(values[c].len())];
            }
            if new == row {
                continue;
            }
            let set: Vec<String> = (0..7)
                .map(|i| format!("{} = {}", COLS[i], new[i].unwrap_or("NULL")))
                .collect();
            let ok = sql(
                &store,
                &format!("UPDATE job_session SET {} WHERE id = {s}", set.join(", ")),
            );
            let want = allowed(&row, &new) && valid(&new);
            assert_eq!(ok, want, "{row:?} -> {new:?}");
            checked += 1;
            if ok {
                row = new;
            }
        }
    }
    assert!(checked > 300, "{checked}");
    // The fixed columns never change.
    let job = create(&store, "Fixed.", None, NOW).unwrap();
    let s = register(&store, job, "codex", NOW).unwrap();
    for set in [
        "job_id = 1",
        "agent = 'y'",
        "registered_at = 2",
        "id = 9999",
    ] {
        assert!(
            !sql(
                &store,
                &format!("UPDATE job_session SET {set} WHERE id = {s}")
            ),
            "{set}"
        );
    }
}
