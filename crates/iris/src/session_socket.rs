//! The session socket `.iris/socket` of one running session
//! (docs/specs/2026-11-17-session-launcher.md, "The session socket"). One
//! request and one answer on each connection, each one line of JSON, as on
//! `iris.sock`. The engine knows the session from the socket, never from
//! the request.
//!
//! Threads, not the async runtime of `iris.sock`: the supervisor and the
//! reap threads of the sessions are threads too, and a session has few
//! connections.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use iris_session::peer;

use crate::socket::{code, Failure, MAX_LINE, READ_WAIT, WRITE_WAIT};

/// The most connections of one session at the same time.
pub const MAX_CONNECTIONS: usize = 8;
/// The longest wait of the shutdown for the accepted requests.
pub const DRAIN_WAIT: Duration = Duration::from_secs(10);
/// The longest wait of the accept thread before it reads the close flag.
const ACCEPT_WAIT: Duration = Duration::from_millis(200);

/// Answers one request line: writes one answer line to the writer.
pub type Handler = dyn Fn(&[u8], &mut dyn Write) -> std::io::Result<()> + Send + Sync;
/// Gives `true` while the session is open in the store.
pub type IsOpen = dyn Fn() -> bool + Send + Sync;

/// The server of one session socket. A drop closes it with no wait.
pub struct Server {
    gate: Arc<Gate>,
    accept: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

#[derive(Default)]
struct GateState {
    closing: bool,
    /// The requests that passed the check of the session and have no
    /// answer yet.
    busy: usize,
    /// A copy of each open connection, for the close.
    streams: HashMap<u64, UnixStream>,
    next: u64,
}

impl Gate {
    fn state(&self) -> MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Server {
    /// Serves `listener` on a thread until [`Server::shutdown`].
    pub fn start(
        listener: UnixListener,
        handler: Arc<Handler>,
        is_open: Arc<IsOpen>,
    ) -> std::io::Result<Server> {
        listener.set_nonblocking(true)?;
        let gate = Arc::new(Gate::default());
        let g = gate.clone();
        let accept = std::thread::Builder::new()
            .name("session-socket".into())
            .spawn(move || accept_loop(listener, g, handler, is_open))?;
        Ok(Server {
            gate,
            accept: Some(accept),
        })
    }

    /// Step 3 of the shutdown order: take no new request, wait at most
    /// `drain` for the answer of each accepted request, then close the
    /// socket and each of its connections.
    pub fn shutdown(mut self, drain: Duration) {
        let end = Instant::now() + drain;
        let mut st = self.gate.state();
        st.closing = true;
        while st.busy > 0 {
            let Some(left) = end.checked_duration_since(Instant::now()) else {
                break;
            };
            st = self
                .gate
                .changed
                .wait_timeout(st, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        drop(st);
        self.close();
    }

    fn close(&mut self) {
        let mut st = self.gate.state();
        st.closing = true;
        for s in st.streams.values() {
            let _ = s.shutdown(Shutdown::Both);
        }
        drop(st);
        // The accept thread reads the flag within ACCEPT_WAIT, then drops
        // the listener.
        if let Some(t) = self.accept.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.close();
    }
}

fn accept_loop(listener: UnixListener, gate: Arc<Gate>, handler: Arc<Handler>, open: Arc<IsOpen>) {
    let me = peer::own_uid();
    loop {
        let full = {
            let st = gate.state();
            if st.closing {
                return;
            }
            st.streams.len() >= MAX_CONNECTIONS
        };
        if full {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        if !peer::readable(&listener, ACCEPT_WAIT).unwrap_or(false) {
            continue;
        }
        let Ok((stream, _)) = listener.accept() else {
            continue;
        };
        // Another user: close with no answer.
        if peer::uid(&stream).ok() != Some(me) || stream.set_nonblocking(false).is_err() {
            continue;
        }
        let Ok(copy) = stream.try_clone() else {
            continue;
        };
        let id = {
            let mut st = gate.state();
            if st.closing {
                return;
            }
            st.next += 1;
            let id = st.next;
            st.streams.insert(id, copy);
            id
        };
        let (g, h, o) = (gate.clone(), handler.clone(), open.clone());
        let spawned = std::thread::Builder::new()
            .name("session-request".into())
            .spawn(move || {
                let _ = connection(stream, &g, &*h, &*o);
                g.state().streams.remove(&id);
            });
        if spawned.is_err() {
            gate.state().streams.remove(&id);
        }
    }
}

fn connection(
    stream: UnixStream,
    gate: &Gate,
    handler: &Handler,
    is_open: &IsOpen,
) -> std::io::Result<()> {
    let line = read_line(&stream)?;
    if line.is_empty() {
        return Ok(());
    }
    let mut out = std::io::BufWriter::new(Deadline {
        stream: &stream,
        end: Instant::now() + WRITE_WAIT,
    });
    if line.last() != Some(&b'\n') {
        let f = Failure {
            code: code::USAGE,
            error: "the request is longer than the limit (1.6 MiB), or has no newline".into(),
        };
        out.write_all(&crate::socket::failure_line(&f))?;
    } else if take(gate, is_open) {
        let done = handler(&line, &mut out);
        let mut st = gate.state();
        st.busy -= 1;
        gate.changed.notify_all();
        drop(st);
        done?;
    } else {
        let f = Failure {
            code: code::DENIED,
            error: "the session ended".into(),
        };
        out.write_all(&crate::socket::failure_line(&f))?;
    }
    out.flush()?;
    drop(out);
    stream.shutdown(Shutdown::Write)
}

/// The check of the session when the engine takes a request: the socket
/// is open and the session is open in the store. A request that passes
/// gets its answer before the shutdown closes the connection, for at most
/// the drain wait.
fn take(gate: &Gate, is_open: &IsOpen) -> bool {
    let mut st = gate.state();
    if st.closing || !is_open() {
        return false;
    }
    st.busy += 1;
    true
}

/// One request line, with its newline, for at most [`READ_WAIT`] and
/// [`MAX_LINE`] bytes. Empty if no byte came.
fn read_line(stream: &UnixStream) -> std::io::Result<Vec<u8>> {
    let end = Instant::now() + READ_WAIT;
    let mut line = Vec::new();
    let mut r = BufReader::new(stream.take(MAX_LINE as u64));
    loop {
        let Some(left) = end.checked_duration_since(Instant::now()) else {
            return Ok(line);
        };
        stream.set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
        match r.read_until(b'\n', &mut line) {
            Ok(_) => return Ok(line),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(line)
            }
            Err(e) => return Err(e),
        }
    }
}

/// A writer that fails after `end`: one deadline for all writes.
struct Deadline<'a> {
    stream: &'a UnixStream,
    end: Instant,
}

impl Write for Deadline<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let left = self.end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.stream.set_write_timeout(Some(left))?;
        (&mut &*self.stream).write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("iris-ss-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A server whose handler waits `wait`, then answers `{"ok":true}`.
    fn server(name: &str, wait: Duration, open: Arc<AtomicBool>) -> (Server, std::path::PathBuf) {
        let path = dir(name).join("socket");
        let listener = UnixListener::bind(&path).unwrap();
        let handler: Arc<Handler> = Arc::new(move |_, out| {
            std::thread::sleep(wait);
            out.write_all(b"{\"ok\":true}\n")
        });
        let is_open: Arc<IsOpen> = Arc::new(move || open.load(Ordering::SeqCst));
        (Server::start(listener, handler, is_open).unwrap(), path)
    }

    fn request(path: &std::path::Path) -> std::thread::JoinHandle<String> {
        let mut c = UnixStream::connect(path).unwrap();
        std::thread::spawn(move || {
            c.write_all(b"{}\n").unwrap();
            let mut out = String::new();
            let _ = c.read_to_string(&mut out);
            out
        })
    }

    #[test]
    fn the_shutdown_waits_for_an_accepted_request() {
        let open = Arc::new(AtomicBool::new(true));
        let (s, path) = server("drain", Duration::from_millis(500), open);
        let answer = request(&path);
        // The request passes the check before the shutdown starts.
        std::thread::sleep(Duration::from_millis(200));
        let t = Instant::now();
        s.shutdown(Duration::from_secs(10));
        assert!(
            t.elapsed() >= Duration::from_millis(200),
            "{:?}",
            t.elapsed()
        );
        assert_eq!(answer.join().unwrap(), "{\"ok\":true}\n");
        // The socket takes no new connection.
        assert!(UnixStream::connect(&path)
            .and_then(|mut c| {
                c.write_all(b"{}\n")?;
                let mut out = String::new();
                c.read_to_string(&mut out)?;
                Ok(out)
            })
            .map_or(true, |out| out.is_empty()));
    }

    #[test]
    fn the_shutdown_closes_a_slow_request_after_the_drain_wait() {
        let open = Arc::new(AtomicBool::new(true));
        let (s, path) = server("slow", Duration::from_secs(30), open);
        let answer = request(&path);
        std::thread::sleep(Duration::from_millis(200));
        let t = Instant::now();
        s.shutdown(Duration::from_millis(300));
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
        assert_eq!(answer.join().unwrap(), "");
    }

    #[test]
    fn a_request_of_an_ended_session_gets_code_3() {
        let open = Arc::new(AtomicBool::new(false));
        let (s, path) = server("ended", Duration::ZERO, open);
        let out = request(&path).join().unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["code"], 3, "{v}");
        s.shutdown(Duration::from_secs(1));
    }
}
