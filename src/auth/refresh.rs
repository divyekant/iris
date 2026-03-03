use oauth2::{RefreshToken, TokenResponse};

use crate::config::Config;
use crate::db::DbPool;
use crate::models::account::Account;

use super::oauth::{build_oauth_client, provider_config, OAuthError};

/// Ensure the account has a fresh access token.
/// Returns the current token if still valid, otherwise refreshes it.
/// For non-OAuth accounts (password auth), returns None.
pub async fn ensure_fresh_token(
    pool: &DbPool,
    account: &Account,
    config: &Config,
) -> Result<Option<String>, OAuthError> {
    // Non-OAuth accounts don't need token refresh
    match account.provider.as_str() {
        "gmail" | "outlook" => {}
        _ => return Ok(None),
    }

    let access_token = account.access_token.as_deref().unwrap_or("");
    let expires_at = account.token_expires_at.unwrap_or(0);
    let now = chrono::Utc::now().timestamp();

    // Token still valid (with 60s buffer)
    if !access_token.is_empty() && now < expires_at - 60 {
        return Ok(Some(access_token.to_string()));
    }

    // Need to refresh
    let refresh_token_str = account
        .refresh_token
        .as_deref()
        .ok_or(OAuthError::TokenExchange("no refresh token stored".into()))?;

    let provider_cfg =
        provider_config(&account.provider).ok_or(OAuthError::UnsupportedProvider)?;
    let client = build_oauth_client(&account.provider, config, &provider_cfg)?;

    let http_client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| OAuthError::Internal(e.to_string()))?;

    let token_result = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token_str.to_string()))
        .request_async(&http_client)
        .await
        .map_err(|e| OAuthError::TokenExchange(e.to_string()))?;

    let new_access = token_result.access_token().secret().clone();
    let new_refresh = token_result
        .refresh_token()
        .map(|rt| rt.secret().clone())
        .unwrap_or_else(|| refresh_token_str.to_string());
    let new_expires = chrono::Utc::now().timestamp()
        + token_result
            .expires_in()
            .map(|d| d.as_secs() as i64)
            .unwrap_or(3600);

    // Persist new tokens
    let conn = pool
        .get()
        .map_err(|e| OAuthError::Internal(e.to_string()))?;
    Account::update_oauth_tokens(&conn, &account.id, &new_access, &new_refresh, new_expires);

    tracing::debug!(account_id = %account.id, "OAuth token refreshed");

    Ok(Some(new_access))
}
