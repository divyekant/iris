use axum::http;
use std::fmt;
use tower_governor::{
    errors::GovernorError,
    governor::GovernorConfigBuilder,
    key_extractor::KeyExtractor,
    GovernorLayer,
};

/// Extracts the session token from either the legacy header or the new session cookie
/// for per-session rate limiting.
#[derive(Clone, Debug)]
pub struct SessionTokenKeyExtractor;

impl KeyExtractor for SessionTokenKeyExtractor {
    type Key = String;

    fn extract<T>(&self, req: &http::Request<T>) -> Result<Self::Key, GovernorError> {
        let key = crate::api::session_auth::extract_session_token(req.headers())
            .map(|(token, _)| token)
            .unwrap_or_else(|| "__anonymous__".to_owned());
        Ok(key)
    }
}

impl fmt::Display for SessionTokenKeyExtractor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SessionTokenKeyExtractor")
    }
}

/// Create a GovernorLayer for rate limiting: ~100 requests per 60 seconds per session token.
///
/// Governor uses a token-bucket (GCRA) algorithm. We configure:
/// - `burst_size(100)`: max 100 tokens in the bucket
/// - `per_second(60)`: replenishment period of 60s per token (~1.67 tokens/sec)
///
/// A client can burst up to 100 requests instantly, then must wait for tokens to
/// replenish at ~1.67/sec (100/min sustained rate).
///
/// When the limit is exceeded, tower_governor returns HTTP 429 with `retry-after` and
/// `x-ratelimit-after` headers. The `use_headers()` call also adds `x-ratelimit-limit`
/// and `x-ratelimit-remaining` headers on successful responses.
pub fn rate_limit_layer() -> GovernorLayer<
    SessionTokenKeyExtractor,
    governor::middleware::StateInformationMiddleware,
    axum::body::Body,
> {
    let config = GovernorConfigBuilder::default()
        .key_extractor(SessionTokenKeyExtractor)
        .per_second(60)
        .burst_size(100)
        .use_headers()
        .finish()
        .expect("Failed to build rate limiter config");

    GovernorLayer::new(config)
}
