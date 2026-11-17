//! The sessions of jobs in the engine (docs/specs/2026-11-17-session-launcher.md:
//! "Supervisor", "Stop", "Engine start and stop" and "The command").
//!
//! The engine starts each session on a launch thread, keeps the helper of
//! each running session, and runs one supervisor turn each second. The turn
//! ends a session on an exit of the agent, a failed helper or the time
//! limit, stops a session that ended in the store (a stop, a revoke, a job
//! move), and runs the reap check until it passes.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Read;
use std::net::Ipv4Addr;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{self, Child, ChildStderr, ChildStdin};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use iris_changeset::{Author, AuthorKind};
use iris_job::{format_session_id, EndReason, Session};
use iris_session::config::Adapter;
use iris_session::egress::{new_token, session_peer, Endpoint, Proxy, SystemResolver};
use iris_session::isolation::Isolation;
use iris_session::launch::{Egress, Launcher, Outcome, Step};
use iris_session::reap::reap_check;
use iris_store::Store;

use crate::engine::now_ms;
use crate::session_socket::{Handler, IsOpen, Server, DRAIN_WAIT};

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
    /// The jobs whose stop came during a launch. The gate of the launch
    /// holds this lock from its read of the mark to its commit (see
    /// `Launcher::stop_marks`), so it has its own lock: a turn holds the
    /// state lock while it waits for the store. Lock order: the state lock
    /// or the store, then this lock. Never wait for the store while this
    /// lock is held. A stop waits here for at most one gate.
    stop_marks: Mutex<HashSet<i64>>,
    after: Mutex<Option<Arc<AfterStep>>>,
    /// The wakes (job, last session before the wake) whose session this
    /// engine tried to start: one try for each wake and engine. After a failed start, the
    /// person runs `iris job run`.
    tried: Mutex<HashSet<(i64, i64)>>,
    /// Each (job, highest pending comment ID) that a review comment tried to
    /// start (R4-08): one try for each new comment and engine, also when the
    /// start fails before a session registers.
    comment_tried: Mutex<HashSet<(i64, i64)>>,
    /// The done jobs whose workspace the build thread builds again. A
    /// build can wait for the workspace lock without a limit, so no
    /// supervisor turn and no stop waits for it.
    builds: mpsc::Sender<i64>,
    /// The egress proxies that the stop thread stops. A stop joins the
    /// threads of the proxy, so no supervisor turn and no stop or run
    /// waits for it.
    releases: mpsc::Sender<Proxy>,
}

#[derive(Default)]
struct State {
    /// The jobs with a launch thread. Rule 0 of the reap check: the
    /// supervisor does not touch a session of such a job.
    launching: HashSet<i64>,
    /// The sessions whose helper this engine started, by session ID.
    running: HashMap<i64, Live>,
    /// The reason of the last failed reap check of each session.
    left: HashMap<i64, String>,
    /// The closed egress proxies of launches that ended with no running
    /// session, until the reap of their session.
    parked: HashMap<i64, Proxy>,
    /// The sessions with a reap thread.
    busy: HashSet<i64>,
}

struct Live {
    child: Child,
    /// The pipe to the helper. `None` after the shutdown closed it.
    pipe: Option<ChildStdin>,
    status: ChildStderr,
    /// The server of the session socket. The reap thread closes it after
    /// the accepted requests got their answer (step 3 of the shutdown
    /// order), then closes the pipe.
    server: Option<Server>,
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
        let (builds, queue) = mpsc::channel::<i64>();
        {
            let store = store.clone();
            std::thread::Builder::new()
                .name("workspace-builds".into())
                .spawn(move || {
                    for job in queue {
                        build_done(&store, job);
                    }
                })
                .map_err(|e| format!("the build thread did not start: {e}"))?;
        }
        let (releases, stops) = mpsc::channel::<Proxy>();
        std::thread::Builder::new()
            .name("proxy-stops".into())
            .spawn(move || {
                for p in stops {
                    let _ = p.stop();
                }
            })
            .map_err(|e| format!("the proxy stop thread did not start: {e}"))?;
        let s = Arc::new(Sessions {
            store,
            isolation,
            helper,
            state: Mutex::new(State::default()),
            stop_marks: Mutex::default(),
            after: Mutex::new(None),
            tried: Mutex::default(),
            comment_tried: Mutex::default(),
            builds,
            releases,
        });
        let live: HashSet<i64> = iris_job::live_sessions(&s.store)
            .map_err(|e| e.to_string())?
            .iter()
            .map(|l| l.job_id)
            .collect();
        for job in iris_job::list(&s.store, iris_job::State::Done).map_err(|e| e.to_string())? {
            if !live.contains(&job.id) {
                let _ = s.builds.send(job.id);
            }
        }
        s.remove_old_logs();
        Ok(s)
    }

    /// The count of egress proxies that run. For tests.
    pub fn open_proxies(&self) -> usize {
        let st = self.state();
        st.parked.len() + st.running.values().filter(|l| l.proxy.is_some()).count()
    }

    /// Calls `f` after each step of each launch. For tests.
    pub fn set_after(&self, f: Arc<AfterStep>) {
        *self.after.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    }

    /// Removes the session logs (`se-<id>.log`, `se-<id>.egress.log`) of
    /// `sessions/` older than [`LOG_DAYS`]. Other files there stay.
    fn remove_old_logs(&self) {
        let Ok(entries) = std::fs::read_dir(self.data_dir().join("sessions")) else {
            return;
        };
        let age = Duration::from_secs(LOG_DAYS * 86_400);
        for e in entries.flatten() {
            let name = e.file_name();
            let log = name.to_str().is_some_and(is_session_log);
            let old = log
                && e.metadata()
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

    fn marks(&self) -> MutexGuard<'_, HashSet<i64>> {
        self.stop_marks.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `true` if a stop of `job` came during its launch.
    fn stopped(&self, job: i64) -> bool {
        self.marks().contains(&job)
    }

    fn data_dir(&self) -> &Path {
        self.store.dir()
    }

    /// `job.run`: steps 1 and 2 of the launch, then the other steps on a
    /// launch thread. Gives the session ID after the register.
    pub fn run(self: &Arc<Self>, job: i64, agent: &str) -> Result<i64, String> {
        let isolation = self.isolation.clone()?;
        // A second run names the live session (a retry after a lost answer).
        let refuse = |live: Option<i64>| match live {
            Some(id) => format!("{}: a session of this job is live", format_session_id(id)),
            None => "a launch of this job starts".into(),
        };
        if !self.state().launching.insert(job) {
            return Err(refuse(self.live_of(job)));
        }
        if let Some(id) = self.live_of(job) {
            self.launch_over(job);
            return Err(refuse(Some(id)));
        }
        let hook = self.hook();
        let after = |step: Step, s: i64| {
            if step == Step::Registered {
                self.registered(job, s);
            }
            if let Some(h) = &hook {
                h(step, s);
            }
        };
        let begun = self
            .launcher(&*isolation, &|_, _| Ok(None), &after)
            .begin(job, agent);
        let begun = match begun {
            Ok(b) => b,
            Err(e) => {
                self.launch_over(job);
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
            self.launch_over(job);
            return Err(format!("the launch thread did not start: {e}"));
        }
        Ok(session)
    }

    /// Each register of a launch (the first and a revoke retry): if a stop
    /// of the job came during the launch, end the new session with `stop`.
    /// The launch sees the end and starts no agent.
    fn registered(&self, job: i64, session: i64) {
        if self.stopped(job) {
            let _ = iris_job::end(&self.store, session, EndReason::Stop, None, now_ms());
        }
    }

    /// The launch of `job` is over: a stop no longer waits for it.
    fn launch_over(&self, job: i64) {
        let mut st = self.state();
        st.launching.remove(&job);
        self.marks().remove(&job);
    }

    /// Sends `proxy` to the stop thread.
    fn release(&self, proxy: Proxy) {
        let _ = self.releases.send(proxy);
    }

    fn is_reaped(&self, session: i64) -> bool {
        iris_job::session(&self.store, session).is_ok_and(|r| r.reaped_ms.is_some())
    }

    /// The live session of `job`, if one exists.
    fn live_of(&self, job: i64) -> Option<i64> {
        iris_job::live_sessions(&self.store)
            .ok()?
            .into_iter()
            .find(|s| s.job_id == job)
            .map(|s| s.id)
    }

    fn hook(&self) -> Option<Arc<AfterStep>> {
        self.after.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn launcher<'a>(
        &'a self,
        isolation: &'a dyn Isolation,
        egress: &'a Egress<'a>,
        after: &'a dyn Fn(Step, i64),
    ) -> Launcher<'a> {
        Launcher {
            store: &self.store,
            data_dir: self.data_dir(),
            isolation,
            helper: &*self.helper,
            egress,
            stop_marks: &self.stop_marks,
            env: &|name| std::env::var_os(name),
            now: &now_ms,
            after,
        }
    }

    /// Steps 3 to 6, on the launch thread.
    fn launch(&self, isolation: &dyn Isolation, begun: iris_session::launch::Begun) {
        let job = begun.job();
        let agent = begun.agent().to_string();
        let limit_ms = i64::from(begun.max_minutes()) * 60_000;
        let hook = self.hook();
        let after = |step: Step, s: i64| {
            if step == Step::Registered {
                self.registered(job, s);
            }
            if let Some(h) = &hook {
                h(step, s);
            }
        };
        // One proxy for each session of the launch: the first, and the one
        // of a revoke retry. Each has the log and the peer address of its
        // session.
        let proxies: RefCell<HashMap<i64, Proxy>> = RefCell::default();
        let egress = |session: i64, adapter: &Adapter| -> Result<Option<Endpoint>, String> {
            if adapter.allow_hosts.is_empty() {
                return Ok(None);
            }
            let p = self.egress(session, adapter.allow_hosts.clone())?;
            let end = p.endpoint();
            proxies.borrow_mut().insert(session, p);
            Ok(Some(end))
        };
        let outcome = self.launcher(isolation, &egress, &after).finish(begun);
        let mut proxies = proxies.into_inner();
        // The session of a launch that ended with no agent, if its cleanup
        // reaped it: each reap asks for the build of a done job.
        let mut reaped = false;
        // One lock: no turn sees the launch over before the running
        // session is in the map.
        let mut st = self.state();
        st.launching.remove(&job);
        self.marks().remove(&job);
        match outcome {
            Outcome::Running(r) => {
                let proxy = proxies.remove(&r.session);
                let server = self.serve(r.session, r.socket, job, agent);
                st.running.insert(
                    r.session,
                    Live {
                        child: r.helper,
                        pipe: Some(r.pipe),
                        status: r.status,
                        server,
                        socket_path: r.socket_path,
                        proxy,
                        limit_ms,
                        exit: None,
                    },
                );
            }
            Outcome::Ended { session, why, .. } | Outcome::Open { session, why } => {
                say!("iris: {}: {why}", format_session_id(session));
                reaped = self.is_reaped(session);
            }
        }
        // Each other proxy belongs to a session that ended: close it now,
        // and stop it at the reap of its session (the shutdown order).
        for (session, p) in proxies {
            p.close();
            if self.is_reaped(session) {
                self.release(p);
            } else {
                st.parked.insert(session, p);
            }
        }
        drop(st);
        if reaped {
            let _ = self.builds.send(job);
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
        // macOS has no session address: the peer is 127.0.0.1, and the token
        // of the session tells its processes from all others.
        let (peer, token) = if cfg!(target_os = "macos") {
            let token = new_token().map_err(|e| format!("no token: {e}"))?;
            (Ipv4Addr::LOCALHOST, Some(token))
        } else {
            let peer = session_peer(session).ok_or("the session ID has no peer address")?;
            (peer, None)
        };
        Proxy::start(allowed, log, Box::new(SystemResolver), peer, token).map_err(|e| e.to_string())
    }

    fn egress_log(&self, session: i64) -> PathBuf {
        self.data_dir()
            .join("sessions")
            .join(format!("{}.egress.log", format_session_id(session)))
    }

    /// `job.stop`: ends the open session of `job` with `stop`, then waits
    /// up to [`STOP_WAIT`] for the reap. A stop during a launch also ends
    /// each session that a revoke retry of the launch registers, and its
    /// stop mark keeps the gate from a start if an end write fails. Gives the
    /// last session of the job, and `Ok` if the launch is over and that
    /// session was reaped, else the reason that it is not.
    #[allow(clippy::type_complexity)]
    pub fn stop(self: &Arc<Self>, job: i64) -> Result<(i64, Result<(), String>), String> {
        {
            let st = self.state();
            if st.launching.contains(&job) {
                self.marks().insert(job);
            }
        }
        let end = Instant::now() + STOP_WAIT;
        loop {
            // A launch can be before its first register: no session yet.
            if let Some(last) = last_session(&self.store, job)? {
                if last.ended_ms.is_none() {
                    iris_job::end(&self.store, last.id, EndReason::Stop, None, now_ms())
                        .map_err(|e| e.to_string())?;
                }
            }
            self.turn(now_ms());
            let launching = self.state().launching.contains(&job);
            let last = last_session(&self.store, job)?;
            let Some(last) = last else {
                if !launching {
                    return Err("the job has no session".into());
                }
                if Instant::now() >= end {
                    return Err("the launch of the job still runs, with no session".into());
                }
                std::thread::sleep(Duration::from_millis(200));
                continue;
            };
            if !launching && last.reaped_ms.is_some() {
                return Ok((last.id, Ok(())));
            }
            if Instant::now() >= end {
                let why = match launching {
                    true => Some("the launch of the job still runs".to_string()),
                    false => self.state().left.get(&last.id).cloned(),
                };
                return Ok((last.id, Err(why.unwrap_or_else(|| "not known".into()))));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// One supervisor turn over each live session. The turn holds the
    /// state lock only for fast work: the end in the store, the close of
    /// the socket and the pipe. Each ended session gets a reap thread for
    /// the isolation stop and the reap check, which have no time limit, so
    /// a slow session never holds a turn, a stop or a run.
    pub fn turn(self: &Arc<Self>, now: i64) {
        let Ok(live) = iris_job::live_sessions(&self.store) else {
            return;
        };
        let mut st = self.state();
        for s in live {
            if st.launching.contains(&s.job_id) || st.busy.contains(&s.id) {
                continue;
            }
            let helper_reaped = match st.running.get_mut(&s.id) {
                Some(l) => match self.supervise(&s, l, now) {
                    Some(reaped) => reaped,
                    None => continue,
                },
                None => {
                    // An orphan: a session of the last engine, or a launch
                    // that could not write its end.
                    if s.ended_ms.is_none() {
                        let _ =
                            iris_job::end(&self.store, s.id, EndReason::LaunchFailed, None, now);
                    }
                    false
                }
            };
            // Steps 3 and 4 of the shutdown order wait, so the reap thread
            // does them.
            let (server, pipe) = st
                .running
                .get_mut(&s.id)
                .map(|l| (l.server.take(), l.pipe.take()))
                .unwrap_or_default();
            let me = self.clone();
            let (id, job) = (s.id, s.job_id);
            let spawned = std::thread::Builder::new()
                .name(format!("reap-{}", format_session_id(id)))
                .spawn(move || me.reap_one(id, job, helper_reaped, server, pipe));
            // With no thread, the next turn tries again.
            if spawned.is_ok() {
                st.busy.insert(id);
            }
        }
    }

    /// One running session of this engine, under the state lock. Gives
    /// `None` while the session runs or its helper still stops the agent,
    /// else whether this engine reaped the helper: the session is ready
    /// for the reap check.
    fn supervise(&self, s: &Session, l: &mut Live, now: i64) -> Option<bool> {
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
            self.shutdown(l);
            return Some(true);
        }
        if s.ended_ms.is_none() {
            match s.started_ms {
                Some(start) if now - start >= l.limit_ms => {
                    let _ = iris_job::end(&self.store, s.id, EndReason::TimeLimit, None, now);
                }
                _ => return None,
            }
        }
        // The session ended in the store (a stop, a revoke, a job move, a
        // time limit): the helper stops the group when the pipe closes. The
        // reap thread stops the isolation; a later turn reaps the helper.
        self.shutdown(l);
        Some(false)
    }

    /// Starts the server of the session socket, with the author of the
    /// session that owns the socket (after a revoke retry, the second
    /// session). If its thread does not start, the session ends with
    /// `launch_failed`: an agent with no socket cannot do its job.
    fn serve(&self, session: i64, socket: UnixListener, job: i64, agent: String) -> Option<Server> {
        let me = Author {
            kind: AuthorKind::Agent,
            name: Some(agent),
            session: Some(format_session_id(session)),
            job: Some(iris_job::format_job_id(job)),
        };
        let store = self.store.clone();
        let handler: Arc<Handler> =
            Arc::new(move |line, mut out| crate::socket::answer_as(&store, &me, line, &mut out));
        let store = self.store.clone();
        let is_open: Arc<IsOpen> = Arc::new(move || {
            iris_job::session(&store, session).is_ok_and(|s| s.ended_ms.is_none())
        });
        match Server::start(socket, handler, is_open) {
            Ok(s) => Some(s),
            Err(e) => {
                say!(
                    "iris: {}: the session socket did not start: {e}",
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
        }
    }

    /// The fast part of the shutdown order, under the state lock: the grant
    /// ended in the store; close the egress proxy (its connections end;
    /// the stop thread frees its port after the reap). The reap thread
    /// does the rest: the session socket, the pipe and the isolation.
    fn shutdown(&self, l: &mut Live) {
        if let Some(p) = &l.proxy {
            p.close();
        }
    }

    /// The reap thread of one ended session: the answers of the accepted
    /// requests and the close of the session socket, the close of the pipe
    /// (the helper stops the group), the isolation stop, then the reap
    /// check. So a verb that ends its own session gets its answer before
    /// a process of the agent gets a signal. On a reap, it removes the
    /// session from the state and sends its job to the build thread.
    fn reap_one(
        &self,
        session: i64,
        job: i64,
        helper_reaped: bool,
        server: Option<Server>,
        pipe: Option<ChildStdin>,
    ) {
        if let Some(s) = server {
            s.shutdown(DRAIN_WAIT);
        }
        drop(pipe);
        if let Ok(iso) = &self.isolation {
            let _ = iso.stop(session);
        }
        let result = self.reap(session, helper_reaped, now_ms());
        let mut st = self.state();
        st.busy.remove(&session);
        match result {
            Ok(()) => {
                if let Some(mut l) = st.running.remove(&session) {
                    let _ = std::fs::remove_file(&l.socket_path);
                    if let Some(p) = l.proxy.take() {
                        self.release(p);
                    }
                }
                if let Some(p) = st.parked.remove(&session) {
                    self.release(p);
                }
                st.left.remove(&session);
                drop(st);
                let _ = self.builds.send(job);
            }
            Err(why) => {
                st.left.insert(session, why);
            }
        }
    }

    fn reap(&self, session: i64, helper_reaped: bool, now: i64) -> Result<(), String> {
        let iso = self
            .isolation
            .as_ref()
            .map_err(|e| format!("no isolation: {e}"))?;
        let s = iris_job::session(&self.store, session).map_err(|e| e.to_string())?;
        match reap_check(&self.store, &**iso, &s, helper_reaped, now) {
            Ok(notes) => {
                if let Some(p) = notes.problem() {
                    say!("iris: {}: {p}", iris_job::format_job_id(s.job_id));
                }
                Ok(())
            }
            Err(why) => {
                // One line in the engine log for a session still live 10
                // minutes after its end.
                if let Some(end) = s.ended_ms {
                    if now - end >= 600_000 && now - end < 600_000 + 1000 {
                        say!("iris: {} is not reaped: {why}", format_session_id(session));
                    }
                }
                Err(why)
            }
        }
    }

    /// The wake of R5-04 (docs/specs/2026-11-17-wait.md): wakes each
    /// waiting job whose reply came or whose wait ended at `now`, then starts
    /// one session for each job that a wake of the engine moved to `working`,
    /// with the agent of its last session. A job with a live session (the
    /// session of the wait before its reap) gets its session at a later call.
    pub fn wake(self: &Arc<Self>, now: i64) {
        if let Err(e) = iris_workspace::wake_due(&self.store, now) {
            say!("iris: the wake of the waiting jobs failed: {e}");
        }
        self.comment_wake(now);
        let Ok(woken) = iris_job::woken(&self.store) else {
            return;
        };
        for (job, last, agent) in woken {
            if self.live_of(job).is_some() || self.state().launching.contains(&job) {
                continue;
            }
            let first = self
                .tried
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert((job, last));
            if !first {
                continue;
            }
            if let Err(e) = self.run(job, &agent) {
                say!(
                    "iris: {}: the session after the wake did not start: {e}",
                    iris_job::format_job_id(job)
                );
            }
        }
    }

    /// The review comments of R4-08 (docs/specs/2026-11-17-review-comments.md,
    /// section 5, stage 2): a `waiting` job with a pending comment gets a
    /// wake of the engine, and a `working` job with a pending comment and no
    /// live session gets a new session. The comment stays pending until the
    /// gate of a session writes its delivery, so a session that is live now
    /// or a launch that runs now gets nothing more here. Each new comment
    /// gets one try: a start that fails, also before its register, waits for
    /// the next comment or for `iris job run`.
    fn comment_wake(self: &Arc<Self>, now: i64) {
        let mut jobs = None;
        if let Err(e) = self.store.read(|c| {
            jobs = Some(iris_changeset::commented_jobs_in(c));
            Ok(())
        }) {
            say!("iris: the read of the review comments failed: {e}");
            return;
        }
        let jobs = match jobs.expect("the read ran") {
            Ok(j) => j,
            Err(e) => {
                say!("iris: the read of the review comments failed: {e}");
                return;
            }
        };
        for (job, state, comment) in jobs {
            if self.live_of(job).is_some() || self.state().launching.contains(&job) {
                continue;
            }
            let first = self
                .comment_tried
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert((job, comment));
            if !first {
                continue;
            }
            let id = iris_job::format_job_id(job);
            if state == "waiting" {
                if let Err(e) =
                    iris_job::wake(&self.store, job, iris_job::By::Engine, Some("comment"), now)
                {
                    say!("iris: {id}: the wake for a review comment failed: {e}");
                }
                continue;
            }
            if let Err(e) = self.resume(job) {
                say!("iris: {id}: the session for a review comment did not start: {e}");
            }
        }
    }

    /// Starts a new session of `job` with the agent of its last session.
    /// For a caller that gives a working job new input (for example a
    /// review comment, R4-08).
    pub fn resume(self: &Arc<Self>, job: i64) -> Result<i64, String> {
        let last = last_session(&self.store, job)?.ok_or("the job has no session")?;
        self.run(job, &last.agent)
    }

    /// The supervisor thread: one turn and one wake each [`TURN`].
    pub fn supervise_forever(self: Arc<Self>) {
        self.supervise_with(&now_ms)
    }

    /// [`Sessions::supervise_forever`] with the clock `now`. For tests.
    pub fn supervise_with(self: Arc<Self>, now: &dyn Fn() -> i64) {
        loop {
            let t = now();
            self.turn(t);
            self.wake(t);
            std::thread::sleep(TURN);
        }
    }
}

/// Builds the workspace of `job` again if the job is done and its
/// workspace folder exists: the mail files go, and the files of the agent
/// stay. The caller sends only a job with no live session; a done job
/// gets no new session.
fn build_done(store: &Store, job: i64) {
    let dir = iris_workspace::default_dir(store.dir(), job);
    let done = iris_job::get(store, job).is_ok_and(|j| j.state == iris_job::State::Done);
    if !done || std::fs::symlink_metadata(&dir).is_err() {
        return;
    }
    if let Err(e) = iris_workspace::build(store, job, &dir) {
        say!(
            "iris: {}: the workspace of the done job was not built again: {e}",
            iris_job::format_job_id(job)
        );
    }
}

/// True for the name of a session log: `se-<id>.log` or
/// `se-<id>.egress.log`.
fn is_session_log(name: &str) -> bool {
    let id = name
        .strip_suffix(".egress.log")
        .or_else(|| name.strip_suffix(".log"));
    id.and_then(iris_job::parse_session_id).is_some()
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
