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

/// The highest anonymous memory (heap, stacks and other private pages that
/// the program allocates) of `iris serve` on Linux, in KiB. Issue #1011: in
/// the debug build of the tests, the pages of the program file were about
/// 33 MB of an RSS of 50 MB, and they grow with each change of the code, not
/// with the work. The store uses no mmap (`PRAGMA mmap_size = 0`), so its
/// page cache is anonymous memory too. The anonymous memory after the first sync of
/// `SYNC_MESSAGES` was 15,900 to 17,500 KiB (2026-11-25); the limit leaves
/// about 15 % over the highest value.
const MAX_ANON_KIB: u64 = 20 * 1024;

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
            "memory {rss_kib} KiB is over the limit {max_rss_kib} KiB"
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

/// The memory of a process that the budget limits, in KiB, with its limit:
/// on Linux the anonymous memory, elsewhere the RSS (`ps`) with the limit of
/// 50 MB. Linux reads `Anonymous` of `/proc/<pid>/smaps_rollup` (kernel 4.14
/// and later), else `RssAnon` of `/proc/<pid>/status` (issue #1035). The
/// debug RSS of Linux is near 50 MB, so Linux never falls back to the RSS.
fn memory_kib(pid: u32) -> (u64, u64) {
    if !cfg!(target_os = "linux") {
        return (rss_kib(pid), MAX_RSS_KIB);
    }
    let anon = linux_anon_kib(|file| std::fs::read_to_string(format!("/proc/{pid}/{file}")).ok())
        .unwrap_or_else(|e| panic!("{e}"));
    eprintln!("RSS {} KiB, anonymous {anon} KiB", rss_kib(pid));
    (anon, MAX_ANON_KIB)
}

/// The anonymous memory in KiB from the `/proc` files of one process, which
/// `read` gives by name (`None`: no such file): `Anonymous` of
/// `smaps_rollup`, else `RssAnon` of `status`. An error if a file has no
/// such line, or if neither file is there.
fn linux_anon_kib(read: impl Fn(&str) -> Option<String>) -> Result<u64, String> {
    for (file, key) in [("smaps_rollup", "Anonymous:"), ("status", "RssAnon:")] {
        if let Some(text) = read(file) {
            return kib_line(&text, key).ok_or_else(|| format!("no {key} line in {file}:\n{text}"));
        }
    }
    Err("measure not available: no /proc/<pid>/smaps_rollup and no /proc/<pid>/status".into())
}

/// The value of the line `<key> <n> kB` of a `/proc` file, for example
/// `Anonymous:` of `smaps_rollup` or `RssAnon:` of `status`. `None` if no
/// such line has a whole number of kB: the caller then fails. The `procfs`
/// crate also reads these files; one field does not need a new dependency.
fn kib_line(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|l| {
        let mut words = l.strip_prefix(key)?.split_whitespace();
        match (words.next()?.parse().ok()?, words.next(), words.next()) {
            (n, Some("kB"), None) => Some(n),
            _ => None,
        }
    })
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

/// The start time is a wall time, so the load of other tests on the machine
/// adds to it (issue #81). The test takes the fastest of at most
/// `START_TRIES` starts: a program that is always slow still fails.
const START_TRIES: usize = 3;

#[test]
fn serve_stays_inside_the_memory_and_start_budget() {
    let mut fastest: Option<(Duration, (u64, u64))> = None;
    for i in 0..START_TRIES {
        let begin = Instant::now();
        let mut child = KillOnDrop(
            Command::new(env!("CARGO_BIN_EXE_iris"))
                .args(["serve", "--addr", "127.0.0.1:0", "--data"])
                .arg(common::data_dir(&format!("budget-empty-{i}")))
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        // Stop well after the limit, so a stalled start fails and does not hang.
        let addr = read_addr(child.0.stderr.take().unwrap(), MAX_START * 10).unwrap();
        wait_for_http(&addr, begin, MAX_START * 10).unwrap();
        let start = begin.elapsed();
        let mem = memory_kib(child.0.id());
        eprintln!("iris serve: start {start:?}, memory {} KiB", mem.0);
        if fastest.is_none_or(|(f, _)| start < f) {
            fastest = Some((start, mem));
        }
        if start <= MAX_START {
            break;
        }
    }
    let (start, (mem, limit)) = fastest.unwrap();
    check(mem, start, limit, MAX_START).unwrap();
}

#[test]
fn kib_line_reads_only_a_whole_line_of_its_key() {
    let anonymous_kib = |t: &str| kib_line(t, "Anonymous:");
    assert_eq!(
        anonymous_kib("Rss: 9 kB\nAnonymous:      16924 kB\n"),
        Some(16924)
    );
    for bad in [
        "",
        "Rss: 9 kB",
        "Anonymous: kB",
        "Anonymous: -1 kB",
        "Anonymous: 1.5 kB",
        "Anonymous: 12",
        "Anonymous: 12 MB",
        "Anonymous: 12 kB x",
        "AnonHugePages: 12 kB",
    ] {
        assert_eq!(anonymous_kib(bad), None, "{bad:?}");
    }
    // Random text never panics, and a valid line among random lines is
    // found with its value.
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let alphabet = b"Anonymous: kB0123456789 \n\t-.xA";
    for _ in 0..2_000 {
        let len = (next() % 64) as usize;
        let noise: String = (0..len)
            .map(|_| alphabet[(next() % alphabet.len() as u64) as usize] as char)
            .collect();
        let _ = anonymous_kib(&noise);
        let n = next() % 10_000_000;
        let text = format!(
            "Rss: 1 kB\nAnonymous: {n} kB\n{}",
            noise.replace("Anonymous", "x")
        );
        assert_eq!(anonymous_kib(&text), Some(n), "{text:?}");
    }
    // The fallback of issue #1035: `status` has `RssAnon:` in the same form.
    assert_eq!(
        kib_line("VmRSS: 9 kB\nRssAnon:\t  812 kB\n", "RssAnon:"),
        Some(812)
    );
    if cfg!(target_os = "linux") {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        assert!(kib_line(&status, "RssAnon:").is_some(), "{status}");
    }
}

#[test]
fn check_accepts_values_inside_the_limits() {
    assert!(check(MAX_RSS_KIB, MAX_START, MAX_RSS_KIB, MAX_START).is_ok());
}

#[test]
fn check_fails_when_a_limit_is_too_low() {
    let err = check(3000, Duration::from_millis(20), 1, MAX_START).unwrap_err();
    assert!(err.contains("memory"), "{err}");
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
    let (mem, limit) = memory_kib(child.0.id());
    let fetched = common::count(
        &store,
        "SELECT count(*) FROM message WHERE fetched_at IS NOT NULL",
    );
    eprintln!(
        "iris serve: first sync of {fetched} messages in {:?}, memory {mem} KiB",
        begin.elapsed()
    );
    assert_eq!(fetched, SYNC_MESSAGES as i64);
    check(mem, Duration::ZERO, limit, MAX_START).unwrap();
}

/// Issue #1035: the choice of the file. With no `smaps_rollup`, the value
/// is `RssAnon` of `status`, with the anonymous limit. With neither file,
/// the measure fails: Linux never falls back to the RSS.
#[test]
fn linux_anon_kib_falls_back_to_status_and_fails_with_no_file() {
    let files = |smaps: Option<&str>, status: Option<&str>| {
        let (smaps, status) = (smaps.map(str::to_string), status.map(str::to_string));
        move |f: &str| match f {
            "smaps_rollup" => smaps.clone(),
            "status" => status.clone(),
            _ => None,
        }
    };
    let status = "VmRSS:\t 48000 kB\nRssAnon:\t 16100 kB\n";
    assert_eq!(
        linux_anon_kib(files(Some("Anonymous: 15900 kB\n"), Some(status))),
        Ok(15_900)
    );
    assert_eq!(linux_anon_kib(files(None, Some(status))), Ok(16_100));
    assert!(linux_anon_kib(files(None, None))
        .unwrap_err()
        .contains("measure not available"));
    // A file with no line of its key fails; it does not try the next file.
    assert!(linux_anon_kib(files(Some("Rss: 1 kB\n"), Some(status))).is_err());
    assert!(linux_anon_kib(files(None, Some("VmRSS: 1 kB\n"))).is_err());
    // The value of status is checked against the anonymous limit.
    let (kib, limit) = (
        linux_anon_kib(files(None, Some(status))).unwrap(),
        MAX_ANON_KIB,
    );
    assert!(check(kib, MAX_START, limit, MAX_START).is_ok());
    assert!(check(MAX_ANON_KIB + 1, MAX_START, limit, MAX_START).is_err());
}
