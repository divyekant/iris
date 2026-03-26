use axum::{
    body::Body,
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::api::session_auth::{extract_session_token, is_safe_method, is_same_origin_browser_context, SessionTransport};
use crate::AppState;

// ---------------------------------------------------------------------------
// Permission enum
// ---------------------------------------------------------------------------

/// Permission levels for API keys, ordered from least to most privileged.
/// The derived `PartialOrd`/`Ord` use declaration order, so `ReadOnly < Autonomous`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Permission {
    ReadOnly,
    DraftOnly,
    SendWithApproval,
    Autonomous,
}

impl Permission {
    /// Parse a permission from its DB string representation.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "read_only" => Some(Self::ReadOnly),
            "draft_only" => Some(Self::DraftOnly),
            "send_with_approval" => Some(Self::SendWithApproval),
            "autonomous" => Some(Self::Autonomous),
            _ => None,
        }
    }

    /// Returns `true` if this permission level satisfies `needed` (i.e. self >= needed).
    pub fn satisfies(self, needed: Permission) -> bool {
        self >= needed
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Permission::ReadOnly => write!(f, "read_only"),
            Permission::DraftOnly => write!(f, "draft_only"),
            Permission::SendWithApproval => write!(f, "send_with_approval"),
            Permission::Autonomous => write!(f, "autonomous"),
        }
    }
}

// ---------------------------------------------------------------------------
// AuthContext enum
// ---------------------------------------------------------------------------

/// Unified auth context for both UI sessions and agent API-key requests.
#[derive(Debug, Clone)]
pub enum AuthContext {
    /// UI session — full access, no permission restrictions.
    Session,
    /// API key — scoped to a specific permission level and optional account.
    Agent {
        key_id: String,
        permission: Permission,
        account_id: Option<String>,
    },
}

impl AuthContext {
    /// Returns `Ok(())` if this context satisfies `needed`, or
    /// `Err(StatusCode::FORBIDDEN)` otherwise.
    /// `Session` always passes; `Agent` checks its permission level.
    pub fn require(&self, needed: Permission) -> Result<(), StatusCode> {
        match self {
            AuthContext::Session => Ok(()),
            AuthContext::Agent { permission, .. } => {
                if permission.satisfies(needed) {
                    Ok(())
                } else {
                    Err(StatusCode::FORBIDDEN)
                }
            }
        }
    }

    /// Check permission, returning a JSON error tuple on failure.
    pub fn require_json(&self, needed: Permission) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        self.require(needed).map_err(|s| (s, Json(serde_json::json!({"error": "insufficient permissions"}))))
    }

    /// Returns the `account_id` scope for agent keys, or `None` for sessions
    /// and agent keys without an account restriction.
    pub fn account_scope(&self) -> Option<&str> {
        match self {
            AuthContext::Session => None,
            AuthContext::Agent { account_id, .. } => account_id.as_deref(),
        }
    }
}

// ---------------------------------------------------------------------------
// Unified auth middleware
// ---------------------------------------------------------------------------

/// Extract a Bearer token from the Authorization header (case-insensitive scheme per RFC 7235).
pub fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    if value.len() > 7 && value[..7].eq_ignore_ascii_case("bearer ") {
        Some(value[7..].trim().to_string())
    } else {
        None
    }
}

/// Look up an API key by its SHA-256 hash.
/// Returns (id, permission, account_id, is_revoked, last_used_at).
fn lookup_api_key(
    conn: &rusqlite::Connection,
    key_hash: &str,
) -> Option<(String, String, Option<String>, bool, Option<i64>)> {
    conn.query_row(
        "SELECT id, permission, account_id, is_revoked, last_used_at FROM api_keys WHERE key_hash = ?1",
        rusqlite::params![key_hash],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    )
    .ok()
}

/// Middleware that accepts both API-key Bearer tokens and session tokens.
///
/// Check order:
/// 1. Bearer token (`Authorization: Bearer iris_...`) — hash, look up in
///    `api_keys`, reject if missing/revoked, insert `AuthContext::Agent`.
/// 2. Session token (`X-Session-Token` header or `iris_session` cookie) —
///    validate against `state.session_token`, CSRF check for cookie transport
///    on mutating methods, insert `AuthContext::Session`.
/// 3. Neither present — 401.
pub async fn unified_auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    // --- Path 1: Bearer token ---
    if let Some(raw_key) = extract_bearer_token(request.headers()) {
        let key_hash = format!("{:x}", Sha256::digest(raw_key.as_bytes()));

        // Scope the DB connection so it's returned to the pool before
        // next.run() — downstream handlers and nested middleware need
        // their own connections.
        let auth_result = {
            let conn = match state.db.get() {
                Ok(c) => c,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };

            match lookup_api_key(&conn, &key_hash) {
                Some((id, permission_str, account_id, is_revoked, last_used)) => {
                    if is_revoked {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }

                    let permission = match Permission::from_str(&permission_str) {
                        Some(p) => p,
                        None => return StatusCode::UNAUTHORIZED.into_response(),
                    };

                    // Debounce last_used_at — only write if older than 60s to avoid
                    // a SQLite write lock on every agent request.
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let stale = last_used.map_or(true, |t| now - t > 60);
                    if stale {
                        let _ = conn.execute(
                            "UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2",
                            rusqlite::params![now, id],
                        );
                    }

                    Some(AuthContext::Agent {
                        key_id: id,
                        permission,
                        account_id,
                    })
                }
                None => None,
            }
        }; // conn is dropped here

        match auth_result {
            Some(ctx) => {
                request.extensions_mut().insert(ctx);
                return next.run(request).await;
            }
            None => {
                return StatusCode::UNAUTHORIZED.into_response();
            }
        }
    }

    // --- Path 2: Session token ---
    if let Some((token, transport)) = extract_session_token(request.headers()) {
        if token == state.session_token {
            // CSRF check: cookie-transported token on a mutating method must
            // come from a same-origin browser context.
            if transport == SessionTransport::Cookie
                && !is_safe_method(request.method())
                && !is_same_origin_browser_context(request.headers())
            {
                return StatusCode::FORBIDDEN.into_response();
            }

            request.extensions_mut().insert(AuthContext::Session);
            return next.run(request).await;
        }
    }

    // --- Path 3: No valid credentials ---
    StatusCode::UNAUTHORIZED.into_response()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_hierarchy() {
        use Permission::*;

        // Every permission satisfies itself
        assert!(ReadOnly.satisfies(ReadOnly));
        assert!(DraftOnly.satisfies(DraftOnly));
        assert!(SendWithApproval.satisfies(SendWithApproval));
        assert!(Autonomous.satisfies(Autonomous));

        // Higher permissions satisfy lower ones
        assert!(DraftOnly.satisfies(ReadOnly));
        assert!(SendWithApproval.satisfies(ReadOnly));
        assert!(SendWithApproval.satisfies(DraftOnly));
        assert!(Autonomous.satisfies(ReadOnly));
        assert!(Autonomous.satisfies(DraftOnly));
        assert!(Autonomous.satisfies(SendWithApproval));

        // Lower permissions do NOT satisfy higher ones
        assert!(!ReadOnly.satisfies(DraftOnly));
        assert!(!ReadOnly.satisfies(SendWithApproval));
        assert!(!ReadOnly.satisfies(Autonomous));
        assert!(!DraftOnly.satisfies(SendWithApproval));
        assert!(!DraftOnly.satisfies(Autonomous));
        assert!(!SendWithApproval.satisfies(Autonomous));
    }

    #[test]
    fn test_permission_from_str() {
        assert_eq!(Permission::from_str("read_only"), Some(Permission::ReadOnly));
        assert_eq!(Permission::from_str("draft_only"), Some(Permission::DraftOnly));
        assert_eq!(
            Permission::from_str("send_with_approval"),
            Some(Permission::SendWithApproval)
        );
        assert_eq!(Permission::from_str("autonomous"), Some(Permission::Autonomous));
        assert_eq!(Permission::from_str("invalid"), None);
        assert_eq!(Permission::from_str(""), None);
        assert_eq!(Permission::from_str("ReadOnly"), None);
    }

    #[test]
    fn test_auth_context_session_always_passes() {
        let ctx = AuthContext::Session;
        assert!(ctx.require(Permission::ReadOnly).is_ok());
        assert!(ctx.require(Permission::DraftOnly).is_ok());
        assert!(ctx.require(Permission::SendWithApproval).is_ok());
        assert!(ctx.require(Permission::Autonomous).is_ok());
    }

    #[test]
    fn test_auth_context_agent_permission_denied() {
        let ctx = AuthContext::Agent {
            key_id: "key_test".to_string(),
            permission: Permission::ReadOnly,
            account_id: None,
        };
        // ReadOnly cannot satisfy SendWithApproval
        assert_eq!(
            ctx.require(Permission::SendWithApproval),
            Err(StatusCode::FORBIDDEN)
        );
        // ReadOnly cannot satisfy DraftOnly or Autonomous either
        assert_eq!(ctx.require(Permission::DraftOnly), Err(StatusCode::FORBIDDEN));
        assert_eq!(ctx.require(Permission::Autonomous), Err(StatusCode::FORBIDDEN));
        // ReadOnly can satisfy itself
        assert!(ctx.require(Permission::ReadOnly).is_ok());
    }

    #[test]
    fn test_account_scope() {
        assert_eq!(AuthContext::Session.account_scope(), None);

        let with_account = AuthContext::Agent {
            key_id: "k1".to_string(),
            permission: Permission::Autonomous,
            account_id: Some("acct_123".to_string()),
        };
        assert_eq!(with_account.account_scope(), Some("acct_123"));

        let without_account = AuthContext::Agent {
            key_id: "k2".to_string(),
            permission: Permission::ReadOnly,
            account_id: None,
        };
        assert_eq!(without_account.account_scope(), None);
    }
}
