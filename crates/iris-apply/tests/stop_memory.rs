//! The memory of Stop does not grow with the count of open sessions (#571).
//! A test binary of its own, because the allocator counts each thread.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::time::Duration;

use iris_store::Store;

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let n = NOW.fetch_add(l.size(), SeqCst) + l.size();
            PEAK.fetch_max(n, SeqCst);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        NOW.fetch_sub(l.size(), SeqCst);
    }
}

#[global_allocator]
static A: Counting = Counting;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Open `n` sessions, Stop, and give the peak of Rust heap bytes during the
/// Stop above the heap before it.
fn stop_peak(store: &Store, n: usize) -> usize {
    for i in 0..n {
        let job = iris_job::create(store, &format!("Job {i}."), None, now()).unwrap();
        iris_job::register(store, job, "claude-code", now()).unwrap();
    }
    let base = NOW.load(SeqCst);
    PEAK.store(base, SeqCst);
    let s = iris_apply::stop_wait(store, Duration::ZERO).unwrap();
    let peak = PEAK.load(SeqCst) - base;
    assert_eq!(s.sessions, n);
    iris_apply::resume(store).unwrap();
    peak
}

#[test]
fn stop_memory_does_not_grow_with_the_sessions() {
    let dir = std::env::temp_dir().join(format!("iris-stop-memory-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let few = stop_peak(&store, 4);
    let many = stop_peak(&store, 400);
    // A list of 400 IDs is 3,200 bytes or more.
    assert!(
        many < few + 1024,
        "4 sessions: {few} bytes, 400 sessions: {many} bytes"
    );
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}
