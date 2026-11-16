//! Memory budget and start time budget of the real `iris serve` program.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::time::{Duration, Instant};

mod common;

/// The messages of the synthetic mailbox for the sync budget test.
const SYNC_MESSAGES: usize = 2_000;

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
    // A thread reads, because a pipe read has no timeout.
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = send.send(BufReader::new(stderr).read_line(&mut line).map(|_| line));
    });
    let line = receive
        .recv_timeout(limit)
        .map_err(|_| format!("no address line in {limit:?}"))?
        .map_err(|e| e.to_string())?;
    Ok(line.trim().rsplit("http://").next().unwrap().to_string())
}

/// Send one GET request. Return true for a 200 answer.
/// A connect, read or write that waits more than `timeout` gives false.
fn answers_http(addr: &str, timeout: Duration) -> bool {
    let Ok(socket) = addr.parse() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&socket, timeout) else {
        return false;
    };
    if stream.set_read_timeout(Some(timeout)).is_err()
        || stream.set_write_timeout(Some(timeout)).is_err()
    {
        return false;
    }
    let request = format!("GET / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    // Read only the status: 12 reads at most, each limited by `timeout`, so the attempt always ends.
    let mut status = [0u8; 12];
    stream.write_all(request.as_bytes()).is_ok()
        && stream.read_exact(&mut status).is_ok()
        && &status == b"HTTP/1.1 200"
}

/// Send GET requests until one gets a 200 answer. Fail after `limit` from `begin`.
fn wait_for_http(addr: &str, begin: Instant, limit: Duration) -> Result<(), String> {
    while !answers_http(addr, Duration::from_millis(100)) {
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

#[test]
fn serve_stays_inside_the_memory_and_start_budget() {
    let begin = Instant::now();
    let mut child = KillOnDrop(
        Command::new(env!("CARGO_BIN_EXE_iris"))
            .args(["serve", "--addr", "127.0.0.1:0", "--data"])
            .arg(common::data_dir("budget-empty"))
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

#[test]
fn wait_for_http_fails_when_the_server_sends_bytes_and_never_closes() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            while stream.write_all(b"x").is_ok() {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    });
    let begin = Instant::now();
    assert!(wait_for_http(&addr, begin, Duration::from_millis(200)).is_err());
    assert!(begin.elapsed() < Duration::from_secs(5));
}

// shortcut: 2,000 messages in a debug build; the 100,000-message scale tests of iris-sync and iris-store cover the large load.
#[test]
fn serve_stays_inside_the_memory_budget_after_a_first_sync() {
    let fake =
        iris_fakegmail::FakeGmail::start(iris_synth::Mailbox::new(11, SYNC_MESSAGES)).unwrap();
    let tokens = common::token_server();
    let dir = common::data_dir("budget-sync");
    let store = iris_store::Store::open(&dir).unwrap();
    common::add_account(&store, &common::gmail(fake.base_url()));
    let mut child = KillOnDrop(
        Command::new(env!("CARGO_BIN_EXE_iris"))
            .args(["serve", "--addr", "127.0.0.1:0", "--data"])
            .arg(&dir)
            .env("IRIS_TEST_GMAIL_URL", fake.base_url())
            .env("IRIS_TEST_TOKEN_URI", &tokens.uri)
            // The fake has no quota. The real quota allows 100 raw fetches each minute.
            .env("IRIS_GMAIL_QUOTA", "100000000")
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let begin = Instant::now();
    let ready = "SELECT count(*) FROM account WHERE mirror_ready_at IS NOT NULL";
    while common::count(&store, ready) == 0 {
        assert!(
            begin.elapsed() < Duration::from_secs(120),
            "no first sync in 120 s"
        );
        assert!(child.0.try_wait().unwrap().is_none(), "iris serve ended");
        std::thread::sleep(Duration::from_millis(50));
    }
    let rss = rss_kib(child.0.id());
    let fetched = common::count(
        &store,
        "SELECT count(*) FROM message WHERE fetched_at IS NOT NULL",
    );
    eprintln!(
        "iris serve: first sync of {fetched} messages in {:?}, RSS {rss} KiB",
        begin.elapsed()
    );
    assert_eq!(fetched, SYNC_MESSAGES as i64);
    check(rss, Duration::ZERO, MAX_RSS_KIB, MAX_START).unwrap();
}
