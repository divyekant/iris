//! The guard of the API (specification 2, section 8.6).
//!
//! - The `Host` header must be the bound address, and an `Origin` header
//!   must be that same origin. So a web page on another site cannot read the
//!   API, also through DNS rebinding.
//! - At each start, the engine makes a random web secret and a one-time code.
//!   `iris serve` prints the sign-in address `http://<addr>/#code=<code>`.
//!   `iris open` asks the engine for a new code ([`Auth::sign_in`],
//!   `docs/specs/2026-11-17-mac-app.md`): it works one time, for
//!   [`CODE_LIFE`]. The
//!   client page reads the code from the fragment (the browser never sends a
//!   fragment to a server) and exchanges it one time at `POST /api/session`
//!   for the secret. The client keeps the secret in the storage of its origin
//!   and sends it as `Authorization: Bearer <secret>`. A browser shares a
//!   cookie with each port of a host, but not the storage of an origin, so a
//!   server of another OS user on another loopback port never gets the
//!   secret.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    body::to_bytes,
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

/// The path of the code exchange.
pub const SIGN_IN: &str = "/api/session";

/// The life of a code of [`Auth::sign_in`]. The code of the start has no end.
pub const CODE_LIFE: Duration = Duration::from_secs(120);
/// The most codes of [`Auth::sign_in`] that wait at the same time. The code
/// of the start is not one of them.
pub const CODES_MAX: usize = 8;

struct Code {
    value: String,
    /// `None` for the code of the start.
    ends: Option<Instant>,
}

pub struct Auth {
    secret: String,
    /// The codes that wait, oldest first.
    codes: Mutex<Vec<Code>>,
    /// The values of `Host` that name the bound address.
    hosts: Vec<String>,
    addr: SocketAddr,
}

impl Auth {
    /// Make a new web secret (32 bytes) and a new one-time code (16 bytes)
    /// for a server bound to `addr`.
    pub fn new(addr: SocketAddr) -> std::io::Result<Auth> {
        let mut hosts = vec![addr.to_string()];
        if addr.ip().is_loopback() {
            hosts.push(format!("localhost:{}", addr.port()));
        }
        Ok(Auth {
            secret: random_hex::<32>()?,
            codes: Mutex::new(vec![Code {
                value: random_hex::<16>()?,
                ends: None,
            }]),
            hosts,
            addr,
        })
    }

    /// The sign-in address of the start to print, `http://<addr>/#code=<code>`.
    /// `None` after the code was used.
    pub fn sign_in_url(&self) -> Option<String> {
        let codes = self.codes.lock().unwrap_or_else(|e| e.into_inner());
        codes
            .iter()
            .find(|c| c.ends.is_none())
            .map(|c| self.url(&c.value))
    }

    /// A new sign-in address with a new one-time code, for [`CODE_LIFE`]. When
    /// [`CODES_MAX`] codes of this call wait, the oldest of them ends. The
    /// code of the start stays.
    pub fn sign_in(&self) -> std::io::Result<String> {
        self.sign_in_at(Instant::now())
    }

    fn sign_in_at(&self, now: Instant) -> std::io::Result<String> {
        let value = random_hex::<16>()?;
        let mut codes = self.codes.lock().unwrap_or_else(|e| e.into_inner());
        codes.retain(|c| live(c, now));
        if codes.iter().filter(|c| c.ends.is_some()).count() >= CODES_MAX {
            if let Some(i) = codes.iter().position(|c| c.ends.is_some()) {
                codes.remove(i);
            }
        }
        let url = self.url(&value);
        codes.push(Code {
            value,
            ends: Some(now + CODE_LIFE),
        });
        Ok(url)
    }

    fn url(&self, code: &str) -> String {
        format!("http://{}/#code={code}", self.addr)
    }

    /// The `Authorization` header of a signed-in client. Only for a client
    /// in the same process, for example a test.
    pub fn bearer(&self) -> String {
        format!("Bearer {}", self.secret)
    }

    /// Use a one-time code. True one time only, for a code that waits and
    /// did not end.
    fn take_code(&self, given: &str) -> bool {
        self.take_code_at(given, Instant::now())
    }

    fn take_code_at(&self, given: &str, now: Instant) -> bool {
        let mut codes = self.codes.lock().unwrap_or_else(|e| e.into_inner());
        codes.retain(|c| live(c, now));
        match codes.iter().position(|c| same(&c.value, given)) {
            Some(i) => {
                codes.remove(i);
                true
            }
            None => false,
        }
    }

    fn signed_in(&self, headers: &HeaderMap) -> bool {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|v| same(v, &self.secret))
    }

    /// The `Host` is the bound address, and an `Origin` is the same origin.
    fn same_origin(&self, headers: &HeaderMap) -> bool {
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
        let Some(host) = host.filter(|h| self.hosts.iter().any(|a| a.eq_ignore_ascii_case(h)))
        else {
            return false;
        };
        // The server speaks plain HTTP, so its own origin is `http://` and
        // the Host.
        match headers.get(header::ORIGIN) {
            None => true,
            Some(v) => v
                .to_str()
                .ok()
                .and_then(|o| o.strip_prefix("http://"))
                .is_some_and(|o| o.eq_ignore_ascii_case(host)),
        }
    }
}

fn live(c: &Code, now: Instant) -> bool {
    c.ends.is_none_or(|e| now < e)
}

/// Compare two values in a time that does not depend on the first change.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

fn random_hex<const N: usize>() -> std::io::Result<String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Deserialize)]
struct SignIn {
    code: String,
}

/// Check each request, answer the code exchange, and add the headers of
/// each answer.
pub(crate) async fn guard(State(auth): State<Arc<Auth>>, req: Request, next: Next) -> Response {
    let mut res = check(&auth, req, next).await;
    let h = res.headers_mut();
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

async fn check(auth: &Auth, req: Request, next: Next) -> Response {
    if !auth.same_origin(req.headers()) {
        return crate::error(
            StatusCode::FORBIDDEN,
            "only requests to the bound address are allowed",
        );
    }
    if req.uri().path() == SIGN_IN {
        if req.method() != Method::POST {
            return crate::error(StatusCode::METHOD_NOT_ALLOWED, "use POST");
        }
        let body = to_bytes(req.into_body(), 1024).await.unwrap_or_default();
        let code = serde_json::from_slice::<SignIn>(&body)
            .map(|s| s.code)
            .unwrap_or_default();
        if !auth.take_code(&code) {
            return crate::error(
                StatusCode::FORBIDDEN,
                "the sign-in code is not valid, was used or ended. Run iris open for a new address",
            );
        }
        return Json(serde_json::json!({ "token": auth.secret })).into_response();
    }
    if !auth.signed_in(req.headers()) {
        return crate::error(
            StatusCode::UNAUTHORIZED,
            "sign in with the address of iris open",
        );
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> Auth {
        Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap()
    }

    #[test]
    fn the_code_works_one_time() {
        let auth = auth();
        let url = auth.sign_in_url().unwrap();
        assert!(url.starts_with("http://127.0.0.1:4800/#code="));
        let code = url.split_once("code=").unwrap().1.to_string();
        assert!(!auth.take_code("wrong"));
        assert!(!auth.take_code(""));
        assert!(auth.take_code(&code));
        assert!(!auth.take_code(&code));
        assert_eq!(auth.sign_in_url(), None);
    }

    #[test]
    fn each_new_code_works_one_time_until_it_ends() {
        let auth = auth();
        let start = auth.sign_in_url().unwrap();
        let now = Instant::now();
        let code = |url: String| url.split_once("code=").unwrap().1.to_string();
        let a = code(auth.sign_in_at(now).unwrap());
        let b = code(auth.sign_in_at(now).unwrap());
        assert_ne!(a, b);
        // The start code stays, and each code works one time.
        assert!(auth.take_code_at(&b, now));
        assert!(!auth.take_code_at(&b, now));
        assert!(auth.take_code_at(&a, now + CODE_LIFE - Duration::from_secs(1)));
        let late = code(auth.sign_in_at(now).unwrap());
        assert!(!auth.take_code_at(&late, now + CODE_LIFE));
        // The start code has no end.
        assert!(auth.take_code_at(&code(start), now + CODE_LIFE * 100));
    }

    #[test]
    fn a_new_code_ends_the_oldest_when_the_list_is_full() {
        let auth = auth();
        let start = auth.sign_in_url().unwrap();
        let now = Instant::now();
        let codes: Vec<String> = (0..=CODES_MAX)
            .map(|_| {
                let url = auth.sign_in_at(now).unwrap();
                url.split_once("code=").unwrap().1.to_string()
            })
            .collect();
        // The code CODES_MAX + 1 ended the first code of a call, not the
        // start code.
        assert!(!auth.take_code_at(&codes[0], now));
        assert!(codes[1..].iter().all(|c| auth.take_code_at(c, now)));
        assert_eq!(auth.sign_in_url(), Some(start));
    }

    #[test]
    fn only_the_secret_signs_in() {
        let auth = auth();
        let mut h = HeaderMap::new();
        assert!(!auth.signed_in(&h));
        h.insert(header::AUTHORIZATION, "Bearer 0000".parse().unwrap());
        assert!(!auth.signed_in(&h));
        // A cookie with the secret is not a sign-in.
        let secret = auth.bearer()[7..].to_string();
        h.insert(header::AUTHORIZATION, "Basic x".parse().unwrap());
        h.insert(
            header::COOKIE,
            format!("iris_session={secret}").parse().unwrap(),
        );
        assert!(!auth.signed_in(&h));
        h.insert(header::AUTHORIZATION, auth.bearer().parse().unwrap());
        assert!(auth.signed_in(&h));
        assert_ne!(
            super::Auth::new("127.0.0.1:4800".parse().unwrap())
                .unwrap()
                .bearer(),
            auth.bearer()
        );
    }

    #[test]
    fn host_must_be_the_bound_address() {
        let auth = auth();
        let ok = |host: &str, origin: Option<&str>| {
            let mut h = HeaderMap::new();
            h.insert(header::HOST, host.parse().unwrap());
            if let Some(o) = origin {
                h.insert(header::ORIGIN, o.parse().unwrap());
            }
            auth.same_origin(&h)
        };
        assert!(ok("127.0.0.1:4800", None));
        assert!(ok("LOCALHOST:4800", Some("http://localhost:4800")));
        assert!(!ok("127.0.0.1:4801", None));
        assert!(!ok("localhost:5173", Some("http://localhost:5173")));
        assert!(!ok("[::1]:4800", None));
        assert!(!ok("127.0.0.1", None));
        assert!(!ok("evil.example", None));
        assert!(!ok("127.0.0.1:4800", Some("http://localhost:4800")));
        assert!(!ok("127.0.0.1:4800", Some("https://127.0.0.1:4800")));
        assert!(!ok("127.0.0.1:4800", Some("null")));
        assert!(!auth.same_origin(&HeaderMap::new()));
    }
}
