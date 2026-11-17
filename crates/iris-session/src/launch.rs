//! The launch of a session (spec "Launch"): register, build, bind the
//! session socket, start the held helper, and the start gate. R4-04e calls
//! it from the engine.
//!
//! Each failure after the register ends the session with `launch_failed`,
//! closes the pipe, waits for the helper if one started, removes the socket
//! and runs the reap check. So no live session stays without a process.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{self, Child, ChildStderr, ChildStdin, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use iris_job::{format_session_id, EndReason};
use iris_store::Store;

use crate::config::{self, Adapter};
use crate::isolation::Isolation;
use crate::reap::reap_check;

/// The most builds of one launch while revokes come during each build.
pub const BUILD_TRIES: u32 = 5;
/// The longest wait for the line `started <pgid>` of the helper, inside the
/// gate. Less than the `busy_timeout` of the store (5 seconds).
pub const START_WAIT: Duration = Duration::from_secs(2);
/// The tries of the end write of a failed launch.
pub const END_TRIES: u32 = 3;
/// The longest wait for the helper to exit after the pipe closed. The
/// helper needs at most `TERM_WAIT` and the isolation stop. After this wait
/// the launch kills the helper, and the lifetime rule of the isolation ends
/// each process of the session.
pub const HELPER_WAIT: Duration = Duration::from_secs(15);

/// A step of the launch, for [`Launcher::after`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Registered,
    Built,
    Bound,
    HelperStarted,
}

/// Gives the egress port of a session from its adapter (see
/// [`Launcher::egress`]).
pub type Egress<'a> = dyn Fn(i64, &Adapter) -> Result<Option<u16>, String> + 'a;

/// What the launch needs from the engine.
pub struct Launcher<'a> {
    pub store: &'a Store,
    pub data_dir: &'a Path,
    pub isolation: &'a dyn Isolation,
    /// Makes the command of the hold helper from its arguments
    /// (`<session> <program> [<argument>...]`). The engine gives its own
    /// program with `session-hold` first. The launch sets the folder, the
    /// environment and the three streams.
    pub helper: &'a dyn Fn(&[OsString]) -> process::Command,
    /// Gives the egress port of a session, if its adapter has allowed
    /// hosts. The launch calls it once for each session that it registers
    /// (a revoke retry registers a second one), after the bind and before
    /// the helper, so each session gets its own proxy. An error ends the
    /// session with `launch_failed`.
    pub egress: &'a Egress<'a>,
    /// Gives `true` if a stop of the job came during the launch. The gate
    /// reads it in its write transaction, so no agent starts after a stop,
    /// also when the end write of the stop failed. The function must not
    /// wait for the store.
    pub stopped: &'a dyn Fn(i64) -> bool,
    /// The environment of the engine. Only the names of the agent
    /// environment are read.
    pub env: &'a dyn Fn(&str) -> Option<OsString>,
    /// The clock, in milliseconds.
    pub now: &'a dyn Fn() -> i64,
    /// Called after each step with the session. The engine gives a function
    /// that does nothing. Tests change the store between two steps.
    pub after: &'a dyn Fn(Step, i64),
}

/// A session after the register (steps 1 and 2).
pub struct Begun {
    pub session: i64,
    job: i64,
    adapter: Adapter,
    max_minutes: u32,
}

impl Begun {
    /// The job of the session.
    pub fn job(&self) -> i64 {
        self.job
    }

    /// The allowed hosts of the adapter. If there is none, the session
    /// gets no egress port.
    pub fn allow_hosts(&self) -> &[String] {
        &self.adapter.allow_hosts
    }

    /// The time limit of the session, in minutes, from the same read of
    /// `agents.toml` as the adapter.
    pub fn max_minutes(&self) -> u32 {
        self.max_minutes
    }
}

/// A session after the gate: the agent runs.
pub struct Running {
    pub session: i64,
    /// The process group of the agent.
    pub pgid: i32,
    pub helper: Child,
    /// The pipe to the helper. A close stops the session.
    pub pipe: ChildStdin,
    /// The second pipe of the helper. It gives the last status line.
    pub status: ChildStderr,
    pub socket: UnixListener,
    pub socket_path: PathBuf,
}

/// The result of [`Launcher::finish`].
pub enum Outcome {
    Running(Running),
    /// The launch ended with no agent. `session` is the last session of the
    /// launch (a revoke registers a second one).
    Ended {
        session: i64,
        end: EndReason,
        why: String,
    },
    /// The launch failed, but the store did not take the end of the
    /// session: the row stays open. The engine must not report an end. Its
    /// supervisor ends an open session that has no launch thread and no
    /// helper.
    Open {
        session: i64,
        why: String,
    },
}

struct Helper {
    child: Child,
    pipe: ChildStdin,
    status: ChildStderr,
}

enum Attempt {
    Running(Running),
    Ended(EndReason, String),
    /// The launch failed, and the end write failed too.
    Open(String),
}

enum Gate {
    Ended(EndReason),
    /// A stop of the job came, but the store may have no end.
    Stopped,
    Failed(String),
}

impl Launcher<'_> {
    /// Steps 1 and 2: resolve the adapter, check the isolation, register.
    /// An error here writes no session row, except a failed register.
    pub fn begin(&self, job: i64, agent: &str) -> Result<Begun, String> {
        let file = config::read_agents(self.data_dir).map_err(|e| e.to_string())?;
        let path = (self.env)("PATH");
        let adapter = config::adapter(&file, agent, path.as_deref())?;
        self.isolation
            .check()
            .map_err(|e| format!("the isolation does not work on this system: {e}"))?;
        let session = iris_job::register(self.store, job, &adapter.name, (self.now)())
            .map_err(|e| e.to_string())?;
        (self.after)(Step::Registered, session);
        Ok(Begun {
            session,
            job,
            adapter,
            max_minutes: file.max_minutes,
        })
    }

    /// Steps 3 to 6. A revoke before the gate starts the launch again once,
    /// with a new register and a new build. Each other end stops the launch.
    pub fn finish(&self, begun: Begun) -> Outcome {
        let Begun {
            mut session,
            job,
            adapter,
            ..
        } = begun;
        let mut retried = false;
        loop {
            match self.attempt(job, session, &adapter) {
                Attempt::Running(r) => return Outcome::Running(r),
                Attempt::Ended(EndReason::Revoke, why) if !retried => {
                    retried = true;
                    match iris_job::register(self.store, job, &adapter.name, (self.now)()) {
                        Ok(s) => {
                            (self.after)(Step::Registered, s);
                            session = s;
                        }
                        Err(e) => {
                            return Outcome::Ended {
                                session,
                                end: EndReason::Revoke,
                                why: format!("{why}; the second register failed: {e}"),
                            }
                        }
                    }
                }
                Attempt::Ended(end, why) => return Outcome::Ended { session, end, why },
                Attempt::Open(why) => return Outcome::Open { session, why },
            }
        }
    }

    fn attempt(&self, job: i64, session: i64, adapter: &Adapter) -> Attempt {
        let dir = iris_workspace::default_dir(self.data_dir, job);
        // 3. Build. A revoke during the build changes the revoke count.
        let mut tries = 0;
        loop {
            tries += 1;
            let built = match iris_workspace::build(self.store, job, &dir) {
                Ok(b) => b,
                Err(e) => return self.fail(session, format!("the build failed: {e}"), None, None),
            };
            (self.after)(Step::Built, session);
            if let Some(a) = self.check_end(session, None, None) {
                return a;
            }
            match iris_job::revoked_count(self.store, job) {
                Ok(n) if n == built.revoked => break,
                Ok(_) if tries < BUILD_TRIES => {}
                Ok(_) => {
                    let why = format!("the scope changed during each of {BUILD_TRIES} builds");
                    return self.fail(session, why, None, None);
                }
                Err(e) => {
                    return self.fail(session, format!("cannot read the scope: {e}"), None, None)
                }
            }
        }
        // 4. Bind the session socket.
        let socket_path = dir.join(".iris").join("socket");
        let socket = match bind(&socket_path) {
            Ok(s) => s,
            Err(why) => return self.fail(session, why, None, None),
        };
        (self.after)(Step::Bound, session);
        if let Some(a) = self.check_end(session, None, Some(&socket_path)) {
            return a;
        }
        // 5. Start the held helper and write its process ID.
        let port = match (self.egress)(session, adapter) {
            Ok(p) => p,
            Err(why) => {
                let why = format!("no egress port: {why}");
                return self.fail(session, why, None, Some(&socket_path));
            }
        };
        let helper = match self.start_helper(session, &dir, adapter, port) {
            Ok(h) => h,
            Err(why) => return self.fail(session, why, None, Some(&socket_path)),
        };
        if let Err(e) = iris_job::set_helper(self.store, session, i64::from(helper.child.id())) {
            let why = format!("cannot write the helper: {e}");
            return self.fail(session, why, Some(helper), Some(&socket_path));
        }
        (self.after)(Step::HelperStarted, session);
        // 6. The gate.
        self.gate(job, session, helper, socket, socket_path)
    }

    /// Step 6: the check of the end, the start and the spawn, in one write
    /// transaction. A revoke or a job move is a write transaction too, so it
    /// commits before the check or after the commit of the gate.
    fn gate(
        &self,
        job: i64,
        session: i64,
        mut helper: Helper,
        socket: UnixListener,
        socket_path: PathBuf,
    ) -> Attempt {
        let mut result: Option<Result<i32, Gate>> = None;
        let now = (self.now)();
        let written = self.store.write(|tx| {
            // An Err rolls the transaction back. The launch acts on
            // `result`, not on this placeholder.
            let rollback = || iris_store::Error::Query(String::new());
            // Take the write lock first, as BEGIN IMMEDIATE does: an UPDATE
            // takes it at its start, also when it changes no row. So no
            // writer of another process comes between the check and the
            // spawn.
            tx.execute(
                "UPDATE job_session SET id = id WHERE id = ?1 AND 0",
                [session],
            )?;
            let s = match iris_job::session_in(tx, session) {
                Ok(s) => s,
                Err(e) => {
                    result = Some(Err(Gate::Failed(format!("cannot read the session: {e}"))));
                    return Err(rollback());
                }
            };
            if let Some(end) = s.end {
                result = Some(Err(Gate::Ended(end)));
                return Err(rollback());
            }
            if (self.stopped)(job) {
                result = Some(Err(Gate::Stopped));
                return Err(rollback());
            }
            match iris_job::start_in(tx, session, now) {
                Ok(true) => {}
                Ok(false) => {
                    result = Some(Err(Gate::Failed(
                        "the start move found no open session".into(),
                    )));
                    return Err(rollback());
                }
                Err(e) => {
                    result = Some(Err(Gate::Failed(format!("the start move failed: {e}"))));
                    return Err(rollback());
                }
            }
            if helper
                .pipe
                .write_all(b"start\n")
                .and_then(|()| helper.pipe.flush())
                .is_err()
            {
                result = Some(Err(Gate::Failed("cannot write start to the helper".into())));
                return Err(rollback());
            }
            let Some(pgid) = read_started(&mut helper.status, START_WAIT) else {
                let why = format!(
                    "the helper did not confirm the start in {} seconds",
                    START_WAIT.as_secs()
                );
                result = Some(Err(Gate::Failed(why)));
                return Err(rollback());
            };
            if let Err(e) = iris_job::set_agent_in(tx, session, i64::from(pgid)) {
                result = Some(Err(Gate::Failed(format!("the spawn move failed: {e}"))));
                return Err(rollback());
            }
            result = Some(Ok(pgid));
            Ok(())
        });
        match (written, result) {
            (Ok(()), Some(Ok(pgid))) => Attempt::Running(Running {
                session,
                pgid,
                helper: helper.child,
                pipe: helper.pipe,
                status: helper.status,
                socket,
                socket_path,
            }),
            (_, Some(Err(Gate::Ended(end)))) => {
                drop(socket);
                let tail = self.cleanup(session, Some(helper), Some(&socket_path));
                Attempt::Ended(
                    end,
                    format!(
                        "the session ended before the start ({}){tail}",
                        end.as_str()
                    ),
                )
            }
            (_, Some(Err(Gate::Stopped))) => {
                drop(socket);
                let why = "a stop came before the start".to_string();
                self.end_now(
                    session,
                    EndReason::Stop,
                    why,
                    Some(helper),
                    Some(&socket_path),
                )
            }
            (_, Some(Err(Gate::Failed(why)))) => {
                drop(socket);
                self.fail(session, why, Some(helper), Some(&socket_path))
            }
            (Err(e), _) => {
                drop(socket);
                // The helper may have the start: the close of the pipe
                // makes it stop the agent.
                let why = format!("the gate did not commit: {e}");
                self.fail(session, why, Some(helper), Some(&socket_path))
            }
            (Ok(()), _) => unreachable!("the gate sets its result"),
        }
    }

    fn start_helper(
        &self,
        session: i64,
        dir: &Path,
        adapter: &Adapter,
        egress_port: Option<u16>,
    ) -> Result<Helper, String> {
        let logs = self.data_dir.join("sessions");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&logs)
            .map_err(|e| format!("cannot make the session log folder: {e}"))?;
        let log = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(logs.join(format!("{}.log", format_session_id(session))))
            .map_err(|e| format!("cannot make the session log: {e}"))?;
        let command =
            self.isolation
                .command(session, dir, egress_port, &adapter.program, &adapter.args);
        let mut args: Vec<OsString> = vec![session.to_string().into(), command.program.into()];
        args.extend(command.args);
        let env = config::agent_env(adapter, egress_port, |n| (self.env)(n));
        let mut child = (self.helper)(&args)
            .current_dir(dir)
            .env_clear()
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(log)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("the helper did not start: {e}"))?;
        let (Some(pipe), Some(status)) = (child.stdin.take(), child.stderr.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("the helper has no pipes".into());
        };
        Ok(Helper {
            child,
            pipe,
            status,
        })
    }

    /// If the session ended (a stop, a revoke or a job move), cleans up and
    /// gives the end.
    fn check_end(
        &self,
        session: i64,
        helper: Option<Helper>,
        socket: Option<&Path>,
    ) -> Option<Attempt> {
        match iris_job::session(self.store, session) {
            Ok(s) => s.end.map(|end| {
                let tail = self.cleanup(session, helper, socket);
                Attempt::Ended(
                    end,
                    format!(
                        "the session ended during the launch ({}){tail}",
                        end.as_str()
                    ),
                )
            }),
            Err(e) => Some(self.fail(
                session,
                format!("cannot read the session: {e}"),
                helper,
                socket,
            )),
        }
    }

    /// Ends the session with `launch_failed` (the first end stays), then
    /// cleans up.
    fn fail(
        &self,
        session: i64,
        why: String,
        helper: Option<Helper>,
        socket: Option<&Path>,
    ) -> Attempt {
        self.end_now(session, EndReason::LaunchFailed, why, helper, socket)
    }

    /// Ends the session with `reason` (the first end stays), then cleans
    /// up.
    fn end_now(
        &self,
        session: i64,
        reason: EndReason,
        why: String,
        helper: Option<Helper>,
        socket: Option<&Path>,
    ) -> Attempt {
        let mut written = Err(iris_job::Error::NotFound);
        for i in 0..END_TRIES {
            if i > 0 {
                thread::sleep(Duration::from_millis(100));
            }
            written = iris_job::end(self.store, session, reason, None, (self.now)());
            if written.is_ok() {
                break;
            }
        }
        let end = match written {
            Ok(true) => reason,
            // An earlier end (a stop, a revoke) stays.
            Ok(false) => iris_job::session(self.store, session)
                .ok()
                .and_then(|s| s.end)
                .unwrap_or(EndReason::LaunchFailed),
            Err(e) => {
                // The store has no end: never report one. The supervisor
                // ends an open session with no launch and no helper.
                let tail = self.cleanup(session, helper, socket);
                return Attempt::Open(format!(
                    "{why}; cannot write the end of the session: {e}{tail}"
                ));
            }
        };
        let tail = self.cleanup(session, helper, socket);
        Attempt::Ended(end, format!("{why}{tail}"))
    }

    /// Closes the pipe, waits for the helper, removes the socket and runs
    /// the reap check. Gives a note if the session stays live.
    fn cleanup(&self, session: i64, helper: Option<Helper>, socket: Option<&Path>) -> String {
        let mut helper_reaped = true;
        if let Some(Helper {
            child,
            pipe,
            status,
        }) = helper
        {
            drop(pipe);
            helper_reaped = wait_helper(child);
            drop(status);
        }
        if let Some(p) = socket {
            let _ = fs::remove_file(p);
        }
        let checked = iris_job::session(self.store, session)
            .map_err(|e| format!("cannot read the session: {e}"))
            .and_then(|s| reap_check(self.store, self.isolation, &s, helper_reaped, (self.now)()));
        match checked {
            Ok(()) => String::new(),
            Err(why) => format!("; the session stays live: {why}"),
        }
    }
}

/// The longest path of a socket on this system, with no NUL byte.
fn socket_path_max() -> usize {
    // SAFETY: sockaddr_un is plain data; all zero bytes is a valid value.
    let a: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    a.sun_path.len() - 1
}

/// Step 4: binds the session socket with mode 0600.
fn bind(path: &Path) -> Result<UnixListener, String> {
    let len = path.as_os_str().len();
    let max = socket_path_max();
    if len > max {
        return Err(format!(
            "the session socket path has {len} bytes, and this system permits {max}; use a shorter data folder"
        ));
    }
    let socket =
        UnixListener::bind(path).map_err(|e| format!("cannot bind the session socket: {e}"))?;
    if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
        let _ = fs::remove_file(path);
        return Err(format!("cannot set the mode of the session socket: {e}"));
    }
    Ok(socket)
}

/// Reads the line `started <pgid>` of the helper, one byte at a time, for
/// at most `wait`. A group ID of 0 or 1 is refused.
fn read_started(status: &mut ChildStderr, wait: Duration) -> Option<i32> {
    let end = Instant::now() + wait;
    let mut line = Vec::new();
    loop {
        let left = end.checked_duration_since(Instant::now())?;
        let mut fd = libc::pollfd {
            fd: status.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = left.as_millis().clamp(1, i32::MAX as u128) as i32;
        // SAFETY: fd is one valid pollfd for the call.
        let r = unsafe { libc::poll(&mut fd, 1, ms) };
        if r < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
        if r == 0 {
            return None;
        }
        let mut b = [0u8; 1];
        match status.read(&mut b) {
            Ok(1) if b[0] == b'\n' => break,
            Ok(1) if line.len() < 64 => line.push(b[0]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            _ => return None,
        }
    }
    let pgid: i32 = std::str::from_utf8(&line)
        .ok()?
        .strip_prefix("started ")?
        .parse()
        .ok()?;
    (pgid > 1).then_some(pgid)
}

/// Waits for the helper for at most [`HELPER_WAIT`], then kills it. Gives
/// true if the helper was reaped.
fn wait_helper(mut child: Child) -> bool {
    let end = Instant::now() + HELPER_WAIT;
    while Instant::now() < end {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    child.wait().is_ok()
}
