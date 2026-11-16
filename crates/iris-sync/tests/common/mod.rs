//! Helpers of the sync tests.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use iris_fakegmail::FakeGmail;
use iris_gmail::{Config, Gmail};
use iris_store::Store;

pub const TOKEN: &str = "test-token";
pub const OWNER: &str = "owner@example.com";

pub fn client(base_url: String) -> Gmail {
    Gmail::new(Config {
        base_url,
        quota_per_minute: 10_000_000,
        backoff_start: Duration::from_millis(1),
        backoff_max: Duration::from_millis(4),
        ..Config::default()
    })
}

pub fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iris-sync-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

pub fn count(store: &Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

pub fn raw_requests(fake: &FakeGmail) -> usize {
    fake.requests()
        .iter()
        .filter(|r| r.contains("format=raw"))
        .count()
}

/// The fake as the mirror must see it: (message ID, sorted label IDs).
pub fn fake_state(fake: &FakeGmail) -> Vec<(String, Vec<String>)> {
    let gmail = client(fake.base_url());
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let p = gmail.messages(TOKEN, &[], true, page.as_deref()).unwrap();
        for m in p.messages {
            let mut labels = gmail.minimal(TOKEN, &m.id).unwrap().label_ids;
            labels.sort();
            out.push((m.id, labels));
        }
        match p.next_page_token {
            Some(t) => page = Some(t),
            None => break,
        }
    }
    out.sort();
    out
}

pub fn mirror_state(store: &Store) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = store
        .read(|c| {
            let mut st = c.prepare(
                "SELECT m.provider_id, coalesce(group_concat(l.provider_id, ' '), '')
                 FROM message m LEFT JOIN message_label ml ON ml.message_id = m.id
                 LEFT JOIN label l ON l.id = ml.label_id
                 WHERE m.gone_at IS NULL GROUP BY m.id",
            )?;
            let rows = st.query_map([], |r| {
                let labels: String = r.get(1)?;
                let mut labels: Vec<String> =
                    labels.split_whitespace().map(str::to_string).collect();
                labels.sort();
                Ok((r.get(0)?, labels))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    out.sort();
    out
}

pub fn sync_state(store: &Store) -> (Option<String>, Option<String>, Option<i64>, Option<String>) {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT sync_kind, sync_start, sync_step, history_id FROM account",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap()
}

/// A TCP proxy in front of the fake. When a request that contains
/// `pattern` comes first, it runs `hook` before it sends the request on.
pub fn hook_proxy(
    target: std::net::SocketAddr,
    pattern: &'static str,
    hook: impl Fn() + Send + Sync + 'static,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hook = Arc::new(hook);
    let fired = Arc::new(AtomicBool::new(false));
    std::thread::spawn(move || {
        for client in listener.incoming() {
            let Ok(client) = client else { continue };
            let server = TcpStream::connect(target).unwrap();
            let (mut s_read, mut c_write) =
                (server.try_clone().unwrap(), client.try_clone().unwrap());
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut s_read, &mut c_write);
                let _ = c_write.shutdown(Shutdown::Both);
            });
            let (hook, fired) = (hook.clone(), fired.clone());
            std::thread::spawn(move || {
                let (mut c_read, mut s_write) = (client, server);
                let mut buf = [0u8; 8192];
                loop {
                    let n = match c_read.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let hit = buf[..n]
                        .windows(pattern.len())
                        .any(|w| w == pattern.as_bytes());
                    if hit && !fired.swap(true, Ordering::SeqCst) {
                        hook();
                    }
                    if s_write.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
                let _ = s_write.shutdown(Shutdown::Both);
            });
        }
    });
    format!("http://{addr}/gmail/v1")
}
