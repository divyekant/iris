//! The launch tests (spec "Launch", R4-04h). This test program runs its own
//! main: with the first argument `session-hold`, it is the hold helper of a
//! session that a test launched, with a plain isolation. So the tests run
//! the real helper as its own process, as the engine does.

use std::cell::Cell;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use iris_job::{add_scope, create, revoke_scope, session, By, EndReason, Ref};
use iris_session::hold;
use iris_session::isolation::{Command, Isolation};
use iris_session::launch::{Launcher, Outcome, Running, Step};
use iris_session::reap::reap_check;
use iris_store::Store;

/// Runs the command with no isolation. For the test scripts only.
struct Plain;

impl Isolation for Plain {
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
    fn command(
        &self,
        _session: i64,
        _workspace: &Path,
        _egress_port: Option<u16>,
        program: &Path,
        args: &[String],
    ) -> Command {
        Command {
            program: program.to_path_buf(),
            args: args.iter().map(OsString::from).collect(),
        }
    }
    fn name(&self, session: i64) -> String {
        format!("plain-{session}")
    }
    fn stop(&self, _session: i64) -> std::io::Result<()> {
        Ok(())
    }
    fn ended(&self, _session: i64) -> std::io::Result<bool> {
        Ok(true)
    }
}

const NOW: i64 = 1_763_283_422_000;

struct Fixture {
    root: PathBuf,
    data: PathBuf,
    store: Store,
    job: i64,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A job in `working` with the threads 5 and 6 in its scope, and the
/// adapter `script` that runs `script` with `/bin/sh`.
fn fixture(name: &str, data_name: &str, script: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!("iris-launch-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let data = root.join(data_name);
    let store = Store::open(&data).unwrap();
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO account (id, provider, address) VALUES (1, 'gmail', 'me@example.com');
                 INSERT INTO thread (id, account_id, provider_id) VALUES (5, 1, 'a');
                 INSERT INTO thread (id, account_id, provider_id) VALUES (6, 1, 'b');",
            )?;
            Ok(())
        })
        .unwrap();
    let job = create(&store, "Get the contract.", None, NOW).unwrap();
    add_scope(&store, job, Ref::Thread(5), By::Person, NOW).unwrap();
    add_scope(&store, job, Ref::Thread(6), By::Person, NOW).unwrap();
    let toml = format!(
        "[agent.script]\ncommand = [\"/bin/sh\", \"-c\", {}]\n",
        toml_string(script)
    );
    fs::write(data.join("agents.toml"), toml).unwrap();
    Fixture {
        root,
        data,
        store,
        job,
    }
}

fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn env(name: &str) -> Option<OsString> {
    (name == "PATH").then(|| "/usr/bin:/bin".into())
}

fn now() -> i64 {
    NOW + 1000
}

/// The real helper: this test program with `session-hold`.
fn real_helper(args: &[OsString]) -> Process {
    let mut c = Process::new(std::env::current_exe().unwrap());
    c.arg("session-hold").args(args);
    c
}

fn launch(
    f: &Fixture,
    helper: &dyn Fn(&[OsString]) -> Process,
    after: &dyn Fn(Step, i64),
) -> Outcome {
    let l = Launcher {
        store: &f.store,
        data_dir: &f.data,
        isolation: &Plain,
        helper,
        egress: &|_, _| Ok(None),
        stop_marks: &Mutex::default(),
        env: &env,
        now: &now,
        after,
    };
    let begun = l.begin(f.job, "script").unwrap();
    l.finish(begun)
}

fn running(o: Outcome) -> Running {
    match o {
        Outcome::Running(r) => r,
        Outcome::Ended { end, why, .. } => panic!("the launch ended: {} ({why})", end.as_str()),
        Outcome::Open { why, .. } => panic!("the session stays open: {why}"),
    }
}

fn ended(o: Outcome) -> (i64, EndReason, String) {
    match o {
        Outcome::Running(r) => panic!("the session {} runs", r.session),
        Outcome::Ended { session, end, why } => (session, end, why),
        Outcome::Open { session, why } => panic!("the session {session} stays open: {why}"),
    }
}

/// What the engine does at the end of a session: close the pipe, read the
/// last status line, reap the helper, end and reap the session.
/// With [`EndReason::Exit`], it waits for the agent to exit first.
fn stop(f: &Fixture, mut r: Running, why: EndReason) -> String {
    let pipe = r.pipe;
    if why != EndReason::Exit {
        drop(pipe);
    }
    let mut status = String::new();
    r.status.read_to_string(&mut status).unwrap();
    r.helper.wait().unwrap();
    let code = status
        .trim()
        .strip_prefix("exited ")
        .and_then(|c| c.parse().ok());
    let code = if why == EndReason::Exit { code } else { None };
    iris_job::end(&f.store, r.session, why, code, now()).unwrap();
    let s = session(&f.store, r.session).unwrap();
    reap_check(&f.store, &Plain, &s, true, now()).unwrap();
    let _ = fs::remove_file(&r.socket_path);
    status.trim().to_string()
}

fn sessions(f: &Fixture) -> Vec<iris_job::Session> {
    f.store
        .read(|c| {
            let mut q = c.prepare("SELECT id FROM job_session ORDER BY id")?;
            let ids = q
                .query_map([], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(ids)
        })
        .unwrap()
        .into_iter()
        .map(|id| session(&f.store, id).unwrap())
        .collect()
}

fn workspace(f: &Fixture) -> PathBuf {
    iris_workspace::default_dir(&f.data, f.job)
}

fn a_script_runs_in_the_workspace_through_the_generic_adapter() {
    let f = fixture(
        "run",
        "data",
        "pwd -P > ran.txt; printf '%s' \"$IRIS_DATA\" > env.txt; exit 4",
    );
    let r = running(launch(&f, &real_helper, &|_, _| {}));
    let s = session(&f.store, r.session).unwrap();
    assert!(s.started_ms.is_some());
    assert_eq!(s.agent_pgid, Some(i64::from(r.pgid)));
    assert_eq!(s.helper_pid, Some(i64::from(r.helper.id())));
    let mode = fs::metadata(&r.socket_path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let status = stop(&f, r, EndReason::Exit);
    assert_eq!(status, "exited 4");
    let ws = fs::canonicalize(workspace(&f)).unwrap();
    let ran = fs::read_to_string(ws.join("ran.txt")).unwrap();
    assert_eq!(ran.trim(), ws.to_str().unwrap());
    // No variable of Iris reaches the agent.
    assert_eq!(fs::read_to_string(ws.join("env.txt")).unwrap(), "");
    let all = sessions(&f);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].exit_code, Some(4));
    assert!(all[0].reaped_ms.is_some());
    // The session log holds status lines only.
    let log = fs::read_to_string(
        f.data
            .join("sessions")
            .join(format!("se-{}.log", all[0].id)),
    )
    .unwrap();
    assert!(log.starts_with("started, process group"), "{log}");
}

fn a_revoke_during_the_build_starts_the_launch_again_once() {
    let f = fixture("revoke-build", "data", "sleep 30");
    let first = Cell::new(true);
    let r = running(launch(&f, &real_helper, &|step, _| {
        if step == Step::Built && first.replace(false) {
            revoke_scope(&f.store, f.job, Ref::Thread(5), By::Person, NOW + 1).unwrap();
        }
    }));
    let all = sessions(&f);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].end, Some(EndReason::Revoke));
    assert_eq!(all[0].started_ms, None);
    assert!(
        all[0].reaped_ms.is_some(),
        "the first session is reaped before the second register"
    );
    assert_eq!(r.session, all[1].id);
    assert_eq!(
        stop(&f, r, EndReason::Stop),
        format!("signaled {}", libc::SIGTERM)
    );
}

fn a_second_revoke_ends_the_launch() {
    let f = fixture("revoke-twice", "data", "sleep 30");
    let n = Cell::new(0);
    let (s, end, why) = ended(launch(&f, &real_helper, &|step, _| {
        if step == Step::Built {
            let t = [5, 6].get(n.get()).copied();
            n.set(n.get() + 1);
            if let Some(t) = t {
                revoke_scope(&f.store, f.job, Ref::Thread(t), By::Person, NOW + 1).unwrap();
            }
        }
    }));
    assert_eq!(end, EndReason::Revoke, "{why}");
    let all = sessions(&f);
    assert_eq!(all.len(), 2);
    assert_eq!(s, all[1].id);
    for x in &all {
        assert_eq!(x.end, Some(EndReason::Revoke));
        assert_eq!(x.started_ms, None);
        assert!(x.reaped_ms.is_some());
    }
}

fn a_revoke_at_the_gate_gives_the_helper_no_start() {
    let f = fixture("revoke-gate", "data", "touch started.txt; sleep 30");
    let first = Cell::new(true);
    let r = running(launch(&f, &real_helper, &|step, _| {
        if step == Step::HelperStarted && first.replace(false) {
            revoke_scope(&f.store, f.job, Ref::Thread(5), By::Person, NOW + 1).unwrap();
        }
    }));
    let all = sessions(&f);
    assert_eq!(all[0].end, Some(EndReason::Revoke));
    assert_eq!(all[0].started_ms, None);
    assert_eq!(all[0].agent_pgid, None);
    assert!(all[0].helper_pid.is_some());
    assert!(all[0].reaped_ms.is_some());
    let log = fs::read_to_string(
        f.data
            .join("sessions")
            .join(format!("se-{}.log", all[0].id)),
    )
    .unwrap();
    assert!(log.starts_with("no start"), "{log}");
    stop(&f, r, EndReason::Stop);
}

fn a_stop_during_the_launch_cancels_it_with_no_retry() {
    let f = fixture("stop", "data", "touch started.txt; sleep 30");
    let second = Cell::new(None);
    let (s, end, why) = ended(launch(&f, &real_helper, &|step, s| {
        if step == Step::Bound {
            iris_job::end(&f.store, s, EndReason::Stop, None, NOW + 1).unwrap();
            // The first session stays live until the launch returns: a
            // second run is refused at the register.
            second.set(Some(
                iris_job::register(&f.store, f.job, "script", NOW + 2).is_err(),
            ));
        }
    }));
    assert_eq!(end, EndReason::Stop, "{why}");
    assert_eq!(second.get(), Some(true));
    let all = sessions(&f);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, s);
    assert_eq!(all[0].started_ms, None);
    assert!(all[0].reaped_ms.is_some());
    assert!(!workspace(&f).join(".iris/socket").exists());
    assert!(!workspace(&f).join("started.txt").exists());
    // After the reap, a new run registers.
    assert!(iris_job::register(&f.store, f.job, "script", NOW + 3).is_ok());
}

/// A stop mark that comes while the gate waits for the helper either
/// comes before the gate reads it (no start) or after the commit of the
/// start. It never comes between the read and the commit.
fn a_stop_mark_during_the_gate_waits_for_its_commit() {
    let f = fixture("stop-gate", "data", "sleep 30");
    // The real helper, 1 s late: the gate waits for `started` with the
    // store write lock held.
    let slow = |args: &[OsString]| {
        let mut c = Process::new("/bin/sh");
        c.args(["-c", "sleep 1; exec \"$0\" \"$@\""])
            .arg(std::env::current_exe().unwrap())
            .arg("session-hold")
            .args(args);
        c
    };
    let marks: Mutex<HashSet<i64>> = Mutex::default();
    // `Some(true)`: the mark came while the start was not committed.
    let early = Mutex::new(None);
    let outcome = std::thread::scope(|sc| {
        let after = |step: Step, session: i64| {
            if step != Step::HelperStarted {
                return;
            }
            let (f, marks, early) = (&f, &marks, &early);
            sc.spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                let mut m = marks.lock().unwrap();
                let started = session_started(f, session);
                m.insert(f.job);
                *early.lock().unwrap() = Some(!started);
            });
        };
        let l = Launcher {
            store: &f.store,
            data_dir: &f.data,
            isolation: &Plain,
            helper: &slow,
            egress: &|_, _| Ok(None),
            stop_marks: &marks,
            env: &env,
            now: &now,
            after: &after,
        };
        let begun = l.begin(f.job, "script").unwrap();
        l.finish(begun)
    });
    let early = early.lock().unwrap().expect("the stop thread did not run");
    match outcome {
        Outcome::Running(r) => {
            assert!(!early, "the agent started after a stop mark");
            stop(&f, r, EndReason::Stop);
        }
        _ => assert!(early, "no start, and no mark before the commit"),
    }
}

fn session_started(f: &Fixture, session: i64) -> bool {
    iris_job::session(&f.store, session)
        .unwrap()
        .started_ms
        .is_some()
}

fn a_late_helper_fails_the_launch() {
    let f = fixture("late", "data", "touch started.txt; sleep 30");
    // This helper reads the start but never confirms it.
    let late = |_: &[OsString]| {
        let mut c = Process::new("/bin/sh");
        c.args(["-c", "read line; sleep 3"]);
        c
    };
    let t = Instant::now();
    let (_, end, why) = ended(launch(&f, &late, &|_, _| {}));
    assert_eq!(end, EndReason::LaunchFailed);
    assert!(why.contains("did not confirm the start"), "{why}");
    assert!(t.elapsed() >= Duration::from_secs(2));
    let all = sessions(&f);
    assert_eq!(all.len(), 1);
    // The gate rolled back: no start and no group in the store.
    assert_eq!(all[0].started_ms, None);
    assert_eq!(all[0].agent_pgid, None);
    assert!(all[0].reaped_ms.is_some());
    assert!(!workspace(&f).join(".iris/socket").exists());
}

fn a_long_socket_path_fails_the_launch() {
    let long = "d".repeat(120);
    let f = fixture("long", &long, "sleep 30");
    let (_, end, why) = ended(launch(&f, &real_helper, &|_, _| {}));
    assert_eq!(end, EndReason::LaunchFailed);
    assert!(why.contains("socket path has"), "{why}");
    let all = sessions(&f);
    assert_eq!(all[0].helper_pid, None);
    assert!(all[0].reaped_ms.is_some());
}

fn a_missing_program_fails_before_a_session_row() {
    let f = fixture("missing", "data", "true");
    fs::write(
        f.data.join("agents.toml"),
        "[agent.script]\ncommand = [\"no-such-iris-tool\"]\n",
    )
    .unwrap();
    let l = Launcher {
        store: &f.store,
        data_dir: &f.data,
        isolation: &Plain,
        helper: &real_helper,
        egress: &|_, _| Ok(None),
        stop_marks: &Mutex::default(),
        env: &env,
        now: &now,
        after: &|_, _| {},
    };
    assert!(l.begin(f.job, "script").is_err());
    assert!(sessions(&f).is_empty());
}

fn the_reap_check_waits_for_the_helper_and_the_group() {
    use std::os::unix::process::CommandExt;
    let f = fixture("reap", "data", "true");
    let s = iris_job::register(&f.store, f.job, "script", NOW).unwrap();
    let mut helper = Process::new("/bin/sleep").arg("30").spawn().unwrap();
    let mut agent = Process::new("/bin/sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap();
    iris_job::set_helper(&f.store, s, i64::from(helper.id())).unwrap();
    iris_job::set_agent(&f.store, s, i64::from(agent.id())).unwrap();
    let get = || session(&f.store, s).unwrap();
    // An open session is never reaped.
    assert!(reap_check(&f.store, &Plain, &get(), false, now()).is_err());
    iris_job::end(&f.store, s, EndReason::Stop, None, now()).unwrap();
    let why = reap_check(&f.store, &Plain, &get(), false, now()).unwrap_err();
    assert!(why.contains("helper"), "{why}");
    helper.kill().unwrap();
    helper.wait().unwrap();
    let why = reap_check(&f.store, &Plain, &get(), false, now()).unwrap_err();
    assert!(why.contains("process group"), "{why}");
    agent.kill().unwrap();
    agent.wait().unwrap();
    reap_check(&f.store, &Plain, &get(), false, now()).unwrap();
    assert!(get().reaped_ms.is_some());
}

/// The launch reports an end only if the store has it. A failed end write
/// gives `Open`, and the row stays open for the engine to end.
fn a_failed_end_write_is_not_reported_as_an_end() {
    let long = "e".repeat(120);
    let f = fixture("end-fails", &long, "sleep 30");
    let o = launch(&f, &real_helper, &|step, _| {
        if step == Step::Built {
            f.store
                .write(|tx| {
                    tx.execute_batch(
                        "CREATE TRIGGER test_no_end BEFORE UPDATE OF ended_at ON job_session
                         BEGIN SELECT RAISE(ABORT, 'test: the disk is full'); END;",
                    )?;
                    Ok(())
                })
                .unwrap();
        }
    });
    let Outcome::Open { session: s, why } = o else {
        panic!("a launch with no end in the store reported an end");
    };
    assert!(why.contains("cannot write the end"), "{why}");
    let row = session(&f.store, s).unwrap();
    assert_eq!(row.ended_ms, None);
    assert_eq!(row.reaped_ms, None);
}

/// One changeset of the job with one review comment. Gives the comment ID.
fn comment(f: &Fixture) -> i64 {
    f.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO changeset (author_kind, author_name, author_session, author_job, created_at)
                 VALUES ('agent', 't', 'se-1', ?1, 0)",
                [iris_job::format_job_id(f.job)],
            )?;
            tx.execute(
                "INSERT INTO changeset_comment (changeset_id, revision, text, key, at_ms)
                 VALUES (?1, 1, 'Keep it.', 'k-0123456789abcdef', 1)",
                [tx.last_insert_rowid()],
            )?;
            Ok(tx.last_insert_rowid())
        })
        .unwrap()
}

fn deliveries(f: &Fixture) -> Vec<(i64, i64)> {
    f.store
        .read(|c| {
            let mut q = c.prepare("SELECT session_id, comment_id FROM comment_delivery")?;
            let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// docs/specs/2026-11-17-review-comments.md, section 5: the gate writes the
/// delivery of the comments that the build read. A launch that fails before
/// the gate commit writes none.
fn the_gate_writes_the_delivery_of_the_comments() {
    let f = fixture("delivery-fail", "data", "sleep 30");
    comment(&f);
    let late = |_: &[OsString]| {
        let mut c = Process::new("/bin/sh");
        c.args(["-c", "read line; sleep 3"]);
        c
    };
    ended(launch(&f, &late, &|_, _| {}));
    assert!(workspace(&f).join("comments").exists());
    assert_eq!(deliveries(&f), []);

    let f = fixture("delivery", "data", "sleep 30");
    let id = comment(&f);
    let r = running(launch(&f, &real_helper, &|_, _| {}));
    assert_eq!(deliveries(&f), [(r.session, id)]);
    stop(&f, r, EndReason::Stop);
}

fn main() {
    let args: Vec<OsString> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "session-hold") {
        std::process::exit(hold::hold_main(&Plain, &args[2..]));
    }
    let filter = args
        .iter()
        .skip(1)
        .filter_map(|a| a.to_str())
        .find(|a| !a.starts_with('-'));
    let tests: &[(&str, fn())] = &[
        (
            "a_script_runs_in_the_workspace_through_the_generic_adapter",
            a_script_runs_in_the_workspace_through_the_generic_adapter,
        ),
        (
            "a_revoke_during_the_build_starts_the_launch_again_once",
            a_revoke_during_the_build_starts_the_launch_again_once,
        ),
        (
            "a_second_revoke_ends_the_launch",
            a_second_revoke_ends_the_launch,
        ),
        (
            "a_revoke_at_the_gate_gives_the_helper_no_start",
            a_revoke_at_the_gate_gives_the_helper_no_start,
        ),
        (
            "a_stop_during_the_launch_cancels_it_with_no_retry",
            a_stop_during_the_launch_cancels_it_with_no_retry,
        ),
        (
            "a_stop_mark_during_the_gate_waits_for_its_commit",
            a_stop_mark_during_the_gate_waits_for_its_commit,
        ),
        (
            "a_late_helper_fails_the_launch",
            a_late_helper_fails_the_launch,
        ),
        (
            "a_long_socket_path_fails_the_launch",
            a_long_socket_path_fails_the_launch,
        ),
        (
            "the_reap_check_waits_for_the_helper_and_the_group",
            the_reap_check_waits_for_the_helper_and_the_group,
        ),
        (
            "a_failed_end_write_is_not_reported_as_an_end",
            a_failed_end_write_is_not_reported_as_an_end,
        ),
        (
            "a_missing_program_fails_before_a_session_row",
            a_missing_program_fails_before_a_session_row,
        ),
        (
            "the_gate_writes_the_delivery_of_the_comments",
            the_gate_writes_the_delivery_of_the_comments,
        ),
    ];
    let mut failed = 0;
    let mut ran = 0;
    for (name, test) in tests {
        if filter.is_some_and(|f| !name.contains(f)) {
            continue;
        }
        ran += 1;
        let ok = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        failed += usize::from(!ok);
    }
    println!(
        "test result: {}. {} passed; {failed} failed",
        if failed == 0 { "ok" } else { "FAILED" },
        ran - failed
    );
    std::process::exit(i32::from(failed > 0));
}
