//! Google and Microsoft sign-in for Iris (specification 1, section 4.9;
//! Outlook spec `docs/specs/2026-11-17-outlook-provider.md`, section 7).
//!
//! - The user gives the JSON file of an own OAuth client of the type "Desktop
//!   app" (decision record `docs/decisions/2026-11-google-sign-in.md`).
//! - The sign-in uses the loopback redirect on `127.0.0.1` with PKCE (S256)
//!   and asks only for [`SCOPE`], or only for [`WRITE_SCOPE`] when the
//!   person asks for write access ([`Client::sign_in_write`]).
//! - The access token stays in memory. The caller seals the refresh token with
//!   the secret box of the store (`iris_store::Store::seal`) and keeps the blob.
//! - No secret goes into a `Debug` text, a `Display` text or an error.
//! - Microsoft: the user gives the application ID of an own app registration
//!   and a tenant (`common` or a tenant GUID). The client is public (no
//!   secret). Each refresh gives a new refresh token
//!   ([`Client::refresh_tokens`]); the caller keeps it in place of the old one.

use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The read-only scope: the default sign-in.
pub const SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";
/// The scope of a write sign-in (spec `docs/specs/2026-11-18-gmail-write-access.md`).
/// It covers read, labels, trash, drafts and send, but not the permanent delete.
pub const WRITE_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

/// The Google endpoints. Iris uses only these, never the endpoints of the
/// client file, so a changed file cannot send the tokens to another host.
pub const GOOGLE_AUTH_URI: &str = "https://accounts.google.com/o/oauth2/auth";
pub const GOOGLE_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// The Microsoft identity platform. The host is fixed; only the tenant
/// segment changes, and Iris accepts only `common` or a GUID there.
pub const MICROSOFT_LOGIN: &str = "https://login.microsoftonline.com";
/// The scopes of an Outlook account (Outlook spec section 7).
pub const MICROSOFT_SCOPE: &str = "offline_access https://graph.microsoft.com/Mail.ReadWrite https://graph.microsoft.com/Mail.Send https://graph.microsoft.com/MailboxSettings.Read https://graph.microsoft.com/User.Read";
/// The Graph permissions that each token answer must hold.
/// `MailboxSettings.Read` reads the category list of the mailbox, which the
/// label list of each sync needs.
const MICROSOFT_NEEDED: [&str; 3] = ["Mail.ReadWrite", "Mail.Send", "MailboxSettings.Read"];

/// The path of the loopback redirect.
pub const CALLBACK: &str = "/oauth/callback";

/// Largest answer body that the token endpoint can give.
const BODY_LIMIT: u64 = 64 * 1024;
/// Largest request line that the loopback listener reads.
const REQUEST_LIMIT: usize = 8 * 1024;
/// Most browser connections that the loopback listener keeps open at a time.
const MAX_OPEN: usize = 16;

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
    /// True if the sign-in asked for write access and Google granted
    /// [`WRITE_SCOPE`]. Always true for Microsoft,
    /// whose sign-in requires `Mail.ReadWrite`.
    pub write: bool,
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
    /// A write sign-in ([`Client::sign_in_write`]) did not get the
    /// `gmail.modify` permission. Nothing is saved.
    WriteScopeMissing,
    /// The Microsoft token answer does not hold this Graph permission. The
    /// account does not start.
    PermissionMissing(&'static str),
    /// The answer of the code exchange has no refresh token.
    NoRefreshToken,
    /// [`Client::refresh`] of a Microsoft client: it would drop the new
    /// refresh token. A program error, not a provider answer.
    RotatingRefresh,
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
    /// The random source of the system failed.
    Random,
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
            Error::WriteScopeMissing => f.write_str(
                "Iris did not get write access to Gmail: select the box that lets Iris \
                 read, compose and send mail, or add the scope gmail.modify to the Data Access \
                 page of your OAuth client",
            ),
            Error::PermissionMissing(p) => write!(
                f,
                "Iris did not get the Microsoft permission {p}: sign in again and accept it"
            ),
            Error::NoRefreshToken => f.write_str("the sign-in gave no refresh token"),
            Error::RotatingRefresh => {
                f.write_str("a Microsoft refresh must keep the new refresh token")
            }
            Error::Revoked => f.write_str("the provider refused the stored sign-in: sign in again"),
            Error::Token { status, code } => {
                write!(f, "the token endpoint refused the request: {status} {code}")
            }
            Error::BadAnswer => {
                f.write_str("the token endpoint gave an answer that Iris cannot read")
            }
            Error::Random => f.write_str("the random source of the system failed"),
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

/// The provider of a client.
#[derive(Clone)]
enum Kind {
    /// A "Desktop app" client, with its client secret.
    Google(Secret),
    /// A public client: no secret.
    Microsoft,
}

/// The OAuth client of the user.
#[derive(Clone)]
pub struct Client {
    id: String,
    kind: Kind,
    auth_uri: String,
    token_uri: String,
    agent: ureq::Agent,
    /// Google only: ask for [`WRITE_SCOPE`] in place of [`SCOPE`].
    write: bool,
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
        Ok(Client {
            id: c.client_id,
            kind: Kind::Google(c.client_secret),
            auth_uri: GOOGLE_AUTH_URI.to_string(),
            token_uri: GOOGLE_TOKEN_URI.to_string(),
            agent: agent(),
            write: false,
        })
    }

    /// A Microsoft client: the application (client) ID of the app
    /// registration of the user, and the tenant: `common` or the tenant ID
    /// (decision record `docs/decisions/2026-11-microsoft-sign-in.md`).
    pub fn microsoft(client_id: &str, tenant: &str) -> Result<Client> {
        if !is_guid(client_id) {
            return Err(Error::ClientFile(
                "the application (client) ID must be a GUID, as the Entra admin center shows it",
            ));
        }
        if tenant != "common" && !is_guid(tenant) {
            return Err(Error::ClientFile(
                "the tenant must be \"common\" or the directory (tenant) ID, a GUID",
            ));
        }
        let tenant = tenant.to_ascii_lowercase();
        Ok(Client {
            id: client_id.to_ascii_lowercase(),
            kind: Kind::Microsoft,
            auth_uri: format!("{MICROSOFT_LOGIN}/{tenant}/oauth2/v2.0/authorize"),
            token_uri: format!("{MICROSOFT_LOGIN}/{tenant}/oauth2/v2.0/token"),
            agent: agent(),
            write: false,
        })
    }

    /// The scope text of the authorization request.
    fn scope(&self) -> &'static str {
        match self.kind {
            Kind::Google(_) if self.write => WRITE_SCOPE,
            Kind::Google(_) => SCOPE,
            Kind::Microsoft => MICROSOFT_SCOPE,
        }
    }

    /// The form fields that name the client: the secret only for Google.
    fn identity(&self) -> Vec<(&str, &str)> {
        let mut form = vec![("client_id", self.id.as_str())];
        if let Kind::Google(secret) = &self.kind {
            form.push(("client_secret", secret.expose()));
        }
        form
    }

    /// Checks the granted scope of a token answer. A code exchange must
    /// name it. A refresh can leave it out when the grant did not change.
    fn check_scope(&self, answer: &TokenAnswer, refresh: bool) -> Result<()> {
        let Some(granted) = &answer.scope else {
            return if refresh { Ok(()) } else { Err(self.missing()) };
        };
        match self.kind {
            // `gmail.modify` includes read.
            Kind::Google(_) if self.write && !has_write(granted) => Err(Error::WriteScopeMissing),
            Kind::Google(_) if has_write(granted) || granted.split(' ').any(|x| x == SCOPE) => {
                Ok(())
            }
            Kind::Google(_) => Err(Error::ScopeMissing),
            Kind::Microsoft => {
                // Microsoft can name a Graph scope with or without its
                // resource prefix, in any case.
                let has = |p: &str| {
                    granted.split(' ').any(|x| {
                        let x = x.to_ascii_lowercase();
                        let x = x.strip_prefix("https://graph.microsoft.com/").unwrap_or(&x);
                        x == p.to_ascii_lowercase()
                    })
                };
                match MICROSOFT_NEEDED.iter().find(|p| !has(p)) {
                    Some(p) => Err(Error::PermissionMissing(p)),
                    None => Ok(()),
                }
            }
        }
    }

    fn missing(&self) -> Error {
        match self.kind {
            Kind::Google(_) if self.write => Error::WriteScopeMissing,
            Kind::Google(_) => Error::ScopeMissing,
            Kind::Microsoft => Error::PermissionMissing(MICROSOFT_NEEDED[0]),
        }
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
        // Microsoft matches `http://localhost` with any port (A9); the
        // listener still binds 127.0.0.1 only.
        let host = match self.kind {
            Kind::Google(_) => "127.0.0.1",
            Kind::Microsoft => "localhost",
        };
        Ok(SignIn {
            client: self.clone(),
            listener,
            redirect_uri: format!("http://{host}:{port}{CALLBACK}"),
            state: random_text()?,
            verifier: Secret(random_text()?),
        })
    }

    /// Starts a sign-in that asks for [`WRITE_SCOPE`] only (Google). The
    /// answer must grant it, else [`Error::WriteScopeMissing`]. A Microsoft
    /// client asks for its usual scopes, which already allow writes.
    pub fn sign_in_write(&self) -> Result<SignIn> {
        let mut client = self.clone();
        client.write = true;
        client.sign_in()
    }

    /// Gets a new access token with the refresh token (Google). A Microsoft
    /// client gets [`Error::RotatingRefresh`] and sends no request: use
    /// [`Client::refresh_tokens`], so that the new refresh token is never
    /// dropped.
    pub fn refresh(&self, refresh: &Secret) -> Result<AccessToken> {
        if matches!(self.kind, Kind::Microsoft) {
            return Err(Error::RotatingRefresh);
        }
        self.refresh_tokens(refresh).map(|r| r.access)
    }

    /// Gets a new access token, and the new refresh token if the answer has
    /// one. Microsoft gives one at each refresh (A11): seal it in place of
    /// the old one. The old token stays valid, so a crash before the write
    /// loses nothing.
    pub fn refresh_tokens(&self, refresh: &Secret) -> Result<Refreshed> {
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.expose()),
        ];
        form.extend(self.identity());
        if matches!(self.kind, Kind::Microsoft) {
            form.push(("scope", MICROSOFT_SCOPE));
        }
        let answer = self.token_call(true, &form)?;
        self.check_scope(&answer, true)?;
        Ok(Refreshed {
            access: answer.access()?,
            refresh: answer.refresh_token.filter(|r| !r.expose().is_empty()),
        })
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
        if self.access_token.expose().is_empty() {
            return Err(Error::BadAnswer);
        }
        let expires_at = Instant::now()
            .checked_add(Duration::from_secs(self.expires_in))
            .ok_or(Error::BadAnswer)?;
        Ok(AccessToken {
            token: self.access_token.clone(),
            expires_at,
        })
    }
}

/// The answer of a refresh.
#[derive(Debug, Clone)]
pub struct Refreshed {
    pub access: AccessToken,
    /// The new refresh token, if the answer has one. Seal it in place of
    /// the old one.
    pub refresh: Option<Secret>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        // A redirect could carry a code, a verifier or a refresh token to
        // another host: a 3xx is an error answer of the token endpoint.
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into()
}

/// `8-4-4-4-12` hex digits, as Entra shows an application or tenant ID.
fn is_guid(v: &str) -> bool {
    let parts: Vec<&str> = v.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// 32 random bytes in base64url: 43 characters, valid as a PKCE verifier.
fn random_text() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Random)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Reads what a connection has now. Gives `true` when the request line is
/// complete, the limit is reached, or the connection ended.
fn read_some(stream: &mut TcpStream, buf: &mut Vec<u8>) -> bool {
    let mut chunk = [0u8; 1024];
    loop {
        if buf.windows(2).any(|w| w == b"\r\n") || buf.len() >= REQUEST_LIMIT {
            return true;
        }
        match stream.read(&mut chunk) {
            Ok(0) => return true,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return false,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return true,
        }
    }
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
        let mut query = form_urlencoded::Serializer::new(String::new());
        query
            .append_pair("client_id", &self.client.id)
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", self.client.scope())
            .append_pair("state", &self.state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        match self.client.kind {
            // A refresh token comes only with offline access and a new consent.
            Kind::Google(_) => query
                .append_pair("access_type", "offline")
                .append_pair("prompt", "consent"),
            // `offline_access` in the scope gives the refresh token.
            Kind::Microsoft => query.append_pair("prompt", "select_account"),
        };
        let query = query.finish();
        format!("{}?{query}", self.client.auth_uri)
    }

    /// The redirect address that Google sends the browser to.
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Waits for the redirect on the loopback listener, then exchanges the code.
    /// `timeout` limits the wait for the redirect only. The code exchange after
    /// it has the 30-second limit of the HTTP client.
    pub fn wait(&self, timeout: Duration) -> Result<Tokens> {
        // A longer wait is of no use, and the cap keeps the Instant sum in range.
        let deadline = Instant::now() + timeout.min(Duration::from_secs(24 * 3600));
        self.listener.set_nonblocking(true).map_err(Error::Io)?;
        // Each open browser connection: the stream, the bytes so far, its end time.
        // All are read without a block, so an idle one cannot hide the callback.
        let mut open: Vec<(TcpStream, Vec<u8>, Instant)> = Vec::new();
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout);
            }
            // At most MAX_OPEN new connections each turn, so a flood cannot
            // keep the loop from the deadline and the open connections.
            for _ in 0..MAX_OPEN {
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        if stream.set_nonblocking(true).is_ok() {
                            // When full, drop the oldest, so a new callback always gets in.
                            if open.len() == MAX_OPEN {
                                open.remove(0);
                            }
                            open.push((stream, Vec::new(), now + Duration::from_secs(5)));
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(Error::Io(e)),
                }
            }
            let mut i = 0;
            while i < open.len() {
                let (stream, buf, end) = &mut open[i];
                if read_some(stream, buf) || now >= *end {
                    let (stream, buf, _) = open.swap_remove(i);
                    if let Some(answer) = self.serve(stream, &buf) {
                        return self.complete(answer);
                    }
                } else {
                    i += 1;
                }
            }
            // shortcut: std has no poll, so check the sockets each 20 ms.
            std::thread::sleep(Duration::from_millis(20));
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

    /// Answers one browser request. Gives an answer only for the callback with
    /// the right `state`. Other requests get an error page, and the wait goes on.
    fn serve(&self, mut stream: TcpStream, buf: &[u8]) -> Option<Answer> {
        let line = String::from_utf8_lossy(buf);
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
                        Answer::Code(_) => "Iris got the sign-in answer. Go back to Iris.",
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
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
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
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("code_verifier", self.verifier.expose()),
        ];
        form.extend(c.identity());
        let answer = c.token_call(false, &form)?;
        c.check_scope(&answer, false)?;
        Ok(Tokens {
            access: answer.access()?,
            refresh: answer
                .refresh_token
                .clone()
                .filter(|r| !r.expose().is_empty())
                .ok_or(Error::NoRefreshToken)?,
            refresh_expires_in: answer.refresh_token_expires_in,
            // A plain sign-in is read only in Iris, also when Google gives
            // an earlier gmail.modify grant: the person asked for read only.
            write: match c.kind {
                Kind::Google(_) => c.write && answer.scope.as_deref().is_some_and(has_write),
                Kind::Microsoft => true,
            },
        })
    }
}

/// True if a granted scope text (scopes with one space between them) holds
/// [`WRITE_SCOPE`].
fn has_write(granted: &str) -> bool {
    granted.split(' ').any(|x| x == WRITE_SCOPE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small random number source for the property tests.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) % n as u64) as usize
        }
    }

    /// The GUID rule by position: 36 characters, `-` at 8, 13, 18 and 23,
    /// hex digits at all other places.
    fn guid_by_position(v: &str) -> bool {
        v.len() == 36
            && v.bytes().enumerate().all(|(i, b)| match i {
                8 | 13 | 18 | 23 => b == b'-',
                _ => b.is_ascii_hexdigit(),
            })
    }

    /// Property: only a GUID (or `common` as the tenant) makes a client,
    /// and the endpoints of each client stay on the fixed host.
    #[test]
    fn generated_ids_and_tenants() {
        let chars = b"0123456789abcdefABCDEF-g/@.:?#%xX ";
        let mut rng = Rng(5);
        let mut made = 0;
        for _ in 0..20_000 {
            let gen = |rng: &mut Rng| -> String {
                match rng.below(4) {
                    // A GUID with one character changed, or none.
                    0 | 1 => {
                        let mut g: Vec<u8> = b"0a1b2c3d-4e5f-6A7B-8C9D-0e1f2a3b4c5d".to_vec();
                        if rng.below(2) == 0 {
                            let i = rng.below(g.len());
                            g[i] = chars[rng.below(chars.len())];
                        }
                        if rng.below(8) == 0 {
                            g.push(chars[rng.below(chars.len())]);
                        }
                        String::from_utf8(g).unwrap()
                    }
                    2 => [
                        "common",
                        "Common",
                        "organizations",
                        "consumers",
                        "common/x",
                        "",
                    ][rng.below(6)]
                    .to_string(),
                    _ => (0..rng.below(40))
                        .map(|_| chars[rng.below(chars.len())] as char)
                        .collect(),
                }
            };
            let (id, tenant) = (gen(&mut rng), gen(&mut rng));
            let ok = guid_by_position(&id) && (tenant == "common" || guid_by_position(&tenant));
            let got = Client::microsoft(&id, &tenant);
            assert_eq!(got.is_ok(), ok, "{id:?} {tenant:?}");
            if let Ok(c) = got {
                made += 1;
                let rest = c
                    .token_uri
                    .strip_prefix("https://login.microsoftonline.com/")
                    .unwrap();
                let (seg, path) = rest.split_once('/').unwrap();
                assert_eq!(path, "oauth2/v2.0/token");
                assert!(seg == "common" || guid_by_position(seg), "{seg}");
                assert!(c.auth_uri.starts_with("https://login.microsoftonline.com/"));
            }
        }
        assert!(made > 100, "too few clients: {made}");
    }

    /// Property: a Microsoft token answer passes only if it names
    /// Mail.ReadWrite, Mail.Send and MailboxSettings.Read, bare or with the Graph prefix, in any
    /// case; else the first missing one is named.
    #[test]
    fn generated_scope_strings() {
        let words = [
            "Mail.ReadWrite",
            "mail.readwrite",
            "https://graph.microsoft.com/Mail.ReadWrite",
            "Mail.Send",
            "MAIL.SEND",
            "https://graph.microsoft.com/mail.send",
            "Mail.Read",
            "Mail.ReadWrite.Shared",
            "Mail.SendX",
            "https://evil.example/Mail.Send",
            "graph.microsoft.com/Mail.Send",
            "MailboxSettings.Read",
            "mailboxsettings.read",
            "https://graph.microsoft.com/MailboxSettings.Read",
            "MailboxSettings.ReadWrite",
            "User.Read",
            "offline_access",
            "openid",
            "",
        ];
        let c = Client::microsoft("0a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d", "common").unwrap();
        let mut rng = Rng(9);
        for _ in 0..10_000 {
            let n = rng.below(8);
            let scope: Vec<&str> = (0..n).map(|_| words[rng.below(words.len())]).collect();
            let scope = scope.join(" ");
            let has = |p: &str| {
                scope.split(' ').any(|t| {
                    let t = t.to_lowercase();
                    let p = p.to_lowercase();
                    t == p || t == format!("https://graph.microsoft.com/{p}")
                })
            };
            let expect = ["Mail.ReadWrite", "Mail.Send", "MailboxSettings.Read"]
                .into_iter()
                .find(|p| !has(p));
            let answer = TokenAnswer {
                access_token: Secret::new("a"),
                expires_in: 1,
                refresh_token: None,
                refresh_token_expires_in: None,
                scope: Some(scope.clone()),
            };
            for refresh in [false, true] {
                match (c.check_scope(&answer, refresh), expect) {
                    (Ok(()), None) => {}
                    (Err(Error::PermissionMissing(p)), Some(e)) if p == e => {}
                    (got, _) => panic!("{scope:?}: {got:?}, want {expect:?}"),
                }
            }
        }
    }

    /// Property: a Google token answer gives write access only if one
    /// space-separated token is exactly `gmail.modify`. A near match (a
    /// prefix, a suffix, another case, another scope) never gives it. A
    /// read sign-in passes with `gmail.readonly` or `gmail.modify`; a write
    /// sign-in passes only with `gmail.modify`.
    #[test]
    fn generated_google_scope_strings() {
        let words = [
            SCOPE,
            WRITE_SCOPE,
            "https://www.googleapis.com/auth/gmail.modify.x",
            "https://www.googleapis.com/auth/gmail.modif",
            "HTTPS://WWW.GOOGLEAPIS.COM/AUTH/GMAIL.MODIFY",
            "https://www.googleapis.com/auth/gmail.send",
            "https://www.googleapis.com/auth/gmail.compose",
            "https://mail.google.com/",
            "gmail.modify",
            "https://evil.example/auth/gmail.modify",
            "https://www.googleapis.com/auth/gmail.readonly2",
            "openid",
            "",
        ];
        let json = br#"{"installed":{"client_id":"id","client_secret":"s"}}"#;
        let read = Client::from_json(json).unwrap();
        let mut write = read.clone();
        write.write = true;
        let mut rng = Rng(17);
        for _ in 0..10_000 {
            let n = rng.below(6);
            let scope: Vec<&str> = (0..n).map(|_| words[rng.below(words.len())]).collect();
            let scope = scope.join(" ");
            let w = scope.split(' ').any(|t| t == WRITE_SCOPE);
            let r = scope.split(' ').any(|t| t == SCOPE);
            assert_eq!(has_write(&scope), w, "{scope:?}");
            let answer = TokenAnswer {
                access_token: Secret::new("a"),
                expires_in: 1,
                refresh_token: None,
                refresh_token_expires_in: None,
                scope: Some(scope.clone()),
            };
            match (read.check_scope(&answer, false), w || r) {
                (Ok(()), true) | (Err(Error::ScopeMissing), false) => {}
                (got, want) => panic!("read {scope:?}: {got:?}, want {want}"),
            }
            match (write.check_scope(&answer, false), w) {
                (Ok(()), true) | (Err(Error::WriteScopeMissing), false) => {}
                (got, want) => panic!("write {scope:?}: {got:?}, want {want}"),
            }
        }
    }
}
