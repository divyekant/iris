//! The sessions of the engine (R4-04e): the supervisor, `job.run` and
//! `job.stop`. This test program runs its own main: with the first argument
//! `session-hold` it is the hold helper, with a plain isolation, so each
//! test runs the real helper as its own process.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iris::sessions::{last_session, state, HelperCommand, Sessions};
use iris_changeset::{Author, AuthorKind};
use iris_job::{add_scope, create, revoke_scope, session, By, EndReason, Ref, Session};
use iris_session::isolation::{Command, Isolation};
use iris_session::launch::Step;
use iris_store::Store;

/// Runs the command with no isolation. For the test scripts only. It
/// keeps each session that got a stop.
struct Plain;

static STOPPED: Mutex<Vec<i64>> = Mutex::new(Vec::new());

fn stopped(session: i64) -> bool {
    STOPPED.lock().unwrap().contains(&session)
}

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
    fn stop(&self, session: i64) -> std::io::Result<()> {
        STOPPED.lock().unwrap().push(session);
        Ok(())
    }
    fn ended(&self, _session: i64) -> std::io::Result<bool> {
        Ok(true)
    }
}

const NOW: i64 = 1_763_283_422_000;

struct Fixture {
    root: PathBuf,
    store: Arc<Store>,
    job: i64,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture(name: &str, script: &str) -> Fixture {
    fixture_with(name, script, "")
}

/// A fixture with `extra` lines in the agent table of `agents.toml`.
fn fixture_with(name: &str, script: &str, extra: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!("iris-es-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let data = root.join("data");
    let store = Store::open(&data).unwrap();
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO account (id, provider, address) VALUES (1, 'gmail', 'me@example.com');
                 INSERT INTO thread (id, account_id, provider_id) VALUES (5, 1, 'a');",
            )?;
            Ok(())
        })
        .unwrap();
    let job = create(&store, "Get the contract.", None, NOW).unwrap();
    add_scope(&store, job, Ref::Thread(5), By::Person, NOW).unwrap();
    let script = script.replace('\\', "\\\\").replace('"', "\\\"");
    fs::write(
        data.join("agents.toml"),
        format!("[agent.script]\ncommand = [\"/bin/sh\", \"-c\", \"{script}\"]\n{extra}"),
    )
    .unwrap();
    Fixture {
        root,
        store: Arc::new(store),
        job,
    }
}

fn helper() -> Arc<HelperCommand> {
    Arc::new(|args: &[OsString]| {
        let mut c = Process::new(std::env::current_exe().unwrap());
        c.arg("session-hold").arg("unused-data-dir").args(args);
        c
    })
}

fn sessions(f: &Fixture) -> Arc<Sessions> {
    Sessions::new(f.store.clone(), Ok(Arc::new(Plain)), helper()).unwrap()
}

fn workspace(f: &Fixture) -> PathBuf {
    f.store.dir().join(format!("workspaces/j-{}", f.job))
}

fn now() -> i64 {
    iris::engine::now_ms()
}

/// Runs supervisor turns until `done` holds for the session, at most 20 s.
fn until(s: &Sessions, f: &Fixture, id: i64, done: impl Fn(&Session) -> bool) -> Session {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        s.turn(now());
        let row = session(&f.store, id).unwrap();
        if done(&row) {
            return row;
        }
        assert!(Instant::now() < end, "no change: {row:?}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn an_exit_of_the_agent_ends_and_reaps_the_session() {
    let f = fixture("exit", "exit 3");
    let s = sessions(&f);
    let id = s.run(f.job, "script").unwrap();
    let row = until(&s, &f, id, |r| r.reaped_ms.is_some());
    assert_eq!(row.end, Some(EndReason::Exit));
    assert_eq!(row.exit_code, Some(3));
    assert!(row.started_ms.is_some());
    assert_eq!(state(&row), "ended (exit)");
    assert!(!f
        .store
        .dir()
        .join(format!("workspaces/j-{}/.iris/socket", f.job))
        .exists());
}

fn a_stop_ends_the_session_and_waits_for_the_reap() {
    let f = fixture("stop", "sleep 30");
    let s = sessions(&f);
    let id = s.run(f.job, "script").unwrap();
    until(&s, &f, id, |r| r.started_ms.is_some());
    let t = Instant::now();
    assert_eq!(s.stop(f.job).unwrap(), (id, true));
    assert!(t.elapsed() < Duration::from_secs(15));
    let row = session(&f.store, id).unwrap();
    assert_eq!(row.end, Some(EndReason::Stop));
    // After the reap, a new run registers.
    let second = s.run(f.job, "script").unwrap();
    assert!(second > id);
    s.stop(f.job).unwrap();
}

fn a_revoke_stops_a_running_session() {
    let f = fixture("revoke", "sleep 30");
    let s = sessions(&f);
    let id = s.run(f.job, "script").unwrap();
    until(&s, &f, id, |r| r.started_ms.is_some());
    revoke_scope(&f.store, f.job, Ref::Thread(5), By::Person, now()).unwrap();
    let row = until(&s, &f, id, |r| r.reaped_ms.is_some());
    assert_eq!(row.end, Some(EndReason::Revoke));
}

fn the_time_limit_ends_a_session() {
    let f = fixture("limit", "sleep 30");
    let s = sessions(&f);
    let id = s.run(f.job, "script").unwrap();
    until(&s, &f, id, |r| r.started_ms.is_some());
    // A turn 31 minutes later: past the default limit of 30 minutes.
    s.turn(now() + 31 * 60_000);
    assert_eq!(
        session(&f.store, id).unwrap().end,
        Some(EndReason::TimeLimit)
    );
    until(&s, &f, id, |r| r.reaped_ms.is_some());
}

/// `kill -KILL` of a process, or of a group as `-<id>`.
fn kill(target: &str) {
    let ok = Process::new("kill")
        .args(["-KILL", "--", target])
        .status()
        .unwrap()
        .success();
    assert!(ok, "kill {target}");
}

fn a_killed_helper_keeps_the_session_live_until_the_group_is_empty() {
    let f = fixture("killed", "sleep 30");
    let s = sessions(&f);
    let id = s.run(f.job, "script").unwrap();
    let row = until(&s, &f, id, |r| r.agent_pgid.is_some());
    let (helper, group) = (
        row.helper_pid.unwrap() as i32,
        row.agent_pgid.unwrap() as i32,
    );
    kill(&helper.to_string());
    let row = until(&s, &f, id, |r| r.ended_ms.is_some());
    assert_eq!(row.end, Some(EndReason::HelperFailed));
    assert_eq!(row.exit_code, None);
    for _ in 0..5 {
        s.turn(now());
    }
    assert_eq!(
        session(&f.store, id).unwrap().reaped_ms,
        None,
        "the agent still runs"
    );
    kill(&format!("-{group}"));
    until(&s, &f, id, |r| r.reaped_ms.is_some());
}

fn the_engine_start_ends_each_open_session_of_the_last_engine() {
    let f = fixture("restart", "true");
    let id = iris_job::register(&f.store, f.job, "script", NOW).unwrap();
    let s = sessions(&f);
    let row = session(&f.store, id).unwrap();
    assert_eq!(row.end, Some(EndReason::Engine));
    // No helper and no group: the reap check passes on the next turn.
    s.turn(now());
    assert!(session(&f.store, id).unwrap().reaped_ms.is_some());
}

fn with_no_isolation_each_run_is_refused() {
    let f = fixture("noiso", "true");
    let s = Sessions::new(f.store.clone(), Err("no isolation here".into()), helper()).unwrap();
    let e = s.run(f.job, "script").unwrap_err();
    assert!(e.contains("no isolation"), "{e}");
    assert!(last_session(&f.store, f.job).unwrap().is_none());
}

fn a_session_cannot_run_or_stop_a_session() {
    let f = fixture("host", "true");
    let me = Author {
        kind: AuthorKind::Agent,
        name: Some("script".into()),
        session: Some("se-1".into()),
        job: Some(iris_job::format_job_id(f.job)),
    };
    for verb in ["job.run", "job.stop"] {
        let line = format!(
            "{{\"v\":1,\"verb\":\"{verb}\",\"args\":{{\"job\":\"j-{}\",\"agent\":\"script\"}}}}\n",
            f.job
        );
        let mut out = Vec::new();
        iris::socket::answer_as(&f.store, &me, line.as_bytes(), &mut out).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["code"], 3, "{verb}: {v}");
    }
}

fn a_done_job_gets_its_workspace_again_after_the_reap() {
    let f = fixture("done", "sleep 30");
    let s = sessions(&f);
    let id = s.run(f.job, "script").unwrap();
    until(&s, &f, id, |r| r.started_ms.is_some());
    // A file of the last build: the next build removes `.iris`.
    let marker = workspace(&f).join(".iris/marker");
    fs::write(&marker, "x").unwrap();
    iris_job::finish(
        &f.store,
        f.job,
        By::Person,
        iris_job::End::Cancel,
        None,
        now(),
    )
    .unwrap();
    let row = until(&s, &f, id, |r| r.reaped_ms.is_some());
    assert_eq!(row.end, Some(EndReason::JobState));
    assert!(!marker.exists(), "the workspace is built again");
    assert!(workspace(&f).join("JOB.md").exists());
}

fn the_engine_start_builds_each_done_job_again() {
    let f = fixture("donestart", "true");
    let marker = workspace(&f).join(".iris/marker");
    fs::create_dir_all(marker.parent().unwrap()).unwrap();
    fs::write(&marker, "x").unwrap();
    // Session logs: one of 31 days ago goes, one of today stays.
    let logs = f.store.dir().join("sessions");
    fs::create_dir_all(&logs).unwrap();
    let old = logs.join("se-90.log");
    let new = logs.join("se-91.log");
    fs::write(&new, "").unwrap();
    let file = fs::File::create(&old).unwrap();
    file.set_modified(std::time::SystemTime::now() - Duration::from_secs(31 * 86_400))
        .unwrap();
    drop(file);
    iris_job::finish(
        &f.store,
        f.job,
        By::Person,
        iris_job::End::Cancel,
        None,
        NOW,
    )
    .unwrap();
    let _s = sessions(&f);
    assert!(!marker.exists(), "the workspace is built again");
    assert!(workspace(&f).join("JOB.md").exists());
    assert!(!old.exists(), "a log older than 30 days goes");
    assert!(new.exists());
}

fn an_orphan_session_gets_a_stop_while_its_helper_runs() {
    let f = fixture("orphan", "true");
    let id = iris_job::register(&f.store, f.job, "script", NOW).unwrap();
    // A helper of the last engine that still runs.
    let mut helper = Process::new("sleep").arg("30").spawn().unwrap();
    iris_job::set_helper(&f.store, id, i64::from(helper.id())).unwrap();
    let s = sessions(&f);
    s.turn(now());
    assert!(stopped(id), "the isolation stop comes before the reap");
    assert_eq!(session(&f.store, id).unwrap().reaped_ms, None);
    helper.kill().unwrap();
    helper.wait().unwrap();
    until(&s, &f, id, |r| r.reaped_ms.is_some());
}

fn a_revoke_retry_gives_the_egress_log_to_the_new_session() {
    let f = fixture_with("retry", "sleep 30", "allow_hosts = [\"example.com\"]\n");
    let s = sessions(&f);
    // A revoke after the first build: the launch registers a second session.
    let (store, job, once) = (f.store.clone(), f.job, AtomicBool::new(false));
    s.set_after(Arc::new(move |step, _| {
        if step == Step::Built && !once.swap(true, Ordering::SeqCst) {
            revoke_scope(&store, job, Ref::Thread(5), By::Person, now()).unwrap();
        }
    }));
    let first = s.run(f.job, "script").unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    let second = loop {
        s.turn(now());
        let last = last_session(&f.store, f.job).unwrap().unwrap();
        if last.id != first && last.started_ms.is_some() {
            break last.id;
        }
        assert!(Instant::now() < end, "no second session: {last:?}");
        std::thread::sleep(Duration::from_millis(100));
    };
    let log = |id: i64| f.store.dir().join(format!("sessions/se-{id}.egress.log"));
    assert!(
        log(second).exists(),
        "the running session has its egress log"
    );
    assert!(
        !log(first).exists(),
        "no log has the name of the first session"
    );
    s.stop(f.job).unwrap();
}

fn main() {
    let args: Vec<OsString> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "session-hold") {
        std::process::exit(iris_session::hold::hold_main(&Plain, &args[3..]));
    }
    let filter = args
        .iter()
        .skip(1)
        .filter_map(|a| a.to_str())
        .find(|a| !a.starts_with('-'));
    let tests: &[(&str, fn())] = &[
        (
            "an_exit_of_the_agent_ends_and_reaps_the_session",
            an_exit_of_the_agent_ends_and_reaps_the_session,
        ),
        (
            "a_stop_ends_the_session_and_waits_for_the_reap",
            a_stop_ends_the_session_and_waits_for_the_reap,
        ),
        (
            "a_revoke_stops_a_running_session",
            a_revoke_stops_a_running_session,
        ),
        (
            "the_time_limit_ends_a_session",
            the_time_limit_ends_a_session,
        ),
        (
            "a_killed_helper_keeps_the_session_live_until_the_group_is_empty",
            a_killed_helper_keeps_the_session_live_until_the_group_is_empty,
        ),
        (
            "the_engine_start_ends_each_open_session_of_the_last_engine",
            the_engine_start_ends_each_open_session_of_the_last_engine,
        ),
        (
            "with_no_isolation_each_run_is_refused",
            with_no_isolation_each_run_is_refused,
        ),
        (
            "a_session_cannot_run_or_stop_a_session",
            a_session_cannot_run_or_stop_a_session,
        ),
        (
            "a_done_job_gets_its_workspace_again_after_the_reap",
            a_done_job_gets_its_workspace_again_after_the_reap,
        ),
        (
            "the_engine_start_builds_each_done_job_again",
            the_engine_start_builds_each_done_job_again,
        ),
        (
            "an_orphan_session_gets_a_stop_while_its_helper_runs",
            an_orphan_session_gets_a_stop_while_its_helper_runs,
        ),
        (
            "a_revoke_retry_gives_the_egress_log_to_the_new_session",
            a_revoke_retry_gives_the_egress_log_to_the_new_session,
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
