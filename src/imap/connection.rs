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
/// Returns the RAW XOAUTH2 string (async_imap handles base64 encoding).
/// Format: `user={email}\x01auth=Bearer {token}\x01\x01`
struct XOAuth2 {
    auth_string: Vec<u8>,
}

impl XOAuth2 {
    fn new(user: &str, access_token: &str) -> Self {
        Self {
            auth_string: format!("user={}\x01auth=Bearer {}\x01\x01", user, access_token)
                .into_bytes(),
        }
    }
}

impl Authenticator for XOAuth2 {
    type Response = Vec<u8>;

    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        // Return raw bytes — async_imap will base64-encode them
        self.auth_string.clone()
    }
}

// ---------------------------------------------------------------------------
// Connect
// ---------------------------------------------------------------------------

/// Open a TLS connection to the IMAP server, authenticate, and return the
/// ready-to-use session.
pub async fn connect(creds: &ImapCredentials) -> Result<ImapSession, Box<dyn std::error::Error + Send + Sync>> {
    // 1. TCP connection (with 30s timeout to prevent indefinite hangs)
    let addr = format!("{}:{}", creds.host, creds.port);
    tracing::info!(host = %creds.host, port = creds.port, "IMAP: connecting...");
    let tcp = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .map_err(|_| format!("IMAP connection timeout after 30s: {}", addr))??;

    // 2. Wrap in TLS (with 30s timeout)
    let tls_connector = async_native_tls::TlsConnector::new();
    let mut tls_stream = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tls_connector.connect(&creds.host, tcp),
    )
    .await
    .map_err(|_| format!("IMAP TLS handshake timeout after 30s: {}", creds.host))??;

    // 3. Read and discard the server greeting before creating the Client.
    //    async_imap's do_auth_handshake has a bug where the unread greeting
    //    gets consumed in the wrong branch, causing a deadlock during
    //    SASL AUTHENTICATE (the `+` continuation is eaten by check_done_ok_from).
    {
        use tokio::io::AsyncBufReadExt;
        let mut buf_stream = tokio::io::BufReader::new(&mut tls_stream);
        let mut greeting = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            buf_stream.read_line(&mut greeting),
        )
        .await
        .map_err(|_| "IMAP greeting timeout after 10s".to_string())??;
        tracing::debug!(greeting = greeting.trim(), "IMAP: server greeting");
    }

    // 4. Create IMAP client (greeting already consumed)
    let client = async_imap::Client::new(tls_stream);

    // 5. Authenticate (with 30s timeout)
    let session = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        async {
            match &creds.auth {
                ImapAuth::OAuth2 { access_token } => {
                    let authenticator = XOAuth2::new(&creds.username, access_token);
                    client
                        .authenticate("XOAUTH2", authenticator)
                        .await
                        .map_err(|(err, _client)| -> Box<dyn std::error::Error + Send + Sync> { Box::new(err) })
                }
                ImapAuth::Password { password } => {
                    client
                        .login(&creds.username, password)
                        .await
                        .map_err(|(err, _client)| -> Box<dyn std::error::Error + Send + Sync> { Box::new(err) })
                }
            }
        },
    )
    .await
    .map_err(|_| "IMAP authentication timeout after 30s".to_string())??;
    tracing::info!("IMAP: authenticated successfully");

    Ok(session)
}
