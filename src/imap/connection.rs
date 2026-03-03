use async_imap::Authenticator;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How to authenticate with the IMAP server.
#[derive(Debug, Clone)]
pub enum ImapAuth {
    OAuth2 { access_token: String },
    Password { password: String },
}

/// Everything needed to connect to an IMAP server.
#[derive(Debug, Clone)]
pub struct ImapCredentials {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: ImapAuth,
}

/// A connected, authenticated IMAP session over TLS.
pub type ImapSession = async_imap::Session<async_native_tls::TlsStream<tokio::net::TcpStream>>;

// ---------------------------------------------------------------------------
// XOAUTH2 authenticator
// ---------------------------------------------------------------------------

/// SASL XOAUTH2 authenticator for Gmail / Outlook.
///
/// The SASL mechanism expects the server challenge to be answered with a
/// base64-encoded string of: `user={email}\x01auth=Bearer {token}\x01\x01`
struct XOAuth2 {
    auth_string: String,
}

impl XOAuth2 {
    fn new(user: &str, access_token: &str) -> Self {
        Self {
            auth_string: format!("user={}\x01auth=Bearer {}\x01\x01", user, access_token),
        }
    }
}

impl Authenticator for XOAuth2 {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&self.auth_string)
    }
}

// ---------------------------------------------------------------------------
// Connect
// ---------------------------------------------------------------------------

/// Open a TLS connection to the IMAP server, authenticate, and return the
/// ready-to-use session.
pub async fn connect(creds: &ImapCredentials) -> Result<ImapSession, Box<dyn std::error::Error + Send + Sync>> {
    // 1. TCP connection
    let addr = format!("{}:{}", creds.host, creds.port);
    let tcp = tokio::net::TcpStream::connect(&addr).await?;

    // 2. Wrap in TLS
    let tls_connector = async_native_tls::TlsConnector::new();
    let tls_stream = tls_connector.connect(&creds.host, tcp).await?;

    // 3. Create IMAP client and read greeting
    let client = async_imap::Client::new(tls_stream);
    // The greeting is read internally when we call login/authenticate

    // 4. Authenticate
    let session = match &creds.auth {
        ImapAuth::OAuth2 { access_token } => {
            let authenticator = XOAuth2::new(&creds.username, access_token);
            client
                .authenticate("XOAUTH2", authenticator)
                .await
                .map_err(|(err, _client)| err)?
        }
        ImapAuth::Password { password } => {
            client
                .login(&creds.username, password)
                .await
                .map_err(|(err, _client)| err)?
        }
    };

    Ok(session)
}
