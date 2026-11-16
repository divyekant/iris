//! The request body goes only to the configured URL: no redirect for any
//! provider, and no proxy for a local model (docs/specs/2026-11-16-cost-control.md).
//! This file is its own test process, because it sets `HTTP_PROXY`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use iris_model::{ApiKey, Client, Config, Price, Provider};

/// A raw HTTP server that answers each request with `answer` and counts
/// the requests.
fn serve(answer: String) -> (String, Arc<AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut len = 0;
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            let _ = r.read_exact(&mut body);
            c.fetch_add(1, Ordering::SeqCst);
            let _ = s.write_all(answer.as_bytes());
        }
    });
    (addr, count)
}

fn answer(status: &str, extra: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n{extra}content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn client(p: Provider, base_url: String) -> Client {
    Client::new(Config {
        provider: p,
        base_url,
        model: "m".into(),
        api_key: Some(ApiKey::new("test-key-not-real")),
        max_tokens: 100,
        timeout: Duration::from_secs(5),
        price: Some(Price {
            input_micro_usd_per_mtok: 1,
            output_micro_usd_per_mtok: 1,
        }),
    })
}

const OPENAI_OK: &str = r#"{"choices":[{"index":0,"message":{"role":"assistant","content":"{}"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2}}"#;

#[test]
fn no_redirect_and_no_proxy_for_a_local_model() {
    // The remote host that must get no request.
    let (remote, remote_count) = serve(answer("200 OK", "", OPENAI_OK));

    // A redirect to the remote host is not followed, for each provider.
    for p in [Provider::Anthropic, Provider::OpenAiCompatible] {
        for status in [
            "301 Moved Permanently",
            "302 Found",
            "303 See Other",
            "307 Temporary Redirect",
            "308 Permanent Redirect",
        ] {
            let location = format!("location: http://{remote}/v1/chat/completions\r\n");
            let (local, _) = serve(answer(status, &location, ""));
            let r = client(p, format!("http://{local}/v1")).ask("i", "mail text");
            assert!(r.is_err(), "{p:?} {status}: a redirect must fail");
        }
    }
    assert_eq!(
        remote_count.load(Ordering::SeqCst),
        0,
        "a redirect sent the body"
    );

    // A proxy in the environment is not used for a local model.
    let (proxy, proxy_count) = serve(answer("200 OK", "", OPENAI_OK));
    std::env::set_var("HTTP_PROXY", format!("http://{proxy}"));
    std::env::set_var("http_proxy", format!("http://{proxy}"));
    std::env::set_var("ALL_PROXY", format!("http://{proxy}"));
    std::env::remove_var("NO_PROXY");
    std::env::remove_var("no_proxy");
    let (local, local_count) = serve(answer("200 OK", "", OPENAI_OK));
    let c = client(Provider::OpenAiCompatible, format!("http://{local}/v1"));
    let reply = c.ask("i", "mail text").unwrap();
    assert!(reply.usage.known);
    assert_eq!(local_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        proxy_count.load(Ordering::SeqCst),
        0,
        "the proxy got the mail"
    );
}

#[test]
fn usage_with_no_counts_is_not_known() {
    let body = r#"{"choices":[{"index":0,"message":{"role":"assistant","content":"{}"},"finish_reason":"stop"}]}"#;
    let (local, _) = serve(answer("200 OK", "", body));
    let c = client(Provider::OpenAiCompatible, format!("http://{local}/v1"));
    let reply = c.ask("i", "mail text").unwrap();
    assert!(!reply.usage.known);
    assert_eq!(reply.usage.input_tokens, 0);
}

#[test]
fn request_bytes_bound_the_text_of_the_body() {
    let c = client(Provider::Anthropic, "http://127.0.0.1:9".into());
    let data = "é\"<>\u{1}".repeat(100);
    let n = c.request_bytes("instruction", &data);
    assert!(n >= ("instruction".len() + data.len()) as u64);
}
