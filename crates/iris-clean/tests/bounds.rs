//! Memory and time bounds for one big message. This file is its own test
//! program, so the counting allocator sees only this test.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let n = NOW.fetch_add(l.size(), Relaxed) + l.size();
        PEAK.fetch_max(n, Relaxed);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        NOW.fetch_sub(l.size(), Relaxed);
        System.dealloc(p, l)
    }
}

#[global_allocator]
static A: Counting = Counting;

/// A message with a big HTML body and a big attachment.
fn message(html_bytes: usize, attachment_bytes: usize) -> Vec<u8> {
    let mut m = b"Subject: Big\r\nContent-Type: multipart/mixed; boundary=\"m\"\r\n\r\n\
                  --m\r\nContent-Type: text/html\r\n\r\n"
        .to_vec();
    let row = b"<tr><td style=\"padding:4px\">Item <a href=\"https://s.example.com/?utm_a=1\">link</a></td></tr>\r\n";
    while m.len() < html_bytes {
        m.extend_from_slice(row);
    }
    m.extend_from_slice(b"--m\r\nContent-Type: application/pdf; name=\"a.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\n");
    let line = [b'Q'; 76];
    for _ in 0..attachment_bytes / 57 {
        m.extend_from_slice(&line);
        m.extend_from_slice(b"\r\n");
    }
    m.extend_from_slice(b"--m--\r\n");
    m
}

/// The peak of new heap memory during one call, in bytes.
fn peak_of(raw: &[u8]) -> (usize, Duration, iris_clean::Clean) {
    let base = NOW.load(Relaxed);
    PEAK.store(base, Relaxed);
    let start = Instant::now();
    let c = iris_clean::clean(raw);
    (PEAK.load(Relaxed) - base, start.elapsed(), c)
}

#[test]
fn memory_and_time_stay_bounded() {
    let small = message(200_000, 1_000_000);
    let big = message(6_000_000, 30_000_000);
    let (small_peak, _, _) = peak_of(&small);
    let (big_peak, time, c) = peak_of(&big);
    eprintln!("peak: {small_peak} bytes (small), {big_peak} bytes (big), time {time:?}");
    assert!(c.cut && c.text.ends_with(iris_clean::CUT_MARK));
    assert_eq!(c.attachments[0].size, 30_000_000 / 57 * 57);
    // The body budget is 2 MiB. The steps hold a few copies of it at most. Measured: about 4 MiB.
    assert!(big_peak < 8 * 1024 * 1024, "peak {big_peak}");
    // The size of an attachment does not change the memory use.
    assert!(big_peak < small_peak + 12 * 1024 * 1024);
    // Debug builds are slow. The limit holds in a debug build on a small machine.
    assert!(time < Duration::from_secs(10), "time {time:?}");
}
