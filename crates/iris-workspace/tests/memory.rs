//! The build reads one message body at a time. A test of its own, so no
//! other test of the process moves the high-water mark of its memory.
//! Linux only: it reads the peak memory in `/proc`.
#![cfg(target_os = "linux")]

use std::fs;

use iris_job::{add_scope, create, By, Ref};
use iris_store::Store;
use iris_workspace::build;

/// The peak resident memory of the process, in KiB.
fn peak_kib() -> u64 {
    let s = fs::read_to_string("/proc/self/status").unwrap();
    let line = s.lines().find(|l| l.starts_with("VmHWM:")).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

/// One granted message in a thread with 60 messages of about 1 MB each that
/// are outside the grant: the build does not hold their bodies.
#[test]
fn a_large_thread_does_not_fill_the_memory() {
    let dir = std::env::temp_dir().join(format!("iris-ws-memory-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let store = Store::open(&dir.join("data")).unwrap();
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO account (id, provider, address) VALUES (1, 'gmail', 'me@example.com');
                 INSERT INTO thread (id, account_id, provider_id) VALUES (1, 1, 'a');
                 INSERT INTO message (id, account_id, thread_id, provider_id, internal_date, understood_at)
                   VALUES (1, 1, 1, 'm1', 1, 1);
                 INSERT INTO header (message_id, date, from_addr, to_addrs, subject)
                   VALUES (1, 1, 'dana@example.com', 'me@example.com', 'Big');
                 INSERT INTO body (message_id, subject, sender, text)
                   VALUES (1, 'Big', 'dana@example.com', 'GRANTED-TEXT');",
            )?;
            Ok(())
        })
        .unwrap();
    let job = create(&store, "Goal.", None, 1).unwrap();
    add_scope(&store, job, Ref::Thread(1), By::Person, 1).unwrap();
    // The later messages are not in the list of the ref.
    for i in 2..62 {
        store
            .write(|tx| {
                tx.execute_batch(&format!(
                    "INSERT INTO message (id, account_id, thread_id, provider_id, internal_date, understood_at)
                       VALUES ({i}, 1, 1, 'm{i}', {i}, 1);
                     INSERT INTO header (message_id, date, from_addr, to_addrs, subject)
                       VALUES ({i}, {i}, 'dana@example.com', 'me@example.com', 'Big');
                     INSERT INTO body (message_id, subject, sender, text)
                       VALUES ({i}, 'Big', 'dana@example.com', hex(zeroblob(500000)));"
                ))?;
                Ok(())
            })
            .unwrap();
    }
    let before = peak_kib();
    build(&store, job, &dir.join("ws")).unwrap();
    let grew = peak_kib().saturating_sub(before);
    let text = fs::read_to_string(dir.join("ws/threads/t-1.md")).unwrap();
    assert!(text.contains("GRANTED-TEXT") && !text.contains("0000000000"));
    let _ = fs::remove_dir_all(&dir);
    // The 60 bodies are 60 MB; one body is 1 MB.
    assert!(grew < 20 * 1024, "the peak memory grew by {grew} KiB");
}
