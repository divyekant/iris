//! Google sign-in for Iris (specification 1, section 4.9).
//!
//! - The user gives the JSON file of an own OAuth client of the type "Desktop
//!   app" (decision record `docs/decisions/2026-11-google-sign-in.md`).
//! - The sign-in uses the loopback redirect on `127.0.0.1` with PKCE (S256)
//!   and asks only for [`SCOPE`].
//! - The access token stays in memory. The caller seals the refresh token with
//!   the secret box of the store (`iris_store::Store::seal`) and keeps the blob.
//! - No secret goes into a `Debug` text, a `Display` text or an error.

use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The only scope of release 1.
pub const SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";

/// The Google endpoints. Iris uses only these, never the endpoints of the
/// client file, so a changed file cannot send the tokens to another host.
pub const GOOGLE_AUTH_URI: &str = "https://accounts.google.com/o/oauth2/auth";
pub const GOOGLE_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// The path of the loopback redirect.
pub const CALLBACK: &str = "/oauth/callback";

/// Largest answer body that the token endpoint can give.
const BODY_LIMIT: u64 = 64 * 1024;
/// Largest request line that the loopback listener reads.
const REQUEST_LIMIT: usize = 8 * 1024;

/// A secret value: a token, a client secret or a PKCE verifier.
/// `Debug` shows no part of the value, and `Secret` has no `Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Secret {
        Secret(value.into())
    }

    /// The value. Give it only to the provider client or to the secret box.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Secret, D::Error> {
        String::deserialize(d).map(Secret)
    }
}

/// An access token. Keep it in memory only.
#[derive(Debug, Clone)]
pub struct AccessToken {
    pub token: Secret,
    pub expires_at: Instant,
}

/// The tokens of a completed sign-in.
#[derive(Debug, Clone)]
pub struct Tokens {
    pub access: AccessToken,
    /// Seal this value with the store and keep the blob.
    pub refresh: Secret,
    /// Seconds until the refresh token expires, if Google gives a limit.
    /// A value here means that the client is in the "Testing" status
    /// (7 days). The guided setup tells the user to publish the app.
    pub refresh_expires_in: Option<u64>,
}

#[derive(Debug)]
pub enum Error {
    /// The client file is not a usable "Desktop app" client. The text is fixed.
    ClientFile(&'static str),
    /// Google or the user refused the sign-in. The value is the OAuth error code.
    Denied(String),
    /// The `state` value of the redirect is not the value of this sign-in.
    State,
    /// The address has no answer from Google in it.
    BadAddress,
    /// No redirect came before the time limit.
    Timeout,
    /// The user did not give the `gmail.readonly` permission on the consent screen.
    ScopeMissing,
    /// The answer of the code exchange has no refresh token.
    NoRefreshToken,
    /// Google refused the refresh token (`invalid_grant`). The account state
    /// becomes "needs sign-in".
    Revoked,
    /// The token endpoint refused the request. `code` is the OAuth error code.
    Token {
        status: u16,
        code: String,
    },
    /// The token endpoint gave an answer that is not a token answer.
    BadAnswer,
    Transport(String),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ClientFile(why) => write!(f, "the OAuth client file is not usable: {why}"),
            Error::Denied(code) => write!(f, "the sign-in was refused: {code}"),
            Error::State => f.write_str("the sign-in answer is not for this sign-in"),
            Error::BadAddress => f.write_str("the address has no sign-in answer"),
            Error::Timeout => f.write_str("the sign-in did not complete in time"),
            Error::ScopeMissing => f.write_str(
                "Iris did not get read access to Gmail: select the Gmail box on the consent screen",
            ),
            Error::NoRefreshToken => f.write_str("Google gave no refresh token"),
            Error::Revoked => f.write_str("Google refused the stored sign-in: sign in again"),
            Error::Token { status, code } => {
                write!(f, "the token endpoint refused the request: {status} {code}")
            }
            Error::BadAnswer => {
                f.write_str("the token endpoint gave an answer that Iris cannot read")
            }
            Error::Transport(e) => write!(f, "the token endpoint is not reachable: {e}"),
            Error::Io(e) => write!(f, "loopback listener: {e}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Gives the OAuth error code, or `unknown` when the value is not the form of
/// a code (`[a-z_]`). So an error never carries text that a server echoed back.
fn error_code(raw: &str) -> String {
    let ok = !raw.is_empty()
        && raw.len() <= 64
        && raw.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
    if ok {
        raw.to_string()
    } else {
        "unknown".to_string()
    }
}

/// The OAuth client of the user, from its JSON file.
#[derive(Clone)]
pub struct Client {
    id: String,
    secret: Secret,
    auth_uri: String,
    token_uri: String,
    agent: ureq::Agent,
}

#[derive(Deserialize)]
struct File {
    installed: Option<Installed>,
    web: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
struct Installed {
    client_id: String,
    client_secret: Secret,
}

impl Client {
    /// Reads the JSON file that the Google console gives for a "Desktop app" client.
    pub fn from_json(bytes: &[u8]) -> Result<Client> {
        // The parser error can quote the file, so it is not passed on.
        let file: File = serde_json::from_slice(bytes)
            .map_err(|_| Error::ClientFile("the file is not an OAuth client JSON file"))?;
        let Some(c) = file.installed else {
            return Err(Error::ClientFile(if file.web.is_some() {
                "the client type is \"Web application\": make a client of the type \"Desktop app\""
            } else {
                "the file has no \"installed\" client"
            }));
        };
        if c.client_id.is_empty() || c.client_secret.expose().is_empty() {
            return Err(Error::ClientFile(
                "the client ID or the client secret is empty",
            ));
        }
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Ok(Client {
            id: c.client_id,
            secret: c.client_secret,
            auth_uri: GOOGLE_AUTH_URI.to_string(),
            token_uri: GOOGLE_TOKEN_URI.to_string(),
            agent,
        })
    }

    /// The token endpoint that this client posts to.
    pub fn token_uri(&self) -> &str {
        &self.token_uri
    }

    /// Tests only: post to a fake token endpoint at
    /// `http://127.0.0.1:<port>/<path>`. Any other address is refused.
    #[doc(hidden)]
    pub fn with_test_token_uri(mut self, uri: &str) -> Result<Client> {
        let rest = uri.strip_prefix("http://127.0.0.1:").unwrap_or_default();
        let (port, _) = rest.split_once('/').unwrap_or_default();
        if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::ClientFile(
                "a test endpoint must be http://127.0.0.1:<port>/",
            ));
        }
        self.token_uri = uri.to_string();
        Ok(self)
    }

    /// Starts a sign-in: a listener on a free port of `127.0.0.1`, a new
    /// `state` value and a new PKCE verifier.
    pub fn sign_in(&self) -> Result<SignIn> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(Error::Io)?;
        let port = listener.local_addr().map_err(Error::Io)?.port();
        Ok(SignIn {
            client: self.clone(),
            listener,
            redirect_uri: format!("http://127.0.0.1:{port}{CALLBACK}"),
            state: random_text(),
            verifier: Secret(random_text()),
        })
    }

    /// Gets a new access token with the refresh token.
    pub fn refresh(&self, refresh: &Secret) -> Result<AccessToken> {
        let answer = self.token_call(
            true,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.expose()),
                ("client_id", &self.id),
                ("client_secret", self.secret.expose()),
            ],
        )?;
        check_scope(&answer)?;
        answer.access()
    }

    /// `refresh` says if the request uses a stored refresh token. Only then
    /// does `invalid_grant` mean that the stored sign-in is gone.
    fn token_call(&self, refresh: bool, form: &[(&str, &str)]) -> Result<TokenAnswer> {
        let mut response = self
            .agent
            .post(&self.token_uri)
            .send_form(form.iter().copied())
            .map_err(|e| Error::Transport(e.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(BODY_LIMIT)
            .read_to_vec()
            .map_err(|e| Error::Transport(e.to_string()))?;
        if !(200..300).contains(&status) {
            let code = serde_json::from_slice::<ErrorAnswer>(&body)
                .map(|e| error_code(&e.error))
                .unwrap_or_default();
            return Err(if refresh && code == "invalid_grant" {
                Error::Revoked
            } else {
                Error::Token { status, code }
            });
        }
        serde_json::from_slice(&body).map_err(|_| Error::BadAnswer)
    }
}

#[derive(Deserialize)]
struct ErrorAnswer {
    error: String,
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: Secret,
    expires_in: u64,
    refresh_token: Option<Secret>,
    refresh_token_expires_in: Option<u64>,
    scope: Option<String>,
}

impl TokenAnswer {
    fn access(&self) -> Result<AccessToken> {
        let expires_at = Instant::now()
            .checked_add(Duration::from_secs(self.expires_in))
            .ok_or(Error::BadAnswer)?;
        Ok(AccessToken {
            token: self.access_token.clone(),
            expires_at,
        })
    }
}

/// With granular consent the user can clear the Gmail box, so check the grant.
fn check_scope(answer: &TokenAnswer) -> Result<()> {
    match &answer.scope {
        Some(s) if !s.split(' ').any(|x| x == SCOPE) => Err(Error::ScopeMissing),
        _ => Ok(()),
    }
}

/// 32 random bytes in base64url: 43 characters, valid as a PKCE verifier.
fn random_text() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the system random source failed");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// One sign-in that waits for the redirect from Google.
pub struct SignIn {
    client: Client,
    listener: TcpListener,
    redirect_uri: String,
    state: String,
    verifier: Secret,
}

enum Answer {
    Code(String),
    Denied(String),
}

impl SignIn {
    /// The address to open in the browser.
    pub fn url(&self) -> String {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.expose()));
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("client_id", &self.client.id)
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", SCOPE)
            .append_pair("state", &self.state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            // A refresh token comes only with offline access and a new consent.
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent")
            .finish();
        format!("{}?{query}", self.client.auth_uri)
    }

    /// The redirect address that Google sends the browser to.
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Waits for the redirect on the loopback listener, then exchanges the code.
    pub fn wait(&self, timeout: Duration) -> Result<Tokens> {
        let deadline = Instant::now() + timeout;
        self.listener.set_nonblocking(true).map_err(Error::Io)?;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout);
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(answer) = self.serve(stream, left) {
                        return self.complete(answer);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(Error::Timeout);
                    }
                    // shortcut: std has no accept with a time limit, so poll each 50 ms.
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(Error::Io(e)),
            }
        }
    }

    /// Completes the sign-in with the address that the user copied from the
    /// browser. Use it when the engine runs on another machine than the browser.
    pub fn finish(&self, address: &str) -> Result<Tokens> {
        let query = address.split_once('?').ok_or(Error::BadAddress)?.1;
        let query = query.split('#').next().unwrap_or_default();
        match self.read_query(query) {
            Ok(Some(answer)) => self.complete(answer),
            Ok(None) => Err(Error::BadAddress),
            Err(e) => Err(e),
        }
    }

    /// Reads one browser request. Gives an answer only for the callback with
    /// the right `state`. Other requests get an error page, and the wait goes on.
    /// Reads for at most `left` (the rest of the wait), and at most 5 s.
    fn serve(&self, mut stream: TcpStream, left: Duration) -> Option<Answer> {
        let end = Instant::now() + left.min(Duration::from_secs(5));
        let _ = stream.set_nonblocking(false);
        let mut buf = Vec::with_capacity(1024);
        let mut chunk = [0u8; 1024];
        while !buf.windows(2).any(|w| w == b"\r\n") && buf.len() < REQUEST_LIMIT {
            let rest = end.saturating_duration_since(Instant::now());
            if rest.is_zero() || stream.set_read_timeout(Some(rest)).is_err() {
                break;
            }
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
        let line = String::from_utf8_lossy(&buf);
        let target = line
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("GET "))
            .and_then(|l| l.split(' ').next())
            .unwrap_or_default();
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let (status, page, answer) = if path != CALLBACK {
            ("404 Not Found", "Not found.", None)
        } else {
            match self.read_query(query) {
                Ok(Some(answer)) => {
                    let page = match answer {
                        Answer::Code(_) => "Iris got the answer from Google. Go back to Iris.",
                        Answer::Denied(_) => "The sign-in was refused. Go back to Iris.",
                    };
                    ("200 OK", page, Some(answer))
                }
                _ => (
                    "400 Bad Request",
                    "This answer is not for the current Iris sign-in.",
                    None,
                ),
            }
        };
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        answer
    }

    /// `Ok(None)` when the query has neither a code nor an error.
    fn read_query(&self, query: &str) -> Result<Option<Answer>> {
        let (mut state, mut code, mut error) = (None, None, None);
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            match &*k {
                "state" => state = Some(v.into_owned()),
                "code" => code = Some(v.into_owned()),
                "error" => error = Some(v.into_owned()),
                _ => {}
            }
        }
        if code.is_none() && error.is_none() {
            return Ok(None);
        }
        if state.as_deref() != Some(self.state.as_str()) {
            return Err(Error::State);
        }
        Ok(Some(match (code, error) {
            (_, Some(e)) => Answer::Denied(error_code(&e)),
            (Some(c), None) => Answer::Code(c),
            (None, None) => unreachable!(),
        }))
    }

    fn complete(&self, answer: Answer) -> Result<Tokens> {
        let code = match answer {
            Answer::Code(c) => c,
            Answer::Denied(e) => return Err(Error::Denied(e)),
        };
        let c = &self.client;
        let answer = c.token_call(
            false,
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("client_id", &c.id),
                ("client_secret", c.secret.expose()),
                ("redirect_uri", &self.redirect_uri),
                ("code_verifier", self.verifier.expose()),
            ],
        )?;
        check_scope(&answer)?;
        Ok(Tokens {
            access: answer.access()?,
            refresh: answer.refresh_token.clone().ok_or(Error::NoRefreshToken)?,
            refresh_expires_in: answer.refresh_token_expires_in,
        })
    }
}
