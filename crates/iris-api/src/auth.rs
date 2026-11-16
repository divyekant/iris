//! The sign-in of the web client (specification 2, section 8.6).
//!
//! At each start, the engine makes a random web secret and a one-time code.
//! `iris serve` prints the sign-in address with the code. The address gives
//! the browser a cookie with the secret (`HttpOnly`, `SameSite=Strict`). The
//! API refuses each other request without that cookie, so another OS user on
//! the same machine cannot read the mail through the API.

use std::sync::Mutex;

use axum::{
    extract::{Query, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

/// The name of the cookie.
const COOKIE: &str = "iris_session";

/// The path of the sign-in address.
pub const SIGN_IN: &str = "/api/session";

pub struct Auth {
    secret: String,
    code: Mutex<Option<String>>,
}

impl Auth {
    /// Make a new web secret (32 bytes) and a new one-time code (16 bytes).
    pub fn new() -> std::io::Result<Auth> {
        Ok(Auth {
            secret: random_hex::<32>()?,
            code: Mutex::new(Some(random_hex::<16>()?)),
        })
    }

    /// The path and query of the sign-in address, for example
    /// `/api/session?code=…`. `None` after the code was used.
    pub fn sign_in_path(&self) -> Option<String> {
        let code = self.code.lock().unwrap_or_else(|e| e.into_inner());
        code.as_ref().map(|c| format!("{SIGN_IN}?code={c}"))
    }

    /// The `Cookie` header that the sign-in gives the browser. Only for a
    /// client in the same process, for example a test.
    pub fn cookie_header(&self) -> String {
        format!("{COOKIE}={}", self.secret)
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
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .any(|(name, value)| name == COOKIE && same(value, &self.secret))
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
    code: Option<String>,
}

/// Refuse each request without the cookie, except the sign-in address.
pub(crate) async fn signed_in(
    State(auth): State<std::sync::Arc<Auth>>,
    req: Request,
    next: Next,
) -> Response {
    if req.uri().path() == SIGN_IN {
        let code = Query::<SignIn>::try_from_uri(req.uri())
            .ok()
            .and_then(|q| q.0.code)
            .unwrap_or_default();
        if !auth.take_code(&code) {
            return crate::error(
                StatusCode::FORBIDDEN,
                "the sign-in code is not valid or was used. Start iris serve again for a new code",
            );
        }
        let cookie = format!(
            "{}; HttpOnly; SameSite=Strict; Path=/",
            auth.cookie_header()
        );
        let mut res = (StatusCode::SEE_OTHER, [(header::LOCATION, "/")]).into_response();
        if let Ok(v) = HeaderValue::from_str(&cookie) {
            res.headers_mut().insert(header::SET_COOKIE, v);
        }
        return res;
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

    #[test]
    fn the_code_works_one_time() {
        let auth = Auth::new().unwrap();
        let path = auth.sign_in_path().unwrap();
        let code = path.split_once("code=").unwrap().1.to_string();
        assert!(!auth.take_code("wrong"));
        assert!(!auth.take_code(""));
        assert!(auth.take_code(&code));
        assert!(!auth.take_code(&code));
        assert_eq!(auth.sign_in_path(), None);
    }

    #[test]
    fn only_the_secret_signs_in() {
        let auth = Auth::new().unwrap();
        let mut h = HeaderMap::new();
        assert!(!auth.signed_in(&h));
        h.insert(
            header::COOKIE,
            "iris_session=0000; other=1".parse().unwrap(),
        );
        assert!(!auth.signed_in(&h));
        h.insert(
            header::COOKIE,
            format!("other=1; {}", auth.cookie_header())
                .parse()
                .unwrap(),
        );
        assert!(auth.signed_in(&h));
        assert_ne!(Auth::new().unwrap().cookie_header(), auth.cookie_header());
    }
}
