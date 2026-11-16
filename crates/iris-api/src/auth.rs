//! The guard of the API (specification 2, section 8.6).
//!
//! - The `Host` header must be the bound address, and an `Origin` header
//!   must be that same origin. So a web page on another site cannot read the
//!   API, also through DNS rebinding.
//! - At each start, the engine makes a random web secret and a one-time code.
//!   `iris serve` prints the sign-in address `http://<addr>/#code=<code>`. The
//!   client page reads the code from the fragment (the browser never sends a
//!   fragment to a server) and exchanges it one time at `POST /api/session`
//!   for the secret. The client keeps the secret in the storage of its origin
//!   and sends it as `Authorization: Bearer <secret>`. A browser shares a
//!   cookie with each port of a host, but not the storage of an origin, so a
//!   server of another OS user on another loopback port never gets the
//!   secret.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

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

pub struct Auth {
    secret: String,
    code: Mutex<Option<String>>,
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
            code: Mutex::new(Some(random_hex::<16>()?)),
            hosts,
            addr,
        })
    }

    /// The sign-in address to print, `http://<addr>/#code=<code>`. `None`
    /// after the code was used.
    pub fn sign_in_url(&self) -> Option<String> {
        let code = self.code.lock().unwrap_or_else(|e| e.into_inner());
        code.as_ref()
            .map(|c| format!("http://{}/#code={c}", self.addr))
    }

    /// The `Authorization` header of a signed-in client. Only for a client
    /// in the same process, for example a test.
    pub fn bearer(&self) -> String {
        format!("Bearer {}", self.secret)
    }

    /// Use the one-time code. True one time only, for the correct code.
    fn take_code(&self, given: &str) -> bool {
        let mut code = self.code.lock().unwrap_or_else(|e| e.into_inner());
        if code.as_deref().is_some_and(|c| same(c, given)) {
            *code = None;
            true
        } else {
            false
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
                "the sign-in code is not valid or was used. Start iris serve again for a new code",
            );
        }
        return Json(serde_json::json!({ "token": auth.secret })).into_response();
    }
    if !auth.signed_in(req.headers()) {
        return crate::error(
            StatusCode::UNAUTHORIZED,
            "sign in with the address that iris serve prints",
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
