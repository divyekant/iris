//! The sessions of jobs in the engine (docs/specs/2026-11-17-session-launcher.md:
//! "Supervisor", "Stop", "Engine start and stop" and "The command").
//!
//! The engine starts each session on a launch thread, keeps the helper of
//! each running session, and runs one supervisor turn each second. The turn
//! ends a session on an exit of the agent, a failed helper or the time
//! limit, stops a session that ended in the store (a stop, a revoke, a job
//! move), and runs the reap check until it passes.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{self, Child, ChildStderr, ChildStdin};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use iris_job::{format_session_id, EndReason, Session};
use iris_session::egress::{Proxy, SystemResolver};
use iris_session::isolation::Isolation;
use iris_session::launch::{Launcher, Outcome, Step};
use iris_session::reap::reap_check;
use iris_store::Store;

use crate::engine::now_ms;

/// The time between two supervisor turns.
pub const TURN: Duration = Duration::from_secs(1);
/// The longest wait of `job.stop` for the reap.
pub const STOP_WAIT: Duration = Duration::from_secs(15);

/// The command of the hold helper, from its arguments.
pub type HelperCommand = dyn Fn(&[OsString]) -> process::Command + Send + Sync;

/// A call after each step of a launch, with the session. For tests.
pub type AfterStep = dyn Fn(Step, i64) + Send + Sync;

/// The age of a session log that the engine start removes.
const LOG_DAYS: u64 = 30;

/// The sessions of the engine. The engine sets one in [`SESSIONS`] at its
/// start, and the engine socket uses it for `job.run` and `job.stop`.
pub struct Sessions {
    store: Arc<Store>,
    /// The isolation of the system, or the reason that there is none. With
    /// no isolation, each run is refused (spec 2, C2 and C6).
    isolation: Result<Arc<dyn Isolation>, String>,
    helper: Arc<HelperCommand>,
    state: Mutex<State>,
    after: Mutex<Option<Arc<AfterStep>>>,
}

#[derive(Default)]
struct State {
    /// The jobs with a launch thread. Rule 0 of the reap check: the
    /// supervisor does not touch a session of such a job.
    launching: HashSet<i64>,
    /// The sessions whose helper this engine started, by session ID.
    running: HashMap<i64, Live>,
}

struct Live {
    child: Child,
    /// The pipe to the helper. `None` after the shutdown closed it.
    pipe: Option<ChildStdin>,
    status: ChildStderr,
    /// Held open until R4-04i serves it. The shutdown closes it.
    socket: Option<UnixListener>,
    socket_path: PathBuf,
    proxy: Option<Proxy>,
    /// The time limit of the session, in milliseconds.
    limit_ms: i64,
    /// The status line of the helper, after this engine reaped it.
    exit: Option<String>,
}

/// The sessions of this engine.
pub static SESSIONS: OnceLock<Arc<Sessions>> = OnceLock::new();

impl Sessions {
    /// The sessions of the engine of `store`. At the engine start, each
    /// open session belongs to the last engine: it ends with `engine`, and
    /// the supervisor reaps it. Then each done job with no live session
    /// gets its workspace again, and the session logs older than 30 days go.
    pub fn new(
        store: Arc<Store>,
        isolation: Result<Arc<dyn Isolation>, String>,
        helper: Arc<HelperCommand>,
    ) -> Result<Arc<Sessions>, String> {
        let now = now_ms();
        for s in iris_job::live_sessions(&store).map_err(|e| e.to_string())? {
            if s.ended_ms.is_none() {
                iris_job::end(&store, s.id, EndReason::Engine, None, now)
                    .map_err(|e| e.to_string())?;
            }
        }
        let s = Arc::new(Sessions {
            store,
            isolation,
            helper,
            state: Mutex::new(State::default()),
            after: Mutex::new(None),
        });
        let live: HashSet<i64> = iris_job::live_sessions(&s.store)
            .map_err(|e| e.to_string())?
            .iter()
            .map(|l| l.job_id)
            .collect();
        for job in iris_job::list(&s.store, iris_job::State::Done).map_err(|e| e.to_string())? {
            if !live.contains(&job.id) {
                s.build_done(job.id);
            }
        }
        s.remove_old_logs();
        Ok(s)
    }

    /// Calls `f` after each step of each launch. For tests.
    pub fn set_after(&self, f: Arc<AfterStep>) {
        *self.after.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    }

    /// Builds the workspace of `job` again if the job is done and its
    /// workspace folder exists: the mail files go, and the files of the
    /// agent stay. Call it only while the job has no live session.
    fn build_done(&self, job: i64) {
        let dir = iris_workspace::default_dir(self.data_dir(), job);
        let done = iris_job::get(&self.store, job).is_ok_and(|j| j.state == iris_job::State::Done);
        if !done || std::fs::symlink_metadata(&dir).is_err() {
            return;
        }
        if let Err(e) = iris_workspace::build(&self.store, job, &dir) {
            say!(
                "iris: {}: the workspace of the done job was not built again: {e}",
                iris_job::format_job_id(job)
            );
        }
    }

    /// Removes the files of `sessions/` older than [`LOG_DAYS`].
    fn remove_old_logs(&self) {
        let Ok(entries) = std::fs::read_dir(self.data_dir().join("sessions")) else {
            return;
        };
        let age = Duration::from_secs(LOG_DAYS * 86_400);
        for e in entries.flatten() {
            let old = e
                .metadata()
                .ok()
                .filter(|m| m.is_file())
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|a| a > age);
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn data_dir(&self) -> &Path {
        self.store.dir()
    }

    /// `job.run`: steps 1 and 2 of the launch, then the other steps on a
    /// launch thread. Gives the session ID after the register.
    pub fn run(self: &Arc<Self>, job: i64, agent: &str) -> Result<i64, String> {
        let isolation = self.isolation.clone()?;
        {
            let mut st = self.state();
            if !st.launching.insert(job) {
                return Err("a launch of this job runs".into());
            }
        }
        let begun = {
            let launcher = self.launcher(&*isolation, None, &|_, _| {});
            launcher.begin(job, agent)
        };
        let begun = match begun {
            Ok(b) => b,
            Err(e) => {
                self.state().launching.remove(&job);
                return Err(e);
            }
        };
        let session = begun.session;
        let me = self.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("launch-{}", format_session_id(session)))
            .spawn(move || me.launch(&*isolation, begun));
        if let Err(e) = spawned {
            // The session stays open with no launch thread: the next turn
            // ends it with `launch_failed`.
            self.state().launching.remove(&job);
            return Err(format!("the launch thread did not start: {e}"));
        }
        Ok(session)
    }

    fn launcher<'a>(
        &'a self,
        isolation: &'a dyn Isolation,
        egress_port: Option<u16>,
        after: &'a dyn Fn(Step, i64),
    ) -> Launcher<'a> {
        Launcher {
            store: &self.store,
            data_dir: self.data_dir(),
            isolation,
            helper: &*self.helper,
            egress_port,
            env: &|name| std::env::var_os(name),
            now: &now_ms,
            after,
        }
    }

    /// Steps 3 to 6, on the launch thread.
    fn launch(&self, isolation: &dyn Isolation, begun: iris_session::launch::Begun) {
        let job = begun.job();
        let session = begun.session;
        let limit_ms = iris_session::config::read_agents(self.data_dir())
            .map(|f| f.max_minutes)
            .unwrap_or(iris_session::config::DEFAULT_MINUTES);
        let limit_ms = i64::from(limit_ms) * 60_000;
        let proxy = if begun.allow_hosts().is_empty() {
            Ok(None)
        } else {
            self.egress(session, begun.allow_hosts().to_vec()).map(Some)
        };
        let hook = self.after.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let outcome = match proxy {
            Ok(proxy) => {
                let port = proxy.as_ref().map(Proxy::port);
                // A revoke retry registers a new session: the egress log
                // moves to it before its build, so the log of the session
                // that runs always has its name.
                let logged = Cell::new(session);
                let after = |step: Step, s: i64| {
                    if step == Step::Registered && proxy.is_some() && s != logged.get() {
                        match move_log(&self.egress_log(logged.get()), &self.egress_log(s)) {
                            Ok(()) => logged.set(s),
                            Err(e) => {
                                // The launch sees the end and stops.
                                say!("iris: {}: no egress log: {e}", format_session_id(s));
                                let _ = iris_job::end(
                                    &self.store,
                                    s,
                                    EndReason::LaunchFailed,
                                    None,
                                    now_ms(),
                                );
                            }
                        }
                    }
                    if let Some(h) = &hook {
                        h(step, s);
                    }
                };
                let outcome = self.launcher(isolation, port, &after).finish(begun);
                Some((outcome, proxy))
            }
            Err(why) => {
                say!(
                    "iris: {}: no egress port: {why}",
                    format_session_id(session)
                );
                let _ = iris_job::end(
                    &self.store,
                    session,
                    EndReason::LaunchFailed,
                    None,
                    now_ms(),
                );
                None
            }
        };
        let mut st = self.state();
        st.launching.remove(&job);
        match outcome {
            Some((Outcome::Running(r), proxy)) => {
                st.running.insert(
                    r.session,
                    Live {
                        child: r.helper,
                        pipe: Some(r.pipe),
                        status: r.status,
                        socket: Some(r.socket),
                        socket_path: r.socket_path,
                        proxy,
                        limit_ms,
                        exit: None,
                    },
                );
            }
            Some((Outcome::Ended { session, why, .. } | Outcome::Open { session, why }, proxy)) => {
                say!("iris: {}: {why}", format_session_id(session));
                if let Some(p) = proxy {
                    let _ = p.stop();
                }
            }
            None => {}
        }
    }

    /// The egress proxy of a session, with its log.
    fn egress(&self, session: i64, allowed: Vec<String>) -> Result<Proxy, String> {
        let dir = self.data_dir().join("sessions");
        std::fs::DirBuilder::new()
            .recursive(true)
            .create(&dir)
            .map_err(|e| e.to_string())?;
        let log = OpenOptions::new()
            .append(true)
            .create_new(true)
            .mode(0o600)
            .open(self.egress_log(session))
            .map_err(|e| e.to_string())?;
        Proxy::start(allowed, log, Box::new(SystemResolver)).map_err(|e| e.to_string())
    }

    fn egress_log(&self, session: i64) -> PathBuf {
        self.data_dir()
            .join("sessions")
            .join(format!("{}.egress.log", format_session_id(session)))
    }

    /// `job.stop`: ends the open session of `job` with `stop`, then waits
    /// up to [`STOP_WAIT`] for the reap. Gives the session and true if it
    /// was reaped.
    pub fn stop(&self, job: i64) -> Result<(i64, bool), String> {
        let open = iris_job::open_session(&self.store, job).map_err(|e| e.to_string())?;
        let session = match open {
            Some(s) => {
                iris_job::end(&self.store, s.id, EndReason::Stop, None, now_ms())
                    .map_err(|e| e.to_string())?;
                s.id
            }
            None => {
                last_session(&self.store, job)?
                    .ok_or("the job has no session")?
                    .id
            }
        };
        let end = Instant::now() + STOP_WAIT;
        loop {
            self.turn(now_ms());
            let s = iris_job::session(&self.store, session).map_err(|e| e.to_string())?;
            if s.reaped_ms.is_some() {
                return Ok((session, true));
            }
            if Instant::now() >= end {
                return Ok((session, false));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// One supervisor turn over each live session.
    pub fn turn(&self, now: i64) {
        let Ok(live) = iris_job::live_sessions(&self.store) else {
            return;
        };
        let mut st = self.state();
        for s in live {
            if st.launching.contains(&s.job_id) {
                continue;
            }
            let reaped = match st.running.get_mut(&s.id) {
                Some(l) => {
                    let reaped = self.supervise(&s, l, now);
                    if reaped {
                        let l = st.running.remove(&s.id).expect("present");
                        let _ = std::fs::remove_file(&l.socket_path);
                    }
                    reaped
                }
                None => self.orphan(&s, now),
            };
            if reaped {
                self.build_done(s.job_id);
            }
        }
    }

    /// One running session of this engine. Gives true when it is reaped.
    fn supervise(&self, s: &Session, l: &mut Live, now: i64) -> bool {
        if l.exit.is_none() {
            if let Ok(Some(_)) = l.child.try_wait() {
                // The helper exited: its last line is in the pipe.
                let mut line = String::new();
                let _ = (&mut l.status).take(64).read_to_string(&mut line);
                l.exit = Some(line.trim().to_string());
            }
        }
        if let Some(line) = &l.exit {
            if s.ended_ms.is_none() {
                let (why, code) = match line.split_once(' ') {
                    Some(("exited", c)) => (EndReason::Exit, c.parse().ok()),
                    Some(("signaled", _)) => (EndReason::Exit, None),
                    // No line: the helper was killed or failed. The agent
                    // result is not known.
                    _ => (EndReason::HelperFailed, None),
                };
                let _ = iris_job::end(&self.store, s.id, why, code, now);
            }
            self.shutdown(s.id, l);
            return self.reap(s.id, true, now);
        }
        if s.ended_ms.is_none() {
            match s.started_ms {
                Some(start) if now - start >= l.limit_ms => {
                    let _ = iris_job::end(&self.store, s.id, EndReason::TimeLimit, None, now);
                }
                _ => return false,
            }
        }
        // The session ended in the store (a stop, a revoke, a job move, a
        // time limit): the helper stops the group when the pipe closes. The
        // next turns reap the helper.
        self.shutdown(s.id, l);
        false
    }

    /// The shutdown order: the grant ended in the store; close the socket,
    /// close the pipe (the helper stops the group), stop the proxy and the
    /// isolation. Each turn calls it again until the reap.
    fn shutdown(&self, session: i64, l: &mut Live) {
        l.socket = None;
        l.pipe = None;
        if let Some(p) = l.proxy.take() {
            let _ = p.stop();
        }
        if let Ok(iso) = &self.isolation {
            let _ = iso.stop(session);
        }
    }

    /// A live session with no launch thread and no helper of this engine:
    /// a session of the last engine, or a launch that could not write its
    /// end. End it, stop its isolation (on each turn: a process of the
    /// agent can still run), then run the reap check. Gives true when it is
    /// reaped.
    fn orphan(&self, s: &Session, now: i64) -> bool {
        if s.ended_ms.is_none() {
            let _ = iris_job::end(&self.store, s.id, EndReason::LaunchFailed, None, now);
        }
        if let Ok(iso) = &self.isolation {
            let _ = iso.stop(s.id);
        }
        self.reap(s.id, false, now)
    }

    fn reap(&self, session: i64, helper_reaped: bool, now: i64) -> bool {
        let Ok(iso) = &self.isolation else {
            return false;
        };
        let Ok(s) = iris_job::session(&self.store, session) else {
            return false;
        };
        match reap_check(&self.store, &**iso, &s, helper_reaped, now) {
            Ok(()) => true,
            Err(why) => {
                // One line in the engine log for a session still live 10
                // minutes after its end.
                if let Some(end) = s.ended_ms {
                    if now - end >= 600_000 && now - end < 600_000 + 1000 {
                        say!("iris: {} is not reaped: {why}", format_session_id(session));
                    }
                }
                false
            }
        }
    }

    /// The supervisor thread: one turn each [`TURN`].
    pub fn supervise_forever(self: Arc<Self>) {
        loop {
            self.turn(now_ms());
            std::thread::sleep(TURN);
        }
    }
}

/// Moves a log file to a new name, and never over a file of that name.
fn move_log(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::hard_link(from, to)?;
    std::fs::remove_file(from)
}

/// The last session of `job`, if one exists.
pub fn last_session(store: &Store, job: i64) -> Result<Option<Session>, String> {
    let id: Option<i64> = store
        .read(|c| {
            use rusqlite::OptionalExtension;
            Ok(c.query_row(
                "SELECT id FROM job_session WHERE job_id = ?1 ORDER BY id DESC LIMIT 1",
                [job],
                |r| r.get(0),
            )
            .optional()?)
        })
        .map_err(|e| e.to_string())?;
    id.map(|id| iris_job::session(store, id).map_err(|e| e.to_string()))
        .transpose()
}

/// The state of a session for `iris job session`.
pub fn state(s: &Session) -> String {
    match (s.started_ms, s.end, s.reaped_ms) {
        (None, None, _) => "starting".into(),
        (Some(_), None, _) => "running".into(),
        (_, Some(e), None) => format!("ending ({})", e.as_str()),
        (_, Some(e), Some(_)) => format!("ended ({})", e.as_str()),
    }
}
