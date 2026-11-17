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
/// The sessions whose isolation stop waits until the test removes them.
static SLOW_STOP: Mutex<Vec<i64>> = Mutex::new(Vec::new());

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
        while SLOW_STOP.lock().unwrap().contains(&session) {
            std::thread::sleep(Duration::from_millis(50));
        }
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

/// Waits until `path` is gone, at most 20 s: the engine builds the
/// workspace of a done job on its own thread.
fn gone(path: &Path) {
    let end = Instant::now() + Duration::from_secs(20);
    while path.exists() {
        assert!(Instant::now() < end, "{} stays", path.display());
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn now() -> i64 {
    iris::engine::now_ms()
}

/// Runs supervisor turns until `done` holds for the session, at most 20 s.
fn until(s: &Arc<Sessions>, f: &Fixture, id: i64, done: impl Fn(&Session) -> bool) -> Session {
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
    assert_eq!(s.stop(f.job).unwrap(), (id, Ok(())));
    assert!(t.elapsed() < Duration::from_secs(15));
    let row = session(&f.store, id).unwrap();
    assert_eq!(row.end, Some(EndReason::Stop));
    // After the reap, a new run registers.
    let second = s.run(f.job, "script").unwrap();
    assert!(second > id);
    let _ = s.stop(f.job).unwrap();
}

/// One request on the session socket of `f`: the answer as JSON, or the
/// error of the connect.
fn ask(f: &Fixture, verb: &str, args: serde_json::Value) -> std::io::Result<serde_json::Value> {
    use std::io::{Read, Write};
    let mut c = std::os::unix::net::UnixStream::connect(workspace(f).join(".iris/socket"))?;
    c.set_read_timeout(Some(Duration::from_secs(20)))?;
    let line = serde_json::json!({"v": 1, "verb": verb, "args": args});
    c.write_all(format!("{line}\n").as_bytes())?;
    let mut out = Vec::new();
    c.read_to_end(&mut out)?;
    serde_json::from_slice(&out).map_err(std::io::Error::other)
}

fn the_session_socket_answers_status_and_refuses_other_verbs() {
    let f = fixture("sockstatus", "sleep 30");
    let s = sessions(&f);
    let id = s.run(f.job, "script").unwrap();
    until(&s, &f, id, |r| r.started_ms.is_some());
    let v = ask(&f, "status", serde_json::json!({})).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["job"], format!("j-{}", f.job), "{v}");
    assert_eq!(v["result"]["goal"], "Get the contract.", "{v}");
    assert_eq!(v["result"]["state"], "working", "{v}");
    for verb in ["job.run", "brief", "drafts", "status.all"] {
        let v = ask(&f, verb, serde_json::json!({})).unwrap();
        assert_eq!(v["code"], 3, "{verb}: {v}");
    }
    // The read verbs of the session (R4-04f) answer on the socket.
    let v = ask(&f, "list", serde_json::json!({})).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    // A request after the end in the store gets code 3, before any turn.
    iris_job::end(&f.store, id, EndReason::Stop, None, now()).unwrap();
    let v = ask(&f, "status", serde_json::json!({})).unwrap();
    assert_eq!(v["code"], 3, "{v}");
    until(&s, &f, id, |r| r.reaped_ms.is_some());
    // After the reap, the socket is gone.
    assert!(ask(&f, "status", serde_json::json!({})).is_err());
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
    // A stop waits 15 s, then gives the reason of the reap check.
    let (stopped_id, left) = s.stop(f.job).unwrap();
    assert_eq!(stopped_id, id);
    let why = left.unwrap_err();
    assert!(why.contains("group"), "{why}");
    kill(&format!("-{group}"));
    until(&s, &f, id, |r| r.reaped_ms.is_some());
}

fn the_engine_start_ends_each_open_session_of_the_last_engine() {
    let f = fixture("restart", "true");
    let id = iris_job::register(&f.store, f.job, "script", NOW).unwrap();
    let s = sessions(&f);
    let row = session(&f.store, id).unwrap();
    assert_eq!(row.end, Some(EndReason::Engine));
    // No helper and no group: the reap check passes on the reap thread of
    // the next turn.
    until(&s, &f, id, |r| r.reaped_ms.is_some());
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
    gone(&marker);
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
    // Other old files in `sessions/` stay: only the session logs go.
    let old_egress = logs.join("se-92.egress.log");
    let other = logs.join("recovery-state");
    let near = logs.join("se-93.log.bak");
    for path in [&old, &old_egress, &other, &near] {
        let file = fs::File::create(path).unwrap();
        file.set_modified(std::time::SystemTime::now() - Duration::from_secs(31 * 86_400))
            .unwrap();
    }
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
    gone(&marker);
    assert!(workspace(&f).join("JOB.md").exists());
    assert!(!old.exists(), "a log older than 30 days goes");
    assert!(
        !old_egress.exists(),
        "an egress log older than 30 days goes"
    );
    assert!(
        other.exists(),
        "an old file that is not a session log stays"
    );
    assert!(near.exists(), "an old file that is not a session log stays");
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
    let end = Instant::now() + Duration::from_secs(20);
    while !stopped(id) {
        assert!(
            Instant::now() < end,
            "the isolation stop comes before the reap"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
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
    // The socket of the running session names the second session.
    let v = ask(&f, "status", serde_json::json!({})).unwrap();
    assert_eq!(v["result"]["session"], format!("se-{second}"), "{v}");
    let log = |id: i64| f.store.dir().join(format!("sessions/se-{id}.egress.log"));
    assert!(
        log(second).exists(),
        "the running session has its egress log"
    );
    assert!(
        !log(first).exists(),
        "no log has the name of the first session"
    );
    let _ = s.stop(f.job).unwrap();
}

fn a_slow_rebuild_does_not_hold_a_stop_of_another_job() {
    let f = fixture("slow", "sleep 30");
    let other = create(&f.store, "Other.", None, NOW).unwrap();
    add_scope(&f.store, other, Ref::Thread(5), By::Person, NOW).unwrap();
    let s = sessions(&f);
    let a = s.run(f.job, "script").unwrap();
    until(&s, &f, a, |r| r.started_ms.is_some());
    let b = s.run(other, "script").unwrap();
    until(&s, &f, b, |r| r.started_ms.is_some());
    // The test holds the build lock of the workspace of job A: the
    // rebuild of the done job waits for it.
    let marker = workspace(&f).join(".iris/marker");
    fs::write(&marker, "x").unwrap();
    let lock = f.store.dir().join(format!("workspaces/.j-{}.lock", f.job));
    let held = iris_store::FileLock::take(&lock).unwrap();
    iris_job::finish(
        &f.store,
        f.job,
        By::Person,
        iris_job::End::Cancel,
        None,
        now(),
    )
    .unwrap();
    // The turns of this stop reap A, which asks for its rebuild.
    let (tx, rx) = std::sync::mpsc::channel();
    {
        let s = s.clone();
        std::thread::spawn(move || {
            let _ = tx.send(s.stop(other));
        });
    }
    let stopped = rx.recv_timeout(Duration::from_secs(20));
    let a_reaped = session(&f.store, a).unwrap().reaped_ms.is_some();
    let built_early = !marker.exists();
    drop(held);
    assert_eq!(stopped.expect("the stop came back").unwrap(), (b, Ok(())));
    assert!(a_reaped, "a turn reaped A");
    assert!(!built_early, "the rebuild waits for the lock");
    // After the lock, the rebuild of A comes.
    let end = Instant::now() + Duration::from_secs(20);
    while marker.exists() {
        assert!(Instant::now() < end, "no rebuild of A");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn the_time_limit_of_the_run_holds_after_a_config_change() {
    let f = fixture_with("limitread", "sleep 30", "[session]\nmax_minutes = 1\n");
    let s = sessions(&f);
    // The owner breaks agents.toml after the register.
    let data = f.store.dir().to_path_buf();
    s.set_after(Arc::new(move |step, _| {
        if step == Step::Registered {
            fs::write(data.join("agents.toml"), "not [ toml").unwrap();
        }
    }));
    let id = s.run(f.job, "script").unwrap();
    until(&s, &f, id, |r| r.started_ms.is_some());
    s.turn(now() + 2 * 60_000);
    assert_eq!(
        session(&f.store, id).unwrap().end,
        Some(EndReason::TimeLimit)
    );
    until(&s, &f, id, |r| r.reaped_ms.is_some());
}

fn a_second_run_names_the_live_session() {
    let f = fixture("twice", "sleep 30");
    let s = sessions(&f);
    // The first build waits 1 s: the second run comes during the launch.
    let once = AtomicBool::new(false);
    s.set_after(Arc::new(move |step, _| {
        if step == Step::Built && !once.swap(true, Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(1));
        }
    }));
    let id = s.run(f.job, "script").unwrap();
    let name = format!("se-{id}");
    let e = s.run(f.job, "script").unwrap_err();
    assert!(e.contains(&name), "during the launch: {e}");
    until(&s, &f, id, |r| r.started_ms.is_some());
    let e = s.run(f.job, "script").unwrap_err();
    assert!(e.contains(&name), "after the launch: {e}");
    let _ = s.stop(f.job).unwrap();
}

fn a_stop_during_a_revoke_retry_stops_the_new_session_too() {
    let f = fixture("stopretry", "sleep 30");
    let s = sessions(&f);
    let (revoked_tx, revoked_rx) = std::sync::mpsc::channel::<()>();
    let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
    let (revoked_tx, go_rx) = (Mutex::new(revoked_tx), Mutex::new(go_rx));
    let (store, job, once) = (f.store.clone(), f.job, AtomicBool::new(false));
    // A revoke after the first build. The launch then waits until the stop
    // runs, and only then registers the second session.
    s.set_after(Arc::new(move |step, _| {
        if step == Step::Built && !once.swap(true, Ordering::SeqCst) {
            revoke_scope(&store, job, Ref::Thread(5), By::Person, now()).unwrap();
            revoked_tx.lock().unwrap().send(()).unwrap();
            let _ = go_rx.lock().unwrap().recv_timeout(Duration::from_secs(5));
        }
    }));
    s.run(f.job, "script").unwrap();
    revoked_rx.recv_timeout(Duration::from_secs(20)).unwrap();
    let stopper = {
        let s = s.clone();
        std::thread::spawn(move || s.stop(job))
    };
    std::thread::sleep(Duration::from_millis(500));
    go_tx.send(()).unwrap();
    let (last, left) = stopper.join().unwrap().unwrap();
    assert_eq!(left, Ok(()));
    let row = last_session(&f.store, f.job).unwrap().unwrap();
    assert_eq!(row.id, last, "the stop answers for the last session");
    assert!(
        row.reaped_ms.is_some(),
        "no session of the job runs: {row:?}"
    );
}

fn a_failed_launch_that_is_reaped_stops_its_proxy() {
    // A data folder path too long for the session socket: the bind fails,
    // and the launch ends and reaps its session.
    let name = format!("bind{}", "x".repeat(110));
    let f = fixture_with(&name, "sleep 30", "allow_hosts = [\"example.com\"]\n");
    let s = sessions(&f);
    let id = s.run(f.job, "script").unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        s.turn(now());
        let row = session(&f.store, id).unwrap();
        if row.reaped_ms.is_some() && s.open_proxies() == 0 {
            break;
        }
        assert!(
            Instant::now() < end,
            "{} proxies run: {row:?}",
            s.open_proxies()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn a_stop_before_the_register_stops_the_launch() {
    let f = fixture("stopfirst", "sleep 30");
    let s = sessions(&f);
    // The test holds the store write lock: the register of the run waits.
    let (locked_tx, locked_rx) = std::sync::mpsc::channel::<()>();
    let holder = {
        let store = f.store.clone();
        std::thread::spawn(move || {
            store
                .write(|_| {
                    locked_tx.send(()).unwrap();
                    std::thread::sleep(Duration::from_millis(1500));
                    Ok(())
                })
                .unwrap();
        })
    };
    locked_rx.recv().unwrap();
    let runner = {
        let (s, job) = (s.clone(), f.job);
        std::thread::spawn(move || s.run(job, "script"))
    };
    std::thread::sleep(Duration::from_millis(300));
    let (last, left) = s.stop(f.job).unwrap();
    holder.join().unwrap();
    let id = runner.join().unwrap().unwrap();
    assert_eq!((last, left), (id, Ok(())));
    let row = session(&f.store, id).unwrap();
    assert_eq!(row.end, Some(EndReason::Stop));
    assert!(row.reaped_ms.is_some(), "{row:?}");
}

/// Issue #517: a stop whose end write fails still stops the launch. The
/// gate reads the stop mark, so no agent starts.
fn a_stop_with_a_failed_end_write_starts_no_agent() {
    let f = fixture("stopnoend", "sleep 30");
    let s = sessions(&f);
    let no_end = "CREATE TRIGGER no_end BEFORE UPDATE OF ended_at ON job_session
                  BEGIN SELECT RAISE(ABORT, 'no end'); END;";
    f.store
        .write(|tx| {
            tx.execute_batch(no_end)?;
            Ok(())
        })
        .unwrap();
    let stopped = Arc::new(Mutex::new(None));
    let (store, job, me, seen) = (f.store.clone(), f.job, s.clone(), stopped.clone());
    s.set_after(Arc::new(move |step, _| match step {
        // The stop comes during the launch, and its end write fails.
        Step::Bound => *seen.lock().unwrap() = Some(me.stop(job).is_err()),
        Step::HelperStarted => store
            .write(|tx| {
                tx.execute_batch("DROP TRIGGER no_end;")?;
                Ok(())
            })
            .unwrap(),
        _ => {}
    }));
    let id = s.run(f.job, "script").unwrap();
    let row = until(&s, &f, id, |r| {
        r.reaped_ms.is_some() || r.started_ms.is_some()
    });
    assert_eq!(
        *stopped.lock().unwrap(),
        Some(true),
        "the end write did not fail"
    );
    assert_eq!(
        row.started_ms, None,
        "the agent started after a stop: {row:?}"
    );
    assert_eq!(row.end, Some(EndReason::Stop), "{row:?}");
}

/// Issue #517, when no end write works until the launch is over: no
/// agent starts, the session stays open, and the supervisor ends it with
/// `launch_failed` when the store takes the end.
fn a_stop_with_no_end_write_leaves_the_session_to_the_supervisor() {
    let f = fixture("stopnowrite", "sleep 30");
    let s = sessions(&f);
    let no_end = "CREATE TRIGGER no_end BEFORE UPDATE OF ended_at ON job_session
                  BEGIN SELECT RAISE(ABORT, 'no end'); END;";
    f.store
        .write(|tx| {
            tx.execute_batch(no_end)?;
            Ok(())
        })
        .unwrap();
    let (job, me) = (f.job, s.clone());
    s.set_after(Arc::new(move |step, _| {
        if step == Step::Bound {
            assert!(me.stop(job).is_err(), "the end write did not fail");
        }
    }));
    let id = s.run(f.job, "script").unwrap();
    // The launch writes its end before it waits for the helper: when the
    // helper is gone, no end write of the launch comes.
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let pid = session(&f.store, id).unwrap().helper_pid;
        let gone = pid.is_some_and(|p| {
            !Process::new("kill")
                .args(["-0", &p.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        });
        if gone {
            break;
        }
        assert!(Instant::now() < end, "the helper did not stop");
        std::thread::sleep(Duration::from_millis(100));
    }
    let row = session(&f.store, id).unwrap();
    assert_eq!((row.started_ms, row.end), (None, None), "{row:?}");
    f.store
        .write(|tx| {
            tx.execute_batch("DROP TRIGGER no_end;")?;
            Ok(())
        })
        .unwrap();
    let row = until(&s, &f, id, |r| r.reaped_ms.is_some());
    assert_eq!(row.started_ms, None, "{row:?}");
    assert_eq!(row.end, Some(EndReason::LaunchFailed), "{row:?}");
}

fn a_job_done_during_the_launch_gets_its_workspace_again() {
    let f = fixture("donelaunch", "sleep 30");
    let s = sessions(&f);
    let marker = workspace(&f).join(".iris/marker");
    // After the first build: a file of that build, then the job moves to
    // done. The launch ends and reaps its session in its cleanup.
    let (store, job, m, once) = (
        f.store.clone(),
        f.job,
        marker.clone(),
        AtomicBool::new(false),
    );
    s.set_after(Arc::new(move |step, _| {
        if step == Step::Built && !once.swap(true, Ordering::SeqCst) {
            fs::write(&m, "x").unwrap();
            iris_job::finish(&store, job, By::Person, iris_job::End::Cancel, None, now()).unwrap();
        }
    }));
    let id = s.run(f.job, "script").unwrap();
    until(&s, &f, id, |r| r.reaped_ms.is_some());
    gone(&marker);
}

fn a_slow_isolation_stop_does_not_hold_a_stop_of_another_job() {
    let f = fixture("slowiso", "sleep 30");
    let other = create(&f.store, "Other.", None, NOW).unwrap();
    add_scope(&f.store, other, Ref::Thread(5), By::Person, NOW).unwrap();
    let s = sessions(&f);
    let a = s.run(f.job, "script").unwrap();
    until(&s, &f, a, |r| r.started_ms.is_some());
    let b = s.run(other, "script").unwrap();
    until(&s, &f, b, |r| r.started_ms.is_some());
    // The isolation stop of A waits 25 s.
    SLOW_STOP.lock().unwrap().push(a);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(25));
        SLOW_STOP.lock().unwrap().retain(|x| *x != a);
    });
    let stop_a = {
        let (s, job) = (s.clone(), f.job);
        std::thread::spawn(move || s.stop(job))
    };
    while !stopped(a) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let t = Instant::now();
    let result = s.stop(other).unwrap();
    let took = t.elapsed();
    assert!(
        took < Duration::from_secs(15),
        "the stop of B took {took:?}"
    );
    assert_eq!(result, (b, Ok(())));
    let _ = stop_a.join().unwrap();
    until(&s, &f, a, |r| r.reaped_ms.is_some());
}

fn the_end_of_a_session_closes_its_proxy_before_the_reap() {
    // The agent writes its proxy address, and ignores the TERM of the
    // helper: it lives about 5 s after the end.
    let f = fixture_with(
        "egressend",
        "trap '' TERM; echo $HTTPS_PROXY > proxy-url; sleep 30",
        "allow_hosts = [\"example.com\"]\n",
    );
    let s = sessions(&f);
    let id = s.run(f.job, "script").unwrap();
    until(&s, &f, id, |r| r.started_ms.is_some());
    let url = workspace(&f).join("proxy-url");
    let end = Instant::now() + Duration::from_secs(20);
    while !fs::read_to_string(&url).is_ok_and(|t| t.ends_with('\n')) {
        assert!(Instant::now() < end, "no proxy address");
        std::thread::sleep(Duration::from_millis(50));
    }
    let text = fs::read_to_string(&url).unwrap();
    let port: u16 = text.trim().rsplit(':').next().unwrap().parse().unwrap();
    // The test connects from 127.0.0.1, not from the peer address of the
    // session: the proxy refuses it, and its log says why.
    let log = f.store.dir().join(format!("sessions/se-{id}.egress.log"));
    let lines = |word: &str| {
        fs::read_to_string(&log)
            .unwrap_or_default()
            .matches(word)
            .count()
    };
    let knock = |word: &str| {
        let before = lines(word);
        let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the port is bound");
        let _ = std::io::Read::read(&mut c, &mut [0u8; 64]);
        let end = Instant::now() + Duration::from_secs(4);
        while lines(word) == before {
            assert!(Instant::now() < end, "no `{word}` line in the egress log");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    knock("not_session");
    iris_job::end(&f.store, id, EndReason::Stop, None, now()).unwrap();
    s.turn(now());
    // The proxy is closed, and its port stays bound until the reap.
    knock("reason=stopped");
    assert_eq!(
        session(&f.store, id).unwrap().reaped_ms,
        None,
        "the agent still runs: the proxy closed before the reap"
    );
    until(&s, &f, id, |r| r.reaped_ms.is_some());
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
            "the_session_socket_answers_status_and_refuses_other_verbs",
            the_session_socket_answers_status_and_refuses_other_verbs,
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
        (
            "a_slow_rebuild_does_not_hold_a_stop_of_another_job",
            a_slow_rebuild_does_not_hold_a_stop_of_another_job,
        ),
        (
            "the_time_limit_of_the_run_holds_after_a_config_change",
            the_time_limit_of_the_run_holds_after_a_config_change,
        ),
        (
            "a_second_run_names_the_live_session",
            a_second_run_names_the_live_session,
        ),
        (
            "a_stop_during_a_revoke_retry_stops_the_new_session_too",
            a_stop_during_a_revoke_retry_stops_the_new_session_too,
        ),
        (
            "a_failed_launch_that_is_reaped_stops_its_proxy",
            a_failed_launch_that_is_reaped_stops_its_proxy,
        ),
        (
            "a_stop_before_the_register_stops_the_launch",
            a_stop_before_the_register_stops_the_launch,
        ),
        (
            "a_stop_with_a_failed_end_write_starts_no_agent",
            a_stop_with_a_failed_end_write_starts_no_agent,
        ),
        (
            "a_stop_with_no_end_write_leaves_the_session_to_the_supervisor",
            a_stop_with_no_end_write_leaves_the_session_to_the_supervisor,
        ),
        (
            "a_job_done_during_the_launch_gets_its_workspace_again",
            a_job_done_during_the_launch_gets_its_workspace_again,
        ),
        (
            "a_slow_isolation_stop_does_not_hold_a_stop_of_another_job",
            a_slow_isolation_stop_does_not_hold_a_stop_of_another_job,
        ),
        (
            "the_end_of_a_session_closes_its_proxy_before_the_reap",
            the_end_of_a_session_closes_its_proxy_before_the_reap,
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
