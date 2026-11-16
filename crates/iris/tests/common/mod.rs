//! Helpers of the engine tests. All tokens and client values are made up.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iris_store::Store;

pub const CLIENT_SECRET: &str = "fake-client-secret-77";
pub const REFRESH: &str = "fake-refresh-token-22";

pub fn client_json() -> String {
    format!(
        r#"{{"installed":{{"client_id":"fake-id.apps.googleusercontent.com",
        "client_secret":"{CLIENT_SECRET}","redirect_uris":["http://localhost"]}}}}"#
    )
}

/// A good token answer, with a refresh token.
pub fn tokens_answer() -> (u16, String) {
    (
        200,
        format!(
            r#"{{"access_token":"fake-access","expires_in":3600,"refresh_token":"{REFRESH}",
            "scope":"{}","token_type":"Bearer"}}"#,
            iris_auth::SCOPE
        ),
    )
}

/// A token endpoint on a random port. Each request takes the next queued
/// answer, or the good token answer when the queue is empty.
pub struct TokenServer {
    pub uri: String,
    pub answers: Arc<Mutex<VecDeque<(u16, String)>>>,
    pub calls: Arc<Mutex<Vec<String>>>,
}

pub fn token_server() -> TokenServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!("http://{}/token", listener.local_addr().unwrap());
    let answers: Arc<Mutex<VecDeque<(u16, String)>>> = Arc::default();
    let calls: Arc<Mutex<Vec<String>>> = Arc::default();
    let (a, c) = (answers.clone(), calls.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = BufReader::new(stream.unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                stream.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            c.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&body).into_owned());
            let (status, body) = a.lock().unwrap().pop_front().unwrap_or_else(tokens_answer);
            let _ = write!(
                stream.get_mut(),
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    TokenServer {
        uri,
        answers,
        calls,
    }
}

pub fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iris-engine-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

pub fn gmail(base_url: String) -> iris_gmail::Gmail {
    iris_gmail::Gmail::new(iris_gmail::Config {
        base_url,
        quota_per_minute: 10_000_000,
        backoff_start: Duration::from_millis(1),
        backoff_max: Duration::from_millis(4),
        ..iris_gmail::Config::default()
    })
}

/// Add the synthetic owner account as `iris account add` does, without a browser.
pub fn add_account(store: &Store, gmail: &iris_gmail::Gmail) -> i64 {
    let tokens = iris_auth::Tokens {
        access: iris_auth::AccessToken {
            token: iris_auth::Secret::new("fake-access"),
            expires_at: std::time::Instant::now() + Duration::from_secs(3600),
        },
        refresh: iris_auth::Secret::new(REFRESH),
        refresh_expires_in: None,
    };
    iris::engine::save_client(store, client_json().as_bytes()).unwrap();
    iris::engine::save_sign_in(store, gmail, &tokens).unwrap().0
}

pub fn count(store: &Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}
