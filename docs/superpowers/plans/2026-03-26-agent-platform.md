# Agent Platform & Memories v5 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make all 200+ Iris API routes accessible to external agents via API key auth, upgrade Memories integration to v5, and add reply/forward convenience endpoints.

**Architecture:** Unified auth middleware replaces separate session/agent auth. The middleware checks Bearer token (API key) first, then session token/cookie. Permission levels gate write/send/config operations. Memories client gains temporal and graph search parameters. Reply/forward endpoints resolve threading context server-side.

**Tech Stack:** Rust/Axum (backend), tower-governor (rate limiting), reqwest (Memories HTTP client), existing SMTP/compose infrastructure.

**Spec:** `docs/superpowers/specs/2026-03-26-agent-platform-design.md`

---

### Task 1: Unified Auth Types & Permission Checking

**Files:**
- Create: `src/api/unified_auth.rs`
- Modify: `src/api/mod.rs`

- [ ] **Step 1: Write the failing test for permission hierarchy**

```rust
// In src/api/unified_auth.rs at the bottom
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_hierarchy() {
        assert!(Permission::ReadOnly.satisfies(Permission::ReadOnly));
        assert!(!Permission::ReadOnly.satisfies(Permission::DraftOnly));
        assert!(Permission::DraftOnly.satisfies(Permission::ReadOnly));
        assert!(Permission::DraftOnly.satisfies(Permission::DraftOnly));
        assert!(!Permission::DraftOnly.satisfies(Permission::SendWithApproval));
        assert!(Permission::Autonomous.satisfies(Permission::ReadOnly));
        assert!(Permission::Autonomous.satisfies(Permission::SendWithApproval));
        assert!(Permission::Autonomous.satisfies(Permission::Autonomous));
    }

    #[test]
    fn test_permission_from_str() {
        assert_eq!(Permission::from_str("read_only"), Some(Permission::ReadOnly));
        assert_eq!(Permission::from_str("draft_only"), Some(Permission::DraftOnly));
        assert_eq!(Permission::from_str("send_with_approval"), Some(Permission::SendWithApproval));
        assert_eq!(Permission::from_str("autonomous"), Some(Permission::Autonomous));
        assert_eq!(Permission::from_str("invalid"), None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test unified_auth -p iris-server -- --nocapture`
Expected: FAIL — module doesn't exist yet

- [ ] **Step 3: Write the types and permission logic**

```rust
// src/api/unified_auth.rs

/// Auth context attached to request extensions by unified_auth_middleware.
#[derive(Clone, Debug)]
pub enum AuthContext {
    /// Session-based auth (UI). Full access, no permission restrictions.
    Session,
    /// API key auth (agents). Permission-gated.
    Agent {
        key_id: String,
        permission: Permission,
        account_id: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Permission {
    ReadOnly,
    DraftOnly,
    SendWithApproval,
    Autonomous,
}

impl Permission {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "read_only" => Some(Self::ReadOnly),
            "draft_only" => Some(Self::DraftOnly),
            "send_with_approval" => Some(Self::SendWithApproval),
            "autonomous" => Some(Self::Autonomous),
            _ => None,
        }
    }

    /// Returns true if this permission level is sufficient for `needed`.
    pub fn satisfies(self, needed: Permission) -> bool {
        self >= needed
    }
}

impl AuthContext {
    /// Check if the auth context has sufficient permission. Session auth always passes.
    pub fn require(&self, needed: Permission) -> Result<(), axum::http::StatusCode> {
        match self {
            AuthContext::Session => Ok(()),
            AuthContext::Agent { permission, .. } => {
                if permission.satisfies(needed) {
                    Ok(())
                } else {
                    Err(axum::http::StatusCode::FORBIDDEN)
                }
            }
        }
    }

    /// Returns the account scope if this is an agent key with account_id set.
    pub fn account_scope(&self) -> Option<&str> {
        match self {
            AuthContext::Agent { account_id, .. } => account_id.as_deref(),
            AuthContext::Session => None,
        }
    }
}
```

- [ ] **Step 4: Register the module in mod.rs**

Add `pub mod unified_auth;` to `src/api/mod.rs` (alphabetical order, after `trust`).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test unified_auth -p iris-server -- --nocapture`
Expected: PASS — both tests green

- [ ] **Step 6: Commit**

```bash
git add src/api/unified_auth.rs src/api/mod.rs
git commit -m "feat: add unified auth types and permission hierarchy"
```

---

### Task 2: Unified Auth Middleware

**Files:**
- Modify: `src/api/unified_auth.rs` (add middleware function)
- Modify: `src/api/agent.rs` (reuse API key lookup)

- [ ] **Step 1: Write the failing integration test**

```rust
// In tests/security_auth.rs — add a new test
#[tokio::test]
async fn test_unified_auth_accepts_api_key_on_protected_route() {
    let (state, _pool) = create_test_state();
    let app = iris_server::build_app(state.clone());

    // Create an API key
    let conn = state.db.get().unwrap();
    let key_resp = iris_server::api::agent::create_api_key(&conn, "test-agent", "read_only", None);
    let raw_key = key_resp.unwrap();

    // Use the API key on a protected route (GET /api/messages)
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/messages")
                .header("Authorization", format!("Bearer {}", raw_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should succeed (200) not 401
    assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_unified_auth_accepts_api_key -p iris-server -- --nocapture`
Expected: FAIL — API key returns 401 on protected routes (session auth only)

- [ ] **Step 3: Implement the unified middleware**

Add to `src/api/unified_auth.rs`:

```rust
use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use crate::AppState;
use crate::api::session_auth::{extract_session_token, is_same_origin_browser_context, SessionTransport};

/// Unified auth middleware: accepts Bearer API key OR session token/cookie.
pub async fn unified_auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    // 1. Check for Bearer API key first
    if let Some(raw_key) = extract_bearer_token(request.headers()) {
        let hash = format!("{:x}", Sha256::digest(raw_key.as_bytes()));
        let conn = match state.db.get() {
            Ok(c) => c,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };

        match lookup_api_key(&conn, &hash) {
            Some((key_id, permission_str, account_id, is_revoked)) => {
                if is_revoked {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                let Some(permission) = Permission::from_str(&permission_str) else {
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                };

                // Update last_used_at
                let _ = conn.execute(
                    "UPDATE api_keys SET last_used_at = strftime('%s', 'now') WHERE id = ?1",
                    rusqlite::params![key_id],
                );

                request.extensions_mut().insert(AuthContext::Agent {
                    key_id,
                    permission,
                    account_id,
                });
                return next.run(request).await;
            }
            None => return StatusCode::UNAUTHORIZED.into_response(),
        }
    }

    // 2. Fall back to session token (header or cookie)
    let auth = extract_session_token(request.headers());
    match auth {
        Some((token, transport)) if token == state.session_token => {
            // CSRF check for cookie-based mutations
            if transport == SessionTransport::Cookie
                && !is_safe_method(request.method())
                && !is_same_origin_browser_context(request.headers())
            {
                return StatusCode::FORBIDDEN.into_response();
            }
            request.extensions_mut().insert(AuthContext::Session);
            next.run(request).await
        }
        _ => StatusCode::UNAUTHORIZED.into_response(),
    }
}

fn extract_bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

fn is_safe_method(method: &axum::http::Method) -> bool {
    matches!(*method, axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS)
}

fn lookup_api_key(conn: &rusqlite::Connection, key_hash: &str) -> Option<(String, String, Option<String>, bool)> {
    conn.query_row(
        "SELECT id, permission, account_id, is_revoked FROM api_keys WHERE key_hash = ?1",
        rusqlite::params![key_hash],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .ok()
}
```

- [ ] **Step 4: Replace session_auth_middleware with unified_auth_middleware in lib.rs**

In `src/lib.rs`, change the protected_api route_layer:

```rust
// Before:
.route_layer(middleware::from_fn_with_state(
    state.clone(),
    api::session_auth::session_auth_middleware,
));

// After:
.route_layer(middleware::from_fn_with_state(
    state.clone(),
    api::unified_auth::unified_auth_middleware,
));
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p iris-server -- --nocapture`
Expected: All existing tests pass + new test passes

- [ ] **Step 6: Commit**

```bash
git add src/api/unified_auth.rs src/lib.rs
git commit -m "feat: unified auth middleware — API keys work on all protected routes"
```

---

### Task 3: Rate Limit Key Extraction for API Keys

**Files:**
- Modify: `src/api/rate_limit.rs`

- [ ] **Step 1: Write the failing test**

```rust
// In src/api/rate_limit.rs tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extracts_bearer_key_id_prefix() {
        // Bearer token should extract a key-based rate limit key, not __anonymous__
        let mut req = http::Request::builder()
            .header("Authorization", "Bearer iris_abc123def456")
            .body(())
            .unwrap();
        let key = SessionTokenKeyExtractor.extract(&req).unwrap();
        assert!(key.starts_with("agent:"));
        assert_ne!(key, "__anonymous__");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test rate_limit::tests -p iris-server -- --nocapture`
Expected: FAIL — currently returns `__anonymous__` for Bearer tokens

- [ ] **Step 3: Update the key extractor**

Modify `SessionTokenKeyExtractor::extract` in `src/api/rate_limit.rs`:

```rust
fn extract<T>(&self, req: &http::Request<T>) -> Result<Self::Key, GovernorError> {
    // Check for Bearer API key first — use key prefix as rate limit bucket
    if let Some(bearer) = req.headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        // Use first 16 chars as bucket key (the key prefix, avoids storing full key)
        let prefix = &bearer[..bearer.len().min(16)];
        return Ok(format!("agent:{}", prefix));
    }

    // Fall back to session token
    let key = crate::api::session_auth::extract_session_token(req.headers())
        .map(|(token, _)| token)
        .unwrap_or_else(|| "__anonymous__".to_owned());
    Ok(key)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p iris-server -- --nocapture`
Expected: All pass including new rate limit test

- [ ] **Step 5: Commit**

```bash
git add src/api/rate_limit.rs
git commit -m "feat: per-API-key rate limiting buckets"
```

---

### Task 4: Permission Checks on Sensitive Endpoints

**Files:**
- Modify: `src/api/config.rs` (add permission check)
- Modify: `src/api/agent.rs` (add permission check to list_keys_handler, audit_log)
- Modify: `src/api/compose.rs` (add permission check to send_message)

- [ ] **Step 1: Write the failing test**

```rust
// In tests/security_auth.rs
#[tokio::test]
async fn test_read_only_key_cannot_access_config() {
    let (state, _pool) = create_test_state();
    let app = iris_server::build_app(state.clone());

    let conn = state.db.get().unwrap();
    let raw_key = iris_server::api::agent::create_api_key(&conn, "reader", "read_only", None).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .header("Authorization", format!("Bearer {}", raw_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_read_only_key_cannot_access_config -p iris-server -- --nocapture`
Expected: FAIL — returns 200 (no permission check yet)

- [ ] **Step 3: Add permission checks to sensitive handlers**

At the top of each sensitive handler, add:

```rust
// In config handlers (get_config, get_appearance, set_theme, etc.):
use crate::api::unified_auth::{AuthContext, Permission};

// For config GET endpoints:
if let Some(auth) = request.extensions().get::<AuthContext>() {
    auth.require(Permission::Autonomous)?;
}

// For send_message:
if let Some(auth) = request.extensions().get::<AuthContext>() {
    auth.require(Permission::SendWithApproval)?;
}

// For draft/mutation handlers:
if let Some(auth) = request.extensions().get::<AuthContext>() {
    auth.require(Permission::DraftOnly)?;
}
```

Apply to these handlers:
- `get_config`, `set_theme`, `set_view_mode`, `get_appearance`, `set_appearance` -> `Autonomous`
- `get_ai_config`, `set_ai_config`, `test_ai_connection` -> `Autonomous`
- `list_keys_handler`, `get_audit_log_handler` -> `Autonomous`
- `send_message`, `cancel_send` -> `SendWithApproval`
- `save_draft`, `delete_draft`, `batch_update_messages` -> `DraftOnly`

Note: The `AuthContext` extension is only present when the middleware runs (protected routes). Session auth inserts `AuthContext::Session` which always passes `require()`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p iris-server -- --nocapture`
Expected: All pass including new permission test

- [ ] **Step 5: Commit**

```bash
git add src/api/config.rs src/api/agent.rs src/api/compose.rs src/api/ai_config.rs src/api/messages.rs
git commit -m "feat: permission checks on sensitive endpoints for API key auth"
```

---

### Task 5: Memories Client v5 — Upsert with document_at

**Files:**
- Modify: `src/ai/memories.rs`

- [ ] **Step 1: Write the failing test**

```rust
// In src/ai/memories.rs tests
#[test]
fn test_upsert_request_with_document_at() {
    let req = UpsertRequest {
        text: "test".into(),
        source: "iris/1/messages/abc".into(),
        key: "key".into(),
        document_at: Some("2026-03-15T10:30:00Z".into()),
    };
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["document_at"], "2026-03-15T10:30:00Z");
}

#[test]
fn test_upsert_request_without_document_at() {
    let req = UpsertRequest {
        text: "test".into(),
        source: "iris/1/messages/abc".into(),
        key: "key".into(),
        document_at: None,
    };
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("document_at").is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test memories::tests -p iris-server -- --nocapture`
Expected: FAIL — `UpsertRequest` has no `document_at` field

- [ ] **Step 3: Add document_at to upsert structs and methods**

In `src/ai/memories.rs`:

Add `document_at` to `UpsertRequest` and `UpsertEntry`:
```rust
#[derive(Debug, Serialize)]
pub struct UpsertEntry {
    pub text: String,
    pub source: String,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct UpsertRequest {
    text: String,
    source: String,
    key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    document_at: Option<String>,
}
```

Update `upsert` method signature:
```rust
pub async fn upsert(&self, text: &str, source: &str, key: &str, document_at: Option<&str>) -> bool {
    let body = UpsertRequest {
        text: text.to_string(),
        source: source.to_string(),
        key: key.to_string(),
        document_at: document_at.map(|s| s.to_string()),
    };
    // ... rest unchanged
}
```

- [ ] **Step 4: Fix all callers of upsert**

Search for `.upsert(` calls and add `None` for the new parameter where no date is available, or pass the email date where it is. Key callers:
- `src/jobs/worker.rs` — `memories_store` job: pass the email's `date` field
- `src/jobs/worker.rs` — `chat_summarize` job: pass `None`
- `src/jobs/worker.rs` — `pref_extract` job: pass `None`

- [ ] **Step 5: Run tests**

Run: `cargo test -p iris-server -- --nocapture`
Expected: All pass

- [ ] **Step 6: Commit**

```bash
git add src/ai/memories.rs src/jobs/worker.rs
git commit -m "feat: add document_at to Memories upsert for temporal search"
```

---

### Task 6: Memories Client v5 — Search with Temporal & Graph Params

**Files:**
- Modify: `src/ai/memories.rs`
- Modify: `src/api/search.rs`

- [ ] **Step 1: Write the failing test**

```rust
// In src/ai/memories.rs tests
#[test]
fn test_search_request_with_v5_params() {
    let req = SearchRequest {
        query: "budget".into(),
        k: 10,
        hybrid: true,
        source: Some("iris/".into()),
        since: Some("2026-03-01".into()),
        until: Some("2026-03-15".into()),
        graph_weight: Some(0.1),
        recency_weight: Some(0.0),
        confidence_weight: Some(0.0),
    };
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["since"], "2026-03-01");
    assert_eq!(json["until"], "2026-03-15");
    assert_eq!(json["graph_weight"], 0.1);
    assert_eq!(json["recency_weight"], 0.0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_search_request_with_v5 -p iris-server -- --nocapture`
Expected: FAIL — `SearchRequest` doesn't have these fields

- [ ] **Step 3: Add v5 params to SearchRequest and search method**

Update `SearchRequest` in `src/ai/memories.rs`:
```rust
#[derive(Debug, Serialize)]
struct SearchRequest {
    query: String,
    k: usize,
    hybrid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    until: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    graph_weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recency_weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence_weight: Option<f64>,
}
```

Add a new `SearchOptions` struct for callers and update the search method:
```rust
#[derive(Default)]
pub struct SearchOptions {
    pub since: Option<String>,
    pub until: Option<String>,
    pub graph_weight: Option<f64>,
    pub recency_weight: Option<f64>,
    pub confidence_weight: Option<f64>,
}

pub async fn search(
    &self,
    query: &str,
    k: usize,
    source_prefix: Option<&str>,
    options: SearchOptions,
) -> Vec<MemoryResult> {
    let body = SearchRequest {
        query: query.to_string(),
        k,
        hybrid: true,
        source: source_prefix.map(|s| s.to_string()),
        since: options.since,
        until: options.until,
        graph_weight: options.graph_weight,
        recency_weight: options.recency_weight,
        confidence_weight: options.confidence_weight,
    };
    // ... rest unchanged
}
```

- [ ] **Step 4: Update all callers of search**

- `src/api/search.rs` — pass `SearchOptions` with email-tuned defaults:
  ```rust
  SearchOptions {
      since: params.since.clone(),
      until: params.until.clone(),
      graph_weight: Some(0.1),
      recency_weight: Some(0.0),
      confidence_weight: Some(0.0),
  }
  ```
- `src/api/search.rs` — add `since` and `until` to the search query params struct
- Any other callers of `memories.search()` — pass `SearchOptions::default()`

- [ ] **Step 5: Run tests**

Run: `cargo test -p iris-server -- --nocapture`
Expected: All pass

- [ ] **Step 6: Commit**

```bash
git add src/ai/memories.rs src/api/search.rs
git commit -m "feat: Memories v5 search — temporal filters, graph weight, decay tuning"
```

---

### Task 7: Reply & Forward Endpoints

**Files:**
- Create: `src/api/reply_forward.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/lib.rs` (add routes)

- [ ] **Step 1: Write the failing test**

```rust
// In tests/api_integration.rs
#[tokio::test]
async fn test_reply_endpoint_returns_404_for_missing_message() {
    let (state, _pool) = create_test_state();
    let app = iris_server::build_app(state.clone());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/reply")
                .header("X-Session-Token", &state.session_token)
                .header("Content-Type", "application/json")
                .header("Sec-Fetch-Site", "same-origin")
                .body(Body::from(r#"{"message_id":"nonexistent","body":"test","reply_all":false}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_reply_endpoint -p iris-server -- --nocapture`
Expected: FAIL — route doesn't exist (404 from fallback, but wrong reason)

- [ ] **Step 3: Create reply_forward module**

Create `src/api/reply_forward.rs` with:
- `ReplyRequest` struct: `{ message_id: String, body: String, reply_all: bool }`
- `ForwardRequest` struct: `{ message_id: String, to: Vec<String>, body: String }`
- `reply` handler: looks up original message, builds ComposeRequest with In-Reply-To/References/Re: subject, delegates to send_message logic
- `forward` handler: looks up original message, builds ComposeRequest with Fwd: subject, includes original body
- `draft_reply` handler: same as reply but saves as draft
- `draft_forward` handler: same as forward but saves as draft
- Shared `resolve_reply_context` function: loads message from DB, builds threading headers and recipient list
- Permission checks: reply/forward require `SendWithApproval`, draft variants require `DraftOnly`
- Account scope checks for API key auth

- [ ] **Step 4: Register module and routes**

Add `pub mod reply_forward;` to `src/api/mod.rs`.

Add routes to `src/lib.rs` in the protected_api block:
```rust
.route("/reply", post(api::reply_forward::reply))
.route("/forward", post(api::reply_forward::forward))
.route("/drafts/reply", post(api::reply_forward::draft_reply))
.route("/drafts/forward", post(api::reply_forward::draft_forward))
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p iris-server -- --nocapture`
Expected: All pass — reply endpoint returns 404 for missing message (correct behavior)

- [ ] **Step 6: Commit**

```bash
git add src/api/reply_forward.rs src/api/mod.rs src/lib.rs
git commit -m "feat: reply/forward endpoints with threading header resolution"
```

---

### Task 8: MCP Auth Alignment & Tool Permissions

**Files:**
- Modify: `src/api/mcp_server.rs`

- [ ] **Step 1: Write the failing test**

```rust
// In tests/security_auth.rs
#[tokio::test]
async fn test_mcp_initialize_accepts_api_key() {
    let (state, _pool) = create_test_state();
    let app = iris_server::build_app(state.clone());

    let conn = state.db.get().unwrap();
    let raw_key = iris_server::api::agent::create_api_key(&conn, "mcp-agent", "read_only", None).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/mcp/initialize")
                .header("Authorization", format!("Bearer {}", raw_key))
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"capabilities":[]}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

- [ ] **Step 2: Run test to verify it fails or passes**

Run: `cargo test test_mcp_initialize_accepts -p iris-server -- --nocapture`

If it passes: MCP initialize already works with unified auth (it's on protected routes). Skip to permission checking.

- [ ] **Step 3: Add permission checking to MCP tool execution**

In `src/api/mcp_server.rs`, in the `execute_tool` function, add permission checks based on the tool name:

```rust
fn tool_permission(tool_name: &str) -> Permission {
    match tool_name {
        "search_emails" | "read_email" | "list_inbox" | "list_threads"
        | "get_thread" | "get_thread_summary" | "get_contact_profile"
        | "get_inbox_stats" | "extract_tasks" | "extract_deadlines" => Permission::ReadOnly,

        "create_draft" | "manage_draft" | "archive_email" | "star_email"
        | "bulk_action" => Permission::DraftOnly,

        "send_email" | "chat" => Permission::SendWithApproval,

        _ => Permission::Autonomous,
    }
}
```

Before executing any tool, check:
```rust
if let Some(auth) = extensions.get::<AuthContext>() {
    let needed = tool_permission(&tool_name);
    if let Err(_) = auth.require(needed) {
        return tool_error(format!("Insufficient permission for tool '{}'. Requires {:?}.", tool_name, needed));
    }
}
```

- [ ] **Step 4: Store permission in MCP session**

When creating an MCP session, store the `AuthContext` permission level in the `mcp_sessions` table (or in-memory on the session struct) so it persists across tool calls within the session.

- [ ] **Step 5: Run tests**

Run: `cargo test -p iris-server -- --nocapture`
Expected: All pass

- [ ] **Step 6: Commit**

```bash
git add src/api/mcp_server.rs
git commit -m "feat: MCP auth alignment — API keys + tool permission checks"
```

---

### Task 9: Integration Verification & Docker Build

**Files:**
- Modify: `tests/security_auth.rs` (add comprehensive tests)

- [ ] **Step 1: Add end-to-end agent flow test**

```rust
#[tokio::test]
async fn test_agent_full_flow_read_search_draft() {
    let (state, _pool) = create_test_state();
    let app = iris_server::build_app(state.clone());

    let conn = state.db.get().unwrap();
    let raw_key = iris_server::api::agent::create_api_key(&conn, "flow-test", "draft_only", None).unwrap();

    // Read messages
    let resp = send_request(&app, "GET", "/api/messages", &raw_key, None).await;
    assert_eq!(resp.status(), 200);

    // Search
    let resp = send_request(&app, "GET", "/api/search?q=test", &raw_key, None).await;
    assert_eq!(resp.status(), 200);

    // Save draft (allowed for draft_only)
    let draft_body = r#"{"account_id":"test","to":["a@b.com"],"subject":"test","body_text":"hi"}"#;
    let resp = send_request(&app, "POST", "/api/drafts", &raw_key, Some(draft_body)).await;
    assert_ne!(resp.status(), 403);

    // Send (NOT allowed for draft_only)
    let resp = send_request(&app, "POST", "/api/send", &raw_key, Some(draft_body)).await;
    assert_eq!(resp.status(), 403);
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test -p iris-server -- --nocapture`
Expected: All pass

- [ ] **Step 3: Docker build verification**

Run: `~/.orbstack/bin/docker compose -f docker-compose.yml up -d --build iris`
Expected: Build succeeds, container starts, health check passes

- [ ] **Step 4: Manual API key test against running instance**

```bash
# Bootstrap and create an API key via UI or curl
# Then test agent access:
curl -s http://127.0.0.1:3000/api/messages \
  -H "Authorization: Bearer iris_<your_key>" | head -c 200
```

Expected: JSON response with messages (not 401)

- [ ] **Step 5: Commit test additions**

```bash
git add tests/
git commit -m "test: end-to-end agent flow and permission enforcement tests"
```

---

## Task Summary

| Task | Scope | Key Files |
|------|-------|-----------|
| 1 | Auth types & permission hierarchy | `src/api/unified_auth.rs` |
| 2 | Unified auth middleware | `src/api/unified_auth.rs`, `src/lib.rs` |
| 3 | Rate limit per API key | `src/api/rate_limit.rs` |
| 4 | Permission checks on sensitive endpoints | `src/api/config.rs`, `compose.rs`, `agent.rs` |
| 5 | Memories upsert with document_at | `src/ai/memories.rs`, `src/jobs/worker.rs` |
| 6 | Memories search with v5 params | `src/ai/memories.rs`, `src/api/search.rs` |
| 7 | Reply & forward endpoints | `src/api/reply_forward.rs`, `src/lib.rs` |
| 8 | MCP auth + tool permissions | `src/api/mcp_server.rs` |
| 9 | Integration tests + Docker build | `tests/`, Docker |
