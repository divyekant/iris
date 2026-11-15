//! Memory budget and start time budget of the real `iris serve` program.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::time::{Duration, Instant};

/// The highest resident memory (RSS) that `iris serve` can use, in KiB.
/// The base decision of Iris is the lightest memory footprint: 50 MB.
const MAX_RSS_KIB: u64 = 50 * 1024;

/// The longest time from the program start to the first HTTP answer.
const MAX_START: Duration = Duration::from_secs(1);

/// Compare the measured values with the limits. Return each limit that the program exceeds.
fn check(
    rss_kib: u64,
    start: Duration,
    max_rss_kib: u64,
    max_start: Duration,
) -> Result<(), String> {
    let mut over = Vec::new();
    if rss_kib > max_rss_kib {
        over.push(format!(
            "RSS {rss_kib} KiB is over the limit {max_rss_kib} KiB"
        ));
    }
    if start > max_start {
        over.push(format!(
            "start time {start:?} is over the limit {max_start:?}"
        ));
    }
    if over.is_empty() {
        Ok(())
    } else {
        Err(over.join("; "))
    }
}

/// Read the RSS of a process in KiB. `ps -o rss=` works on macOS and Linux.
fn rss_kib(pid: u32) -> u64 {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .expect("run ps");
    assert!(out.status.success(), "ps failed: {out:?}");
    String::from_utf8(out.stdout)
        .unwrap()
        .trim()
        .parse()
        .expect("ps printed a number")
}

/// Read the address from the first line of the program. Fail after `limit`.
fn read_addr(stderr: ChildStderr, limit: Duration) -> Result<String, String> {
    let _ = limit;
    let mut line = String::new();
    BufReader::new(stderr)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line.trim().rsplit("http://").next().unwrap().to_string())
}

/// Send one GET request. Return true for a 200 answer.
fn answers_http(addr: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(addr) else {
        return false;
    };
    let request = format!("GET / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    let mut response = String::new();
    stream.write_all(request.as_bytes()).is_ok()
        && stream.read_to_string(&mut response).is_ok()
        && response.starts_with("HTTP/1.1 200")
}

/// Send GET requests until one gets a 200 answer. Fail after `limit` from `begin`.
fn wait_for_http(addr: &str, begin: Instant, limit: Duration) -> Result<(), String> {
    while !answers_http(addr) {
        if begin.elapsed() > limit {
            return Err(format!("no HTTP answer from {addr} in {limit:?}"));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

/// Kill the child process when the test ends, also when an assert fails.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// shortcut: measures an empty program with no store; add a load of 100,000 messages when the store exists.
#[test]
fn serve_stays_inside_the_memory_and_start_budget() {
    let begin = Instant::now();
    let mut child = KillOnDrop(
        Command::new(env!("CARGO_BIN_EXE_iris"))
            .args(["serve", "--addr", "127.0.0.1:0"])
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    // Stop well after the limit, so a stalled start fails and does not hang.
    let addr = read_addr(child.0.stderr.take().unwrap(), MAX_START * 10).unwrap();
    wait_for_http(&addr, begin, MAX_START * 10).unwrap();
    let start = begin.elapsed();
    let rss = rss_kib(child.0.id());

    eprintln!("iris serve: start {start:?}, RSS {rss} KiB");
    check(rss, start, MAX_RSS_KIB, MAX_START).unwrap();
}

#[test]
fn check_accepts_values_inside_the_limits() {
    assert!(check(MAX_RSS_KIB, MAX_START, MAX_RSS_KIB, MAX_START).is_ok());
}

#[test]
fn check_fails_when_a_limit_is_too_low() {
    let err = check(3000, Duration::from_millis(20), 1, MAX_START).unwrap_err();
    assert!(err.contains("RSS"), "{err}");
    let err = check(
        3000,
        Duration::from_millis(20),
        MAX_RSS_KIB,
        Duration::from_millis(1),
    )
    .unwrap_err();
    assert!(err.contains("start time"), "{err}");
}

#[test]
fn read_addr_fails_when_the_program_prints_nothing() {
    let mut silent = KillOnDrop(
        Command::new("sleep")
            .arg("30")
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let begin = Instant::now();
    let stderr = silent.0.stderr.take().unwrap();
    assert!(read_addr(stderr, Duration::from_millis(200)).is_err());
    assert!(begin.elapsed() < Duration::from_secs(5));
}

#[test]
fn wait_for_http_fails_when_the_server_never_answers() {
    // The kernel accepts the connection, but nothing reads or answers it.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let begin = Instant::now();
    assert!(wait_for_http(&addr, begin, Duration::from_millis(200)).is_err());
    assert!(begin.elapsed() < Duration::from_secs(5));
}
