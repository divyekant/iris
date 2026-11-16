//! Measurement probe: heap peak, time and RSS of `clean` on the synthetic mailbox.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

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

fn hwm() -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmHWM"))
        .unwrap_or("")
        .to_string()
}

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(20_000);
    let mb = iris_synth::Mailbox::new(1, n);
    let raws: Vec<Vec<u8>> = (0..mb.len()).map(|i| mb.raw(i)).collect();
    let bytes: usize = raws.iter().map(|r| r.len()).sum();
    let base = NOW.load(Relaxed);
    PEAK.store(base, Relaxed);
    let t = Instant::now();
    let mut chars = 0usize;
    let mut worst = 0usize;
    for r in &raws {
        let before = NOW.load(Relaxed);
        PEAK.store(before, Relaxed);
        let c = iris_clean::clean(r);
        worst = worst.max(PEAK.load(Relaxed) - before);
        chars += c.text.len() + c.subject.len() + c.attachments.len();
    }
    let el = t.elapsed();
    println!(
        "messages {n}, input {} MB, time {:?}, per message {:?}, output bytes {chars}",
        bytes / 1_000_000,
        el,
        el / n as u32
    );
    println!("worst heap peak of one clean call: {} KiB", worst / 1024);
    println!("{}", hwm());
}
