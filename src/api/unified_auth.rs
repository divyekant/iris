use axum::http::StatusCode;

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
