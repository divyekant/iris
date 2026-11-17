use super::*;
use crate::isolation::test::{Call, TestIsolation};
use std::io::{BufRead, BufReader, PipeWriter};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

/// A log writer that the test can read after the hold.
#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl Write for Buf {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Buf {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

struct Run {
    input: Option<PipeWriter>,
    status: BufReader<io::PipeReader>,
    log: Buf,
    isolation: Arc<TestIsolation>,
    done: thread::JoinHandle<i32>,
}

impl Run {
    fn line(&mut self) -> String {
        let mut s = String::new();
        self.status.read_line(&mut s).unwrap();
        s.trim_end().to_string()
    }
    fn send(&mut self, text: &str) {
        self.input
            .as_mut()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    /// Closes the pipe from the engine, as an engine stop does.
    fn close(&mut self) {
        self.input = None;
    }
    fn end(self) -> (i32, String, Vec<Call>) {
        drop(self.input);
        let code = self.done.join().unwrap();
        let calls = self.isolation.calls.lock().unwrap().clone();
        (code, self.log.text(), calls)
    }
}

fn run(script: &str, term_wait: Duration) -> Run {
    run_with(script, term_wait, Duration::ZERO)
}

fn run_with(script: &str, term_wait: Duration, drain_delay: Duration) -> Run {
    let (in_r, in_w) = io::pipe().unwrap();
    let (st_r, st_w) = io::pipe().unwrap();
    let log = Buf::default();
    let isolation = Arc::new(TestIsolation::default());
    let command = Command {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".into(), script.into()],
    };
    let (iso, out) = (isolation.clone(), log.clone());
    let done = thread::spawn(move || {
        hold_with(&*iso, 7, &command, in_r, st_w, out, term_wait, drain_delay)
    });
    Run {
        input: Some(in_w),
        status: BufReader::new(st_r),
        log,
        isolation,
        done,
    }
}

fn pid_of(started: &str) -> i32 {
    started.strip_prefix("started ").unwrap().parse().unwrap()
}

/// The state letter of a process in /proc, or None if it is gone. Linux
/// only: on macOS it is always None.
fn state(pid: i32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit(')').next()?.trim_start().chars().next()
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

#[test]
fn no_start_line_runs_no_agent() {
    for first in ["", "begin\n", "start now\n", "START\n"] {
        let marker = std::env::temp_dir().join(format!("iris-hold-nostart-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let mut r = run(
            &format!("touch {}", marker.display()),
            Duration::from_millis(100),
        );
        r.send(first);
        r.close();
        let (code, log, calls) = r.end();
        assert_eq!(code, 2, "first line {first:?}");
        assert!(log.contains("no start"));
        assert!(calls.is_empty());
        assert!(!marker.exists(), "the agent ran after {first:?}");
    }
}

#[test]
fn a_long_first_line_is_refused() {
    let mut r = run("exit 0", Duration::from_millis(100));
    r.send(&"s".repeat(LINE_MAX + 10));
    r.send("\n");
    assert_eq!(r.end().0, 2);
}

#[test]
fn an_exit_keeps_a_zombie_until_the_last_signal_then_reaps() {
    let mut r = run(
        "echo IRIS-MARKER-OUT; echo IRIS-MARKER-ERR >&2; exit 3",
        Duration::from_secs(1),
    );
    r.send("start\n");
    let started = r.line();
    let pid = pid_of(&started);
    // The agent exits at once. Inside the wait, it must stay a zombie: its
    // process ID is not free for another process.
    thread::sleep(Duration::from_millis(400));
    assert!(alive(pid));
    if cfg!(target_os = "linux") {
        assert_eq!(state(pid), Some('Z'));
    }
    assert_eq!(r.line(), "exited 3");
    let (code, log, calls) = r.end();
    assert_eq!(code, 0);
    assert!(!alive(pid), "the reap removes the zombie");
    assert_eq!(calls, vec![Call::Stop(7)]);
    let order = [
        "started, process group",
        "the agent exited",
        "SIGTERM to the process group",
        "SIGKILL to the process group",
        "isolation stop: ok",
        "exited 3;",
    ];
    let mut at = 0;
    for part in order {
        let i = log[at..]
            .find(part)
            .unwrap_or_else(|| panic!("{part:?} not in order in {log}"));
        at += i + part.len();
    }
    assert!(
        !log.contains("IRIS-MARKER"),
        "a byte of the agent is in the log"
    );
}

#[test]
fn the_end_of_the_pipe_stops_the_agent_with_sigterm() {
    let mut r = run("sleep 100", Duration::from_secs(1));
    r.send("start\n");
    let pid = pid_of(&r.line());
    r.close();
    let t = Instant::now();
    assert_eq!(r.line(), format!("signaled {}", libc::SIGTERM));
    assert!(t.elapsed() < Duration::from_secs(10));
    let (code, log, _) = r.end();
    assert_eq!(code, 0);
    assert!(log.contains("the pipe from the engine ended"));
    assert!(!alive(pid));
}

#[test]
fn an_agent_that_ignores_sigterm_gets_sigkill() {
    let ready = std::env::temp_dir().join(format!("iris-hold-trap-{}", std::process::id()));
    let _ = std::fs::remove_file(&ready);
    let script = format!("trap '' TERM; touch {}; sleep 100 & wait", ready.display());
    let mut r = run(&script, Duration::from_millis(500));
    r.send("start\n");
    r.line();
    // Close the pipe only after the trap is set.
    let t = Instant::now();
    while !ready.exists() {
        assert!(t.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
    }
    let _ = std::fs::remove_file(&ready);
    r.close();
    assert_eq!(r.line(), format!("signaled {}", libc::SIGKILL));
    assert_eq!(r.end().0, 0);
}

#[test]
fn the_whole_group_stops_with_the_agent() {
    let file = std::env::temp_dir().join(format!("iris-hold-bg-{}", std::process::id()));
    let _ = std::fs::remove_file(&file);
    let script = format!("sleep 100 & echo $! > {}; exit 0", file.display());
    let mut r = run(&script, Duration::from_millis(300));
    r.send("start\n");
    r.line();
    assert_eq!(r.line(), "exited 0");
    r.end();
    let bg: i32 = std::fs::read_to_string(&file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let _ = std::fs::remove_file(&file);
    // The child of the agent got SIGKILL. Its new parent reaps it, so it is
    // gone or, for a short time, a zombie.
    let t = Instant::now();
    while state(bg).is_some_and(|s| s != 'Z') {
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "the child of the agent still runs"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_output_is_read_and_dropped() {
    let mut r = run(
        "head -c 300000 /dev/zero; exit 0",
        Duration::from_millis(200),
    );
    r.send("start\n");
    r.line();
    assert_eq!(r.line(), "exited 0");
    let (_, log, _) = r.end();
    assert!(log.contains("; 300000 bytes of output dropped"), "{log}");
    assert!(log.len() < 1024);
}

#[test]
fn a_failed_spawn_writes_failed() {
    let (in_r, mut in_w) = io::pipe().unwrap();
    let (st_r, st_w) = io::pipe().unwrap();
    let log = Buf::default();
    let command = Command {
        program: PathBuf::from("/nonexistent/iris-agent"),
        args: vec![],
    };
    in_w.write_all(b"start\n").unwrap();
    let code = hold_with(
        &TestIsolation::default(),
        7,
        &command,
        in_r,
        st_w,
        log.clone(),
        Duration::ZERO,
        Duration::ZERO,
    );
    assert_eq!(code, 1);
    let mut s = String::new();
    BufReader::new(st_r).read_line(&mut s).unwrap();
    assert_eq!(s, "failed\n");
}

#[test]
fn a_failed_isolation_stop_is_logged_and_the_reap_still_runs() {
    let mut r = run("exit 0", Duration::from_millis(100));
    *r.isolation.fail_stops.lock().unwrap() = 1;
    r.send("start\n");
    r.line();
    assert_eq!(r.line(), "exited 0");
    let (code, log, _) = r.end();
    assert_eq!(code, 0);
    assert!(log.contains("isolation stop: failed"));
}

#[test]
fn the_log_stays_inside_its_limit() {
    let mut log = Log {
        out: Vec::new(),
        written: 0,
    };
    for _ in 0..10_000 {
        log.line("SIGTERM to the process group");
    }
    assert!(log.out.len() as u64 <= LOG_LIMIT);
    assert!(log.out.ends_with(b"\n"));
}

#[test]
fn the_count_holds_each_byte_that_the_agent_wrote() {
    // The output thread starts late, so the bytes are still in the pipe
    // when the agent is reaped.
    let mut r = run_with(
        "head -c 1000 /dev/zero; exit 0",
        Duration::ZERO,
        Duration::from_millis(300),
    );
    r.send("start\n");
    r.line();
    assert_eq!(r.line(), "exited 0");
    let (_, log, _) = r.end();
    assert!(log.contains("; 1000 bytes of output dropped"), "{log}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_process_that_keeps_the_pipe_open_gives_a_lower_bound() {
    // setsid moves the child out of the group, and the test isolation
    // stops nothing, so the child keeps the output pipe open.
    let ready = std::env::temp_dir().join(format!("iris-hold-setsid-{}", std::process::id()));
    let _ = std::fs::remove_file(&ready);
    // The agent exits only after the child left the group.
    let script = format!(
        "setsid sh -c 'touch {f}; exec sleep 5' & while [ ! -e {f} ]; do sleep 0.05; done; exit 0",
        f = ready.display()
    );
    let mut r = run(&script, Duration::ZERO);
    r.send("start\n");
    r.line();
    assert_eq!(r.line(), "exited 0");
    let (code, log, _) = r.end();
    assert_eq!(code, 0);
    assert!(
        log.contains("at least 0 bytes of output dropped, the output pipe is still open"),
        "{log}"
    );
}
