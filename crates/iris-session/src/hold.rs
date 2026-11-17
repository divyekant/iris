//! The hold helper of a session (spec "The hold helper"). The engine starts
//! `iris session-hold` (R4-04e calls [`hold`]) with a pipe on its standard
//! input, the session log on its standard output and a second pipe to the
//! engine on its standard error. The helper runs no code of the agent until
//! the engine writes `start`.

use std::io::{self, Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use crate::isolation::{Command, Isolation};

/// The wait between `SIGTERM` and `SIGKILL` to the group of the agent.
pub const TERM_WAIT: Duration = Duration::from_secs(5);
/// The size limit of the session log.
pub const LOG_LIMIT: u64 = 64 * 1024;
/// The longest wait for the output thread after the reap.
const DRAIN_WAIT: Duration = Duration::from_secs(2);
/// The longest first line that the helper reads.
const LINE_MAX: usize = 64;

/// The session log: fixed status lines and numbers only, never a byte of the
/// agent. Lines past the limit are dropped.
struct Log<W: Write> {
    out: W,
    written: u64,
}

impl<W: Write> Log<W> {
    fn line(&mut self, text: &str) {
        let line = format!("{text}\n");
        if self.written + line.len() as u64 > LOG_LIMIT {
            return;
        }
        if self.out.write_all(line.as_bytes()).is_ok() {
            self.written += line.len() as u64;
        }
        let _ = self.out.flush();
    }
}

/// The two events that end the hold of a running agent.
enum Event {
    /// The agent exited. It stays a zombie until the reap.
    Exited,
    /// The pipe from the engine ended: the engine stopped or closed it.
    PipeEnded,
}

/// Runs the hold helper with the wait of the spec ([`TERM_WAIT`]). Gives the
/// exit code of the helper: 0 after a reap, 1 if the agent did not start,
/// 2 if the engine sent no `start`.
pub fn hold(
    isolation: &dyn Isolation,
    session: i64,
    command: &Command,
    input: impl Read + Send + 'static,
    status: impl Write,
    log: impl Write,
) -> i32 {
    hold_with(
        isolation,
        session,
        command,
        input,
        status,
        log,
        Tuning {
            term_wait: TERM_WAIT,
            drain_delay: Duration::ZERO,
            fail_thread: None,
        },
    )
}

/// The waits of [`hold_with`], and a fault for tests.
#[derive(Clone, Copy)]
pub(crate) struct Tuning {
    /// The wait between `SIGTERM` and `SIGKILL`.
    pub term_wait: Duration,
    /// A delay before the output thread reads.
    pub drain_delay: Duration,
    /// The name of a helper thread that does not start.
    pub fail_thread: Option<&'static str>,
}

impl Tuning {
    fn spawn(
        &self,
        name: &'static str,
        f: impl FnOnce() + Send + 'static,
    ) -> io::Result<thread::JoinHandle<()>> {
        if self.fail_thread == Some(name) {
            return Err(io::Error::other("test thread failure"));
        }
        thread::Builder::new().name(name.into()).spawn(f)
    }
}

/// [`hold`] with other waits, for tests.
pub(crate) fn hold_with(
    isolation: &dyn Isolation,
    session: i64,
    command: &Command,
    mut input: impl Read + Send + 'static,
    mut status: impl Write,
    log: impl Write,
    tuning: Tuning,
) -> i32 {
    let mut log = Log {
        out: log,
        written: 0,
    };
    // 1. One line from the engine. No code of the agent runs before it.
    if read_line(&mut input).as_deref() != Some("start") {
        log.line("no start: the helper exits with no agent");
        return 2;
    }
    // 2. Start the agent in a new process group of its own. The helper is
    // not in that group, so a signal to the group never reaches the helper.
    let (child, output) = match spawn(command) {
        Ok(x) => x,
        Err(_) => {
            log.line("the agent did not start");
            let _ = writeln!(status, "failed");
            let _ = status.flush();
            return 1;
        }
    };
    let pgid = child.id() as i32;
    let _ = writeln!(status, "started {pgid}");
    let _ = status.flush();
    log.line(&format!("started, process group {pgid}"));

    // 3. Start the three helper threads, then wait for the first of two
    // events. Each thread is necessary: the output thread reads the output
    // without end, so the agent never blocks and no byte is stored, and the
    // other two see the two events. If one thread does not start, the
    // helper stops the agent now.
    let dropped = Arc::new(AtomicU64::new(0));
    let count = dropped.clone();
    let drain_delay = tuning.drain_delay;
    // The output thread sends on `drained` when it reads the end of the pipe.
    let (drained_tx, drained) = mpsc::channel::<()>();
    let (tx, rx) = mpsc::channel();
    let exited = tx.clone();
    let started = tuning
        .spawn("iris-hold-output", move || {
            thread::sleep(drain_delay);
            drain(output, &count);
            let _ = drained_tx.send(());
        })
        .and_then(|_| {
            tuning.spawn("iris-hold-wait", move || {
                wait_exit_no_reap(pgid);
                let _ = exited.send(Event::Exited);
            })
        })
        .and_then(|_| {
            tuning.spawn("iris-hold-pipe", move || {
                let mut buf = [0u8; 64];
                loop {
                    match input.read(&mut buf) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
                let _ = tx.send(Event::PipeEnded);
            })
        });
    if started.is_err() {
        log.line("a helper thread did not start");
    } else {
        match rx.recv() {
            Ok(Event::Exited) => log.line("the agent exited"),
            Ok(Event::PipeEnded) | Err(_) => log.line("the pipe from the engine ended"),
        }
    }

    // 4. Stop the group. The agent is not reaped before the last signal, so
    // its process ID (the group ID) cannot go to another process.
    signal_group(pgid, libc::SIGTERM);
    log.line("SIGTERM to the process group");
    thread::sleep(tuning.term_wait);
    signal_group(pgid, libc::SIGKILL);
    log.line("SIGKILL to the process group");
    match isolation.stop(session) {
        Ok(()) => log.line("isolation stop: ok"),
        Err(_) => log.line("isolation stop: failed"),
    }
    // 5. The reap, only now.
    let line = reap(child);
    // Each process of the session is stopped now, so the output thread
    // reads the last bytes and the end of the pipe. Wait for it, so that
    // the count holds each byte. A process that the stop missed can keep
    // the pipe open: then the count is a lower bound, and the log says so.
    let end = drained.recv_timeout(DRAIN_WAIT);
    let count = dropped.load(Ordering::SeqCst);
    log.line(&match end {
        Ok(()) => format!("{line}; {count} bytes of output dropped"),
        Err(mpsc::RecvTimeoutError::Timeout) => format!(
            "{line}; at least {count} bytes of output dropped, the output pipe is still open"
        ),
        // The output thread did not start.
        Err(mpsc::RecvTimeoutError::Disconnected) => format!("{line}; the output was not read"),
    });
    let _ = writeln!(status, "{line}");
    let _ = status.flush();
    0
}

/// Reads one line of at most [`LINE_MAX`] bytes, one byte at a time, so that
/// no byte after the line is taken from the pipe.
fn read_line(input: &mut impl Read) -> Option<String> {
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    loop {
        match input.read(&mut b) {
            Ok(1) if b[0] == b'\n' => return String::from_utf8(line).ok(),
            Ok(1) if line.len() < LINE_MAX => line.push(b[0]),
            Ok(_) => return None,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
}

/// Starts the agent with its standard input on `/dev/null`, and its standard
/// output and standard error on one pipe to the helper.
fn spawn(command: &Command) -> io::Result<(Child, io::PipeReader)> {
    let (reader, writer) = io::pipe()?;
    let child = std::process::Command::new(&command.program)
        .args(&command.args)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer)
        .spawn()?;
    // The Command and its copies of the write end are gone: the reader sees
    // the end of the pipe when the agent and its children close it.
    Ok((child, reader))
}

fn drain(mut output: io::PipeReader, dropped: &AtomicU64) {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        match output.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                dropped.fetch_add(n as u64, Ordering::SeqCst);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

/// Waits until the agent exits, and leaves it a zombie
/// (`waitid(P_PID, pid, WEXITED | WNOWAIT)`).
fn wait_exit_no_reap(pid: i32) {
    loop {
        // SAFETY: info is a valid siginfo_t for the call to write into.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let r = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if r == 0 || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return;
        }
    }
}

fn signal_group(pgid: i32, signal: i32) {
    // SAFETY: kill has no memory effects; an error (ESRCH) means that the
    // group is empty, which the reap check sees later.
    unsafe {
        libc::kill(-pgid, signal);
    }
}

/// Reaps the agent and gives the status line for the engine.
fn reap(mut child: Child) -> String {
    match child.wait() {
        Ok(s) => match (s.code(), s.signal()) {
            (Some(code), _) => format!("exited {code}"),
            (None, Some(sig)) => format!("signaled {sig}"),
            (None, None) => "signaled 0".to_string(),
        },
        Err(_) => "signaled 0".to_string(),
    }
}

#[cfg(test)]
#[path = "hold_tests.rs"]
mod tests;
