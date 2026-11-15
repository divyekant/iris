//! Load 100,000 synthetic messages and time the search (tracker item R1-04:
//! "a search answers in less than 100 ms").
//!
//! The test is ignored in the normal run, because a debug build of SQLite is
//! slow. Run it with a release build:
//!
//! ```text
//! cargo test --release -p iris-store --test scale -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use iris_store::{Body, Header, NewMessage, Store};

const COUNT: usize = 100_000;
const BATCH: usize = 1_000;
const LIMIT: Duration = Duration::from_millis(100);

/// The value of one header line, and the body. Enough for the plain
/// ASCII messages of `iris_synth`. The real parser is tracker item R1-08.
fn parse(raw: &[u8]) -> (Header, String) {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let get = |name: &str| {
        head.split("\r\n")
            .find_map(|l| l.strip_prefix(name)?.strip_prefix(": "))
            .map(str::to_string)
    };
    let from = get("From").unwrap_or_default();
    let (name, addr) = match from.split_once(" <") {
        Some((n, a)) => (
            n.trim_matches('"').to_string(),
            a.trim_end_matches('>').to_string(),
        ),
        None => (String::new(), from),
    };
    let header = Header {
        from_addr: Some(addr),
        from_name: Some(name),
        to_addrs: get("To"),
        subject: get("Subject"),
        message_id_hdr: get("Message-ID"),
        in_reply_to: get("In-Reply-To"),
        list_id: get("List-Id"),
        ..Header::default()
    };
    // FTS5 indexes the clean text of tracker item R1-08, never attachment
    // data. Some synthetic messages have megabytes of base64 attachments, so
    // keep the body up to the first base64 part. The store cuts the rest to
    // its limit of 32,000 characters.
    let end = body
        .find("Content-Transfer-Encoding: base64")
        .unwrap_or(body.len());
    (header, body[..end].to_string())
}

fn status_kb(field: &str) -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = s.lines().find(|l| l.starts_with(field))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

#[test]
#[ignore = "slow in a debug build: run with --release -- --ignored"]
fn search_100k_under_100ms() {
    let dir = std::env::temp_dir().join(format!("iris-store-scale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .put_labels(
            acc,
            &[
                ("INBOX", "INBOX", "system"),
                ("CATEGORY_UPDATES", "Updates", "system"),
            ],
        )
        .unwrap();

    let mailbox = iris_synth::Mailbox::new(42, COUNT);
    let start = Instant::now();
    let mut batch = Vec::with_capacity(BATCH);
    for i in 0..COUNT {
        let truth = mailbox.truth(i);
        let raw = mailbox.raw(i);
        let (header, text) = parse(&raw);
        let labels = if i % 3 == 0 {
            vec!["INBOX".into()]
        } else {
            vec!["CATEGORY_UPDATES".into()]
        };
        batch.push(NewMessage {
            provider_id: format!("{i:016x}"),
            thread_provider_id: format!("{:016x}", truth.thread),
            size: Some(raw.len() as i64),
            internal_date: Some(truth.date * 1000),
            labels,
            body: Some(Body {
                subject: header.subject.clone().unwrap_or_default(),
                sender: format!(
                    "{} {}",
                    header.from_name.as_deref().unwrap_or(""),
                    header.from_addr.as_deref().unwrap_or("")
                ),
                text,
            }),
            header,
            ..NewMessage::default()
        });
        if batch.len() == BATCH {
            store.put_messages(acc, &batch).unwrap();
            batch.clear();
        }
    }
    store.put_messages(acc, &batch).unwrap();
    let load = start.elapsed();

    let queries = [
        "invoice",         // a common word
        "unsubscribe",     // in most feed mail
        "\"new sign-in\"", // a phrase
        "sign",            // a word in many messages
        "invoice after:2025-06-01 before:2025-07-01",
        "from:nobody.example.org", // a filter with no words and no match: a full scan
        "from:example.com",        // a filter that matches all
        "label:INBOX meeting",
        "to:owner", // matches all: the worst case for the sort
        "zzzqqq",   // no match
    ];
    let mut worst = Duration::ZERO;
    for q in queries {
        // The first run reads the pages from disk. Time the second run too.
        let t = Instant::now();
        let hits = store.search(q, 50).unwrap();
        let cold = t.elapsed();
        let t = Instant::now();
        store.search(q, 50).unwrap();
        let warm = t.elapsed();
        println!(
            "{q:45} {:3} hits  cold {cold:>10.2?}  warm {warm:>10.2?}",
            hits.len()
        );
        worst = worst.max(cold).max(warm);
    }
    let db = std::fs::metadata(dir.join("iris.db"))
        .map(|m| m.len())
        .unwrap_or(0);
    println!(
        "load {COUNT} messages: {load:.2?}; iris.db {} MB",
        db / 1_000_000
    );
    if let (Some(rss), Some(peak)) = (status_kb("VmRSS:"), status_kb("VmHWM:")) {
        println!(
            "resident memory now {} MB, peak {} MB",
            rss / 1024,
            peak / 1024
        );
    }
    // SAFETY: both functions only read counters of SQLite.
    let (used, high) = unsafe {
        (
            rusqlite::ffi::sqlite3_memory_used(),
            rusqlite::ffi::sqlite3_memory_highwater(0),
        )
    };
    println!(
        "SQLite memory now {} MB, high-water mark {} MB",
        used / 1_000_000,
        high / 1_000_000
    );
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(worst < LIMIT, "the slowest search took {worst:?}");
}
