//! The peak memory of a first sync, a full resync and a verify of 100,000 messages (specification 1,
//! section 9).
//!
//! The fake Gmail server runs in a child process, so the peak memory of this
//! process is the sync, the client and the store only. The test is ignored
//! in the normal run. Run it on Linux with a release build:
//!
//! ```text
//! cargo test --release -p iris-sync --test scale -- --ignored --nocapture peak
//! ```

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use iris_fakegmail::FakeGmail;
use iris_gmail::{Config, Gmail};
use iris_store::Store;
use iris_synth::Mailbox;

const COUNT: usize = 100_000;
const SEED: u64 = 42;

/// The child process: start the fake, print its URL, and wait for the end of stdin.
#[test]
#[ignore]
fn fake_server() {
    if std::env::var_os("IRIS_SYNC_FAKE").is_none() {
        return;
    }
    let fake = FakeGmail::start(Mailbox::new(SEED, COUNT)).unwrap();
    println!("FAKE {}", fake.base_url());
    let mut line = String::new();
    while std::io::stdin().read_line(&mut line).is_ok_and(|n| n > 0) {}
}

fn status_kb(field: &str) -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = s.lines().find(|l| l.starts_with(field))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn report_line(what: &str, time: Duration, before: Option<u64>) {
    println!("{what}: {COUNT} messages in {time:?}");
    if let (Some(rss), Some(peak), Some(before)) =
        (status_kb("VmRSS:"), status_kb("VmHWM:"), before)
    {
        println!(
            "{what}: resident memory before {} MB, after {} MB, peak {} MB",
            before / 1024,
            rss / 1024,
            peak / 1024
        );
    }
}

#[test]
#[ignore]
fn peak_memory_of_a_first_sync() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fake_server", "--ignored", "--nocapture"])
        .env("IRIS_SYNC_FAKE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    // Keep the pipe open: the child dies on a write to a closed pipe.
    let mut out = BufReader::new(child.stdout.take().unwrap()).lines();
    let url = out
        .by_ref()
        .map(Result::unwrap)
        .find_map(|l| l.strip_prefix("FAKE ").map(str::to_string))
        .unwrap();

    let dir = std::env::temp_dir().join(format!("iris-sync-scale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("gmail", "owner@example.com").unwrap();
    let gmail = Gmail::new(Config {
        base_url: url,
        quota_per_minute: u32::MAX,
        backoff_start: Duration::from_millis(1),
        ..Config::default()
    });
    let before = status_kb("VmRSS:");
    let start = Instant::now();
    let done = iris_sync::sync(&store, &gmail, account, "test-token").unwrap();
    report_line("first sync", start.elapsed(), before);
    assert_eq!(done.fetched as usize, COUNT);

    // A full resync of the same mailbox (specification 1, section 4.4).
    store
        .write(|tx| {
            tx.execute("UPDATE account SET sync_kind = 'resync'", [])?;
            Ok(())
        })
        .unwrap();
    // Linux: "5" sets the peak (VmHWM) to the resident memory of now.
    let _ = std::fs::write("/proc/self/clear_refs", "5");
    let before = status_kb("VmRSS:");
    let start = Instant::now();
    let done = iris_sync::sync(&store, &gmail, account, "test-token").unwrap();
    report_line("full resync", start.elapsed(), before);
    assert!(done.resync);
    assert_eq!(done.fetched, 0);

    // Verify of the same mailbox (specification 1, section 4.8).
    let _ = std::fs::write("/proc/self/clear_refs", "5");
    let before = status_kb("VmRSS:");
    let start = Instant::now();
    let report = iris_sync::verify(&store, &gmail, account, "test-token").unwrap();
    report_line("verify", start.elapsed(), before);
    assert_eq!(report.differences, vec![]);
    drop(child.stdin.take());
    let _ = child.wait();
    drop(out);
    let _ = std::fs::remove_dir_all(&dir);
}
