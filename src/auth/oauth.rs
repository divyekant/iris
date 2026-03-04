use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Redirect},
    Json,
};
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken,
    EndpointNotSet, EndpointSet, RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};

use crate::models::account::CreateAccount;
use crate::AppState;

/// Fully-configured BasicClient with auth URL and token URL set.
pub(crate) type ConfiguredClient = BasicClient<
    EndpointSet,   // HasAuthUrl
    EndpointNotSet, // HasDeviceAuthUrl
    EndpointNotSet, // HasIntrospectionUrl
    EndpointNotSet, // HasRevocationUrl
    EndpointSet,   // HasTokenUrl
>;

// ---------------------------------------------------------------------------
// Provider configuration
// ---------------------------------------------------------------------------

pub(crate) struct ProviderConfig {
    pub(crate) auth_url: &'static str,
    pub(crate) token_url: &'static str,
    pub(crate) userinfo_url: &'static str,
    pub(crate) scopes: &'static [&'static str],
}

pub(crate) fn provider_config(provider: &str) -> Option<ProviderConfig> {
    match provider {
        "gmail" => Some(ProviderConfig {
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            userinfo_url: "https://www.googleapis.com/oauth2/v2/userinfo",
            scopes: &[
                "https://mail.google.com/",
                "openid",
                "email",
                "profile",
            ],
        }),
        "outlook" => Some(ProviderConfig {
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            userinfo_url: "https://graph.microsoft.com/v1.0/me",
            scopes: &[
                "https://outlook.office365.com/IMAP.AccessAsUser.All",
                "https://outlook.office365.com/SMTP.Send",
                "offline_access",
                "openid",
                "email",
                "profile",
            ],
        }),
        _ => None,
    }
}

fn client_credentials<'a>(
    provider: &str,
    config: &'a crate::config::Config,
) -> Result<(&'a str, &'a str), OAuthError> {
    match provider {
        "gmail" => Ok((
            config
                .gmail_client_id
                .as_deref()
                .ok_or(OAuthError::MissingConfig("GMAIL_CLIENT_ID"))?,
            config
                .gmail_client_secret
                .as_deref()
                .ok_or(OAuthError::MissingConfig("GMAIL_CLIENT_SECRET"))?,
        )),
        "outlook" => Ok((
            config
                .outlook_client_id
                .as_deref()
                .ok_or(OAuthError::MissingConfig("OUTLOOK_CLIENT_ID"))?,
            config
                .outlook_client_secret
                .as_deref()
                .ok_or(OAuthError::MissingConfig("OUTLOOK_CLIENT_SECRET"))?,
        )),
        _ => Err(OAuthError::UnsupportedProvider),
    }
}

pub(crate) fn build_oauth_client(
    provider: &str,
    config: &crate::config::Config,
    provider_cfg: &ProviderConfig,
) -> Result<ConfiguredClient, OAuthError> {
    let (client_id, client_secret) = client_credentials(provider, config)?;
    let redirect_url = format!("{}/auth/callback", config.public_url);

    let client = BasicClient::new(ClientId::new(client_id.to_string()))
        .set_client_secret(ClientSecret::new(client_secret.to_string()))
        .set_auth_uri(
            AuthUrl::new(provider_cfg.auth_url.to_string())
                .map_err(|e| OAuthError::Internal(e.to_string()))?,
        )
        .set_token_uri(
            TokenUrl::new(provider_cfg.token_url.to_string())
                .map_err(|e| OAuthError::Internal(e.to_string()))?,
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect_url)
                .map_err(|e| OAuthError::Internal(e.to_string()))?,
        );

    Ok(client)
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("unsupported provider")]
    UnsupportedProvider,
    #[error("missing config: {0}")]
    MissingConfig(&'static str),
    #[error("token exchange failed: {0}")]
    TokenExchange(String),
    #[error("failed to fetch user info: {0}")]
    UserInfo(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for OAuthError {
    fn into_response(self) -> axum::response::Response {
        let status = match &self {
            OAuthError::UnsupportedProvider => axum::http::StatusCode::BAD_REQUEST,
            OAuthError::MissingConfig(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            OAuthError::TokenExchange(_) => axum::http::StatusCode::BAD_GATEWAY,
            OAuthError::UserInfo(_) => axum::http::StatusCode::BAD_GATEWAY,
            OAuthError::Internal(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = serde_json::json!({ "error": self.to_string() });
        (status, Json(body)).into_response()
    }
}

// ---------------------------------------------------------------------------
// GET /api/auth/oauth/:provider  -- start OAuth flow
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct StartOAuthResponse {
    pub url: String,
    pub provider: String,
}

pub async fn start_oauth(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<String>,
) -> Result<Json<StartOAuthResponse>, OAuthError> {
    let provider_cfg = provider_config(&provider).ok_or(OAuthError::UnsupportedProvider)?;
    let client = build_oauth_client(&provider, &state.config, &provider_cfg)?;

    // Encode provider in the state parameter: "{provider}:{csrf_random}"
    let csrf_random = CsrfToken::new_random();
    let state_value = format!("{}:{}", provider, csrf_random.secret());

    let mut auth_request = client.authorize_url(|| CsrfToken::new(state_value));

    // Add scopes
    for scope in provider_cfg.scopes {
        auth_request = auth_request.add_scope(Scope::new(scope.to_string()));
    }

    // Gmail-specific: force offline access and consent prompt
    if provider == "gmail" {
        auth_request = auth_request
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent");
    }

    let (auth_url, _csrf_token) = auth_request.url();

    Ok(Json(StartOAuthResponse {
        url: auth_url.to_string(),
        provider,
    }))
}

// ---------------------------------------------------------------------------
// GET /auth/callback?code=...&state=...  -- OAuth callback
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
}

#[derive(Deserialize)]
struct GoogleUserInfo {
    email: String,
    name: Option<String>,
}

#[derive(Deserialize)]
struct MicrosoftUserInfo {
    #[serde(alias = "mail", alias = "userPrincipalName")]
    email: Option<String>,
    #[serde(alias = "displayName")]
    display_name: Option<String>,
}

pub async fn oauth_callback(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CallbackParams>,
) -> Result<impl IntoResponse, OAuthError> {
    // Parse provider from state: "{provider}:{csrf_random}"
    let provider = params
        .state
        .split(':')
        .next()
        .unwrap_or("")
        .to_string();

    let provider_cfg = provider_config(&provider).ok_or(OAuthError::UnsupportedProvider)?;
    let client = build_oauth_client(&provider, &state.config, &provider_cfg)?;

    // Build an HTTP client for the token exchange (no redirects per oauth2 crate guidance)
    let http_client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| OAuthError::Internal(e.to_string()))?;

    // Exchange the authorization code for tokens
    let token_result = client
        .exchange_code(AuthorizationCode::new(params.code))
        .request_async(&http_client)
        .await
        .map_err(|e| OAuthError::TokenExchange(e.to_string()))?;

    let access_token = token_result.access_token().secret().clone();
    let refresh_token = token_result
        .refresh_token()
        .map(|rt| rt.secret().clone())
        .unwrap_or_default();
    let expires_in_secs: i64 = token_result
        .expires_in()
        .map(|d| d.as_secs() as i64)
        .unwrap_or(3600);
    let expires_at = chrono::Utc::now().timestamp() + expires_in_secs;

    // Fetch user info from provider
    let (email, display_name) =
        fetch_user_info(&provider, provider_cfg.userinfo_url, &access_token).await?;

    // Determine IMAP/SMTP settings based on provider
    let (imap_host, imap_port, smtp_host, smtp_port) = match provider.as_str() {
        "gmail" => (
            Some("imap.gmail.com".to_string()),
            Some(993),
            Some("smtp.gmail.com".to_string()),
            Some(587),
        ),
        "outlook" => (
            Some("outlook.office365.com".to_string()),
            Some(993),
            Some("smtp.office365.com".to_string()),
            Some(587),
        ),
        _ => (None, None, None, None),
    };

    // Create account in DB
    let conn = state
        .db
        .get()
        .map_err(|e| OAuthError::Internal(e.to_string()))?;

    let input = CreateAccount {
        provider: provider.clone(),
        email: email.clone(),
        display_name,
        imap_host,
        imap_port,
        smtp_host,
        smtp_port,
        username: Some(email),
        password: None,
    };

    let account = crate::models::account::Account::create(&conn, &input);

    // Store OAuth tokens
    crate::models::account::Account::update_oauth_tokens(
        &conn,
        &account.id,
        &access_token,
        &refresh_token,
        expires_at,
    );

    tracing::info!(
        provider = %provider,
        account_id = %account.id,
        email = %account.email,
        "OAuth account created"
    );

    // Spawn initial sync + IDLE listener in the background
    if let (Some(imap_host), Some(imap_port)) = (account.imap_host.clone(), account.imap_port) {
        let sync_creds = crate::imap::connection::ImapCredentials {
            host: imap_host,
            port: imap_port as u16,
            username: account.email.clone(),
            auth: crate::imap::connection::ImapAuth::OAuth2 {
                access_token: access_token.clone(),
            },
        };
        let db_clone = state.db.clone();
        let ws_clone = state.ws_hub.clone();
        let ollama_clone = state.ollama.clone();
        let memories_clone = state.memories.clone();
        let acct_id = account.id.clone();

        tokio::spawn(async move {
            let engine = crate::imap::sync::SyncEngine::new(
                db_clone.clone(),
                ws_clone.clone(),
                ollama_clone.clone(),
                memories_clone.clone(),
            );
            if let Err(e) = engine.initial_sync(&acct_id, &sync_creds).await {
                tracing::error!(account_id = %acct_id, error = %e, "Initial sync failed");
            }
            // Start IDLE listener after initial sync completes
            crate::imap::idle::spawn_idle_listener(acct_id, sync_creds, db_clone, ws_clone, ollama_clone, memories_clone);
        });
    }

    // Redirect to frontend success page
    Ok(Redirect::to(&format!(
        "/setup/success?account_id={}",
        account.id
    )))
}

// ---------------------------------------------------------------------------
// Fetch user info from the provider's userinfo endpoint
// ---------------------------------------------------------------------------

async fn fetch_user_info(
    provider: &str,
    userinfo_url: &str,
    access_token: &str,
) -> Result<(String, Option<String>), OAuthError> {
    let http = reqwest::Client::new();

    match provider {
        "gmail" => {
            let resp: GoogleUserInfo = http
                .get(userinfo_url)
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|e| OAuthError::UserInfo(e.to_string()))?
                .json()
                .await
                .map_err(|e| OAuthError::UserInfo(e.to_string()))?;
            Ok((resp.email, resp.name))
        }
        "outlook" => {
            let resp: MicrosoftUserInfo = http
                .get(userinfo_url)
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|e| OAuthError::UserInfo(e.to_string()))?
                .json()
                .await
                .map_err(|e| OAuthError::UserInfo(e.to_string()))?;
            let email = resp
                .email
                .ok_or_else(|| OAuthError::UserInfo("no email in Microsoft profile".into()))?;
            Ok((email, resp.display_name))
        }
        _ => Err(OAuthError::UnsupportedProvider),
    }
}
