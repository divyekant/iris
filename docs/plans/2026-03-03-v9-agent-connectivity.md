# V9: Agent Connectivity Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add trust indicators (SPF/DKIM/DMARC badges, tracking pixel detection), API key management with permission-based access control, agent REST API, and audit logging.

**Architecture:** V9 adds three pillars: (1) email trust/privacy indicators parsed from stored headers and HTML bodies, (2) a scoped API key system with 4 permission levels enabling external agent access to inbox operations, and (3) an append-only audit trail logging every agent action. Agent-facing REST endpoints reuse existing business logic (search, messages, send) but are gated behind API key auth middleware.

**Tech Stack:** Rust (Axum middleware, SHA-256 key hashing), SQLite (migration 003), Svelte 5 frontend

**Scoping notes:**
- MCP protocol deferred — REST API provides equivalent functionality; MCP can wrap it later
- Webhooks (N62/S16) deferred — nice-to-have, not essential for core agent access
- Keyboard shortcuts (U55/N24/S3) deferred — unrelated to agent connectivity theme

---

## Task 1: Database Migration (S13, S17)

**Files:**
- Create: `migrations/003_agent.sql`
- Modify: `src/db/migrations.rs`

**Step 1: Write migration SQL**

Create `migrations/003_agent.sql`:

```sql
-- migrations/003_agent.sql

-- API keys for agent access (S13)
CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,                -- human-readable label ("Claude agent", "Zapier")
    key_hash TEXT NOT NULL UNIQUE,     -- SHA-256 hash of the actual key
    key_prefix TEXT NOT NULL,          -- first 8 chars for display ("iris_ak_1a2b...")
    permission TEXT NOT NULL DEFAULT 'read_only'
        CHECK(permission IN ('read_only', 'draft_only', 'send_with_approval', 'autonomous')),
    account_id TEXT REFERENCES accounts(id),  -- NULL = all accounts, set = scoped to one
    is_revoked INTEGER DEFAULT 0,
    last_used_at INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    revoked_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);

-- Audit log for agent actions (S17)
CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    api_key_id TEXT NOT NULL REFERENCES api_keys(id),
    action TEXT NOT NULL,              -- "search", "read", "send", "draft", "archive", etc.
    resource_type TEXT,                -- "message", "thread", "draft"
    resource_id TEXT,                  -- message/thread ID acted upon
    details TEXT,                      -- JSON: request summary (query params, etc.)
    status TEXT NOT NULL DEFAULT 'success',  -- "success", "denied", "error"
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_audit_log_key ON audit_log(api_key_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_log_time ON audit_log(created_at DESC);

INSERT INTO schema_version (version) VALUES (3);
```

**Step 2: Update migrations.rs**

In `src/db/migrations.rs`, add:

```rust
const MIGRATION_003: &str = include_str!("../../migrations/003_agent.sql");

// In run() function, after migration 002 check:
let has_v3: bool = conn
    .query_row("SELECT COUNT(*) FROM schema_version WHERE version = 3", [], |row| row.get(0))
    .unwrap_or(false);
if !has_v3 {
    conn.execute_batch(MIGRATION_003)?;
    tracing::info!("Applied migration 003 (agent tables)");
}
```

**Step 3: Run tests**

Run: `cargo test`
Expected: All 54 existing tests still pass (migration is additive).

**Step 4: Commit**

```bash
git add migrations/003_agent.sql src/db/migrations.rs
git commit -m "feat(v9): migration 003 — api_keys and audit_log tables"
```

---

## Task 2: Trust Indicators — Backend (U17, N61)

**Files:**
- Create: `src/api/trust.rs`
- Modify: `src/api/mod.rs`

Trust badges parse the `Authentication-Results` header from the `raw_headers` column (already stored during IMAP sync). No external lookups needed.

**Step 1: Write tests**

In `src/api/trust.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auth_results_all_pass() {
        let header = "Authentication-Results: mx.google.com; \
            dkim=pass header.d=example.com; \
            spf=pass (google.com: domain of sender@example.com); \
            dmarc=pass (p=REJECT)";
        let result = parse_authentication_results(header);
        assert_eq!(result.spf, Some(AuthStatus::Pass));
        assert_eq!(result.dkim, Some(AuthStatus::Pass));
        assert_eq!(result.dmarc, Some(AuthStatus::Pass));
    }

    #[test]
    fn test_parse_auth_results_mixed() {
        let header = "Authentication-Results: mx.example.com; \
            spf=fail; dkim=none; dmarc=fail";
        let result = parse_authentication_results(header);
        assert_eq!(result.spf, Some(AuthStatus::Fail));
        assert_eq!(result.dkim, Some(AuthStatus::None));
        assert_eq!(result.dmarc, Some(AuthStatus::Fail));
    }

    #[test]
    fn test_parse_auth_results_missing_header() {
        let headers = "From: sender@example.com\r\nSubject: Test\r\n";
        let result = extract_trust_indicators(headers);
        assert!(result.spf.is_none());
        assert!(result.dkim.is_none());
        assert!(result.dmarc.is_none());
    }

    #[test]
    fn test_detect_tracking_pixels_found() {
        let html = r#"<html><body>Hello<img src="https://track.mailchimp.com/open.gif" width="1" height="1"></body></html>"#;
        let trackers = detect_tracking_pixels(html);
        assert!(!trackers.is_empty());
    }

    #[test]
    fn test_detect_tracking_pixels_none() {
        let html = r#"<html><body><img src="photo.jpg" width="400" height="300"></body></html>"#;
        let trackers = detect_tracking_pixels(html);
        assert!(trackers.is_empty());
    }

    #[test]
    fn test_detect_tracking_pixels_known_domains() {
        let html = r#"<img src="https://pixel.watch/abc123"><img src="https://t.co/open">"#;
        let trackers = detect_tracking_pixels(html);
        assert!(!trackers.is_empty());
    }
}
```

**Step 2: Implement trust.rs**

```rust
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthStatus {
    Pass,
    Fail,
    Softfail,
    None,
    Neutral,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TrustIndicators {
    pub spf: Option<AuthStatus>,
    pub dkim: Option<AuthStatus>,
    pub dmarc: Option<AuthStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackingPixel {
    pub url: String,
    pub domain: String,
}

/// Known tracking pixel domains (partial list).
const KNOWN_TRACKER_DOMAINS: &[&str] = &[
    "mailchimp.com", "list-manage.com",
    "sendgrid.net", "sendgrid.com",
    "hubspot.com", "hsforms.com",
    "pixel.watch", "mailtrack.io",
    "google-analytics.com",
    "t.co", "clicks.beehiiv.com",
    "convertkit.com",
    "drip.com",
    "em.sailthru.com",
    "exact-target.com",
    "pardot.com",
    "mktotracking.com",
];

/// Parse an Authentication-Results header value into trust indicators.
pub fn parse_authentication_results(header_value: &str) -> TrustIndicators {
    let mut indicators = TrustIndicators::default();

    let parse_status = |s: &str| -> AuthStatus {
        match s.trim().to_lowercase().as_str() {
            "pass" => AuthStatus::Pass,
            "fail" => AuthStatus::Fail,
            "softfail" => AuthStatus::Softfail,
            "none" => AuthStatus::None,
            "neutral" => AuthStatus::Neutral,
            _ => AuthStatus::None,
        }
    };

    // Split by semicolons, look for spf=, dkim=, dmarc=
    for part in header_value.split(';') {
        let trimmed = part.trim();
        if let Some(rest) = trimmed.strip_prefix("spf=") {
            let status = rest.split_whitespace().next().unwrap_or("none")
                .split('(').next().unwrap_or("none");
            indicators.spf = Some(parse_status(status));
        } else if let Some(rest) = trimmed.strip_prefix("dkim=") {
            let status = rest.split_whitespace().next().unwrap_or("none")
                .split('(').next().unwrap_or("none");
            indicators.dkim = Some(parse_status(status));
        } else if let Some(rest) = trimmed.strip_prefix("dmarc=") {
            let status = rest.split_whitespace().next().unwrap_or("none")
                .split('(').next().unwrap_or("none");
            indicators.dmarc = Some(parse_status(status));
        }
    }

    indicators
}

/// Extract trust indicators from full raw_headers text.
/// Looks for the Authentication-Results header and parses it.
pub fn extract_trust_indicators(raw_headers: &str) -> TrustIndicators {
    // Find the Authentication-Results header
    for line in raw_headers.lines() {
        if line.to_lowercase().starts_with("authentication-results:") {
            let value = &line["authentication-results:".len()..];
            return parse_authentication_results(value);
        }
    }
    TrustIndicators::default()
}

/// Scan HTML body for tracking pixels.
/// Detects: 1x1 images, images from known tracker domains, and zero-size images.
pub fn detect_tracking_pixels(html: &str) -> Vec<TrackingPixel> {
    let mut trackers = Vec::new();

    // Simple regex-free approach: find <img> tags and check attributes
    let html_lower = html.to_lowercase();
    let mut search_from = 0;

    while let Some(img_start) = html_lower[search_from..].find("<img") {
        let abs_start = search_from + img_start;
        let tag_end = html_lower[abs_start..].find('>').map(|i| abs_start + i + 1);
        let tag_end = match tag_end {
            Some(e) => e,
            None => break,
        };

        let tag = &html[abs_start..tag_end];
        let tag_lower = &html_lower[abs_start..tag_end];

        // Extract src attribute
        if let Some(src) = extract_attr(tag, tag_lower, "src") {
            let is_tiny = is_tiny_image(tag_lower);
            let domain = extract_domain(&src);
            let is_known_tracker = domain.as_ref().map_or(false, |d| {
                KNOWN_TRACKER_DOMAINS.iter().any(|t| d.contains(t))
            });

            if is_tiny || is_known_tracker {
                trackers.push(TrackingPixel {
                    url: src,
                    domain: domain.unwrap_or_default(),
                });
            }
        }

        search_from = tag_end;
    }

    trackers
}

/// Check if an img tag represents a tiny (1x1 or 0x0) image.
fn is_tiny_image(tag_lower: &str) -> bool {
    let width = extract_numeric_attr(tag_lower, "width");
    let height = extract_numeric_attr(tag_lower, "height");

    match (width, height) {
        (Some(w), Some(h)) => w <= 1 && h <= 1,
        (Some(w), None) => w <= 1,
        (None, Some(h)) => h <= 1,
        _ => false,
    }
}

/// Extract an attribute value from an HTML tag (case-insensitive key).
fn extract_attr(tag: &str, tag_lower: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=", attr);
    let pos = tag_lower.find(&pattern)?;
    let after = &tag[pos + pattern.len()..];

    if after.starts_with('"') {
        let end = after[1..].find('"')?;
        Some(after[1..1 + end].to_string())
    } else if after.starts_with('\'') {
        let end = after[1..].find('\'')?;
        Some(after[1..1 + end].to_string())
    } else {
        let end = after.find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(after.len());
        Some(after[..end].to_string())
    }
}

/// Extract a numeric attribute value from an HTML tag.
fn extract_numeric_attr(tag_lower: &str, attr: &str) -> Option<u32> {
    let val = extract_attr(tag_lower, tag_lower, attr)?;
    val.trim().parse().ok()
}

/// Extract domain from a URL.
fn extract_domain(url: &str) -> Option<String> {
    let without_protocol = url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let domain = without_protocol.split('/').next()?;
    Some(domain.to_lowercase())
}
```

**Step 3: Register module**

Add to `src/api/mod.rs`:
```rust
pub mod trust;
```

**Step 4: Run tests**

Run: `cargo test api::trust`
Expected: All 6 trust tests pass.

**Step 5: Commit**

```bash
git add src/api/trust.rs src/api/mod.rs
git commit -m "feat(v9): trust indicators — SPF/DKIM/DMARC parsing and tracking pixel detection"
```

---

## Task 3: Wire Trust + Tracking into MessageDetail

**Files:**
- Modify: `src/api/messages.rs` (get_message endpoint)
- Modify: `src/api/threads.rs` (get_thread endpoint)

The `MessageDetail` struct doesn't need to change — trust/tracking data is computed on-the-fly from `raw_headers` and `body_html` and included in a wrapping response.

**Step 1: Update get_message endpoint**

In `src/api/messages.rs`, modify `get_message` to include trust indicators and tracking pixels in the response. Create a new `MessageDetailResponse` that wraps `MessageDetail` with trust/tracking fields:

```rust
use crate::api::trust;

#[derive(Serialize)]
pub struct MessageDetailResponse {
    #[serde(flatten)]
    pub message: MessageDetail,
    pub trust: trust::TrustIndicators,
    pub tracking_pixels: Vec<trust::TrackingPixel>,
}
```

In the `get_message` handler, after fetching the `MessageDetail`, compute the trust indicators:

```rust
let raw_headers: Option<String> = conn.query_row(
    "SELECT raw_headers FROM messages WHERE id = ?1",
    rusqlite::params![id],
    |row| row.get(0),
).ok().flatten();

let trust_indicators = raw_headers
    .as_deref()
    .map(trust::extract_trust_indicators)
    .unwrap_or_default();

let tracking = msg.body_html
    .as_deref()
    .map(trust::detect_tracking_pixels)
    .unwrap_or_default();
```

**Step 2: Update get_thread endpoint**

Similarly in `src/api/threads.rs`, include trust + tracking for each message in the thread response.

**Step 3: Write integration test**

Add test in `src/api/trust.rs`:
```rust
#[test]
fn test_end_to_end_trust_from_raw_headers() {
    let raw = "Authentication-Results: mx.google.com;\r\n dkim=pass header.d=example.com;\r\n spf=pass;\r\n dmarc=pass\r\nFrom: test@example.com\r\n";
    let indicators = extract_trust_indicators(raw);
    assert_eq!(indicators.spf, Some(AuthStatus::Pass));
    assert_eq!(indicators.dkim, Some(AuthStatus::Pass));
    assert_eq!(indicators.dmarc, Some(AuthStatus::Pass));
}
```

**Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass (existing + new).

**Step 5: Commit**

```bash
git add src/api/messages.rs src/api/threads.rs src/api/trust.rs
git commit -m "feat(v9): wire trust indicators and tracking pixels into message/thread responses"
```

---

## Task 4: API Key Management — Backend (N43, N25, S13)

**Files:**
- Create: `src/api/agent.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/main.rs`

**Step 1: Write tests**

In `src/api/agent.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};

    fn setup() -> (r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, Account) {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        // Run migration 003
        conn.execute_batch(include_str!("../../migrations/003_agent.sql")).unwrap();
        let account = Account::create(&conn, &CreateAccount {
            provider: "gmail".into(), email: "agent-test@example.com".into(),
            display_name: Some("Test".into()),
            imap_host: Some("imap.gmail.com".into()), imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".into()), smtp_port: Some(587),
            username: Some("agent-test@example.com".into()), password: None,
        });
        (conn, account)
    }

    #[test]
    fn test_create_api_key() {
        let (conn, _) = setup();
        let (key, stored) = create_api_key(&conn, "Test Key", "read_only", None).unwrap();
        assert!(key.starts_with("iris_"));
        assert_eq!(stored.name, "Test Key");
        assert_eq!(stored.permission, "read_only");
        assert!(!stored.is_revoked);
    }

    #[test]
    fn test_validate_api_key() {
        let (conn, _) = setup();
        let (key, _) = create_api_key(&conn, "Validate Test", "read_only", None).unwrap();
        let found = validate_api_key(&conn, &key);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Validate Test");
    }

    #[test]
    fn test_revoke_api_key() {
        let (conn, _) = setup();
        let (key, stored) = create_api_key(&conn, "Revoke Test", "read_only", None).unwrap();
        assert!(revoke_api_key(&conn, &stored.id));
        assert!(validate_api_key(&conn, &key).is_none()); // revoked key fails validation
    }

    #[test]
    fn test_list_api_keys() {
        let (conn, _) = setup();
        create_api_key(&conn, "Key A", "read_only", None).unwrap();
        create_api_key(&conn, "Key B", "draft_only", None).unwrap();
        let keys = list_api_keys(&conn);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_permission_hierarchy() {
        assert!(has_permission("autonomous", "read"));
        assert!(has_permission("autonomous", "draft"));
        assert!(has_permission("autonomous", "send"));
        assert!(has_permission("send_with_approval", "read"));
        assert!(has_permission("send_with_approval", "draft"));
        assert!(has_permission("send_with_approval", "send"));
        assert!(has_permission("draft_only", "read"));
        assert!(has_permission("draft_only", "draft"));
        assert!(!has_permission("draft_only", "send"));
        assert!(has_permission("read_only", "read"));
        assert!(!has_permission("read_only", "draft"));
        assert!(!has_permission("read_only", "send"));
    }

    #[test]
    fn test_log_audit_entry() {
        let (conn, _) = setup();
        let (_, stored) = create_api_key(&conn, "Audit Test", "read_only", None).unwrap();
        log_audit(&conn, &stored.id, "search", Some("message"), None, None, "success");
        let entries = get_audit_log(&conn, Some(&stored.id), 50, 0);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].action, "search");
    }
}
```

**Step 2: Implement agent.rs**

Core functions:
- `create_api_key(conn, name, permission, account_id) -> Result<(String, ApiKey), String>` — generate `iris_<random>` key, store SHA-256 hash + prefix
- `validate_api_key(conn, raw_key) -> Option<ApiKey>` — hash input, look up in DB, check not revoked, update last_used_at
- `revoke_api_key(conn, key_id) -> bool` — set is_revoked=1, revoked_at=now
- `list_api_keys(conn) -> Vec<ApiKeySummary>` — list all (no key_hash exposed)
- `has_permission(key_permission, required_action) -> bool` — hierarchy check
- `log_audit(conn, key_id, action, resource_type, resource_id, details, status)` — append to audit_log
- `get_audit_log(conn, key_id_filter, limit, offset) -> Vec<AuditEntry>` — read audit entries

Key generation: `iris_` prefix + 32 random hex chars. Store SHA-256 hash. Display only prefix.

Permission hierarchy:
- `read_only`: read, search
- `draft_only`: read, search, draft
- `send_with_approval`: read, search, draft, send
- `autonomous`: all

**Step 3: Add HTTP endpoints**

In `src/main.rs`, add routes:
```rust
.route("/api-keys", get(api::agent::list_keys_handler).post(api::agent::create_key_handler))
.route("/api-keys/{id}", delete(api::agent::revoke_key_handler))
.route("/audit-log", get(api::agent::get_audit_log_handler))
```

Handlers:
- `POST /api/api-keys` — body: `{ "name": "...", "permission": "read_only", "account_id": null }` → returns the raw key (shown once)
- `GET /api/api-keys` — returns list (no raw keys)
- `DELETE /api/api-keys/{id}` — revokes key
- `GET /api/audit-log` — query params: `?api_key_id=&limit=50&offset=0`

**Step 4: Register module and routes**

Add to `src/api/mod.rs`: `pub mod agent;`

**Step 5: Run tests**

Run: `cargo test api::agent`
Expected: All 6 agent tests pass.

**Step 6: Commit**

```bash
git add src/api/agent.rs src/api/mod.rs src/main.rs
git commit -m "feat(v9): API key management — create, validate, revoke, list, audit logging"
```

---

## Task 5: API Key Auth Middleware + Agent Endpoints (N61, N60)

**Files:**
- Modify: `src/api/agent.rs`
- Modify: `src/main.rs`

**Step 1: Write tests**

Add to `src/api/agent.rs` tests:

```rust
#[test]
fn test_extract_bearer_token() {
    assert_eq!(extract_bearer_token("Bearer iris_abc123"), Some("iris_abc123".to_string()));
    assert_eq!(extract_bearer_token("bearer iris_abc123"), Some("iris_abc123".to_string()));
    assert_eq!(extract_bearer_token("Basic abc123"), None);
    assert_eq!(extract_bearer_token(""), None);
}
```

**Step 2: Implement auth middleware**

Create an Axum middleware that:
1. Extracts `Authorization: Bearer iris_xxx` header
2. Validates the key against the DB
3. Checks permission for the requested action
4. Injects the validated `ApiKey` into request extensions
5. Logs to audit_log on completion

```rust
use axum::middleware::{self, Next};
use axum::extract::Request;
use axum::response::Response;

pub async fn agent_auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = extract_bearer_token(auth_header)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let api_key = validate_api_key(&conn, &token)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    request.extensions_mut().insert(api_key);
    Ok(next.run(request).await)
}
```

**Step 3: Add agent REST endpoints**

Create a nested router under `/api/agent/` that uses the auth middleware:

```rust
// Agent-facing endpoints (API key auth required)
let agent_routes = Router::new()
    .route("/search", get(api::agent::agent_search))
    .route("/messages/{id}", get(api::agent::agent_get_message))
    .route("/threads/{id}", get(api::agent::agent_get_thread))
    .route("/drafts", post(api::agent::agent_create_draft))
    .route("/send", post(api::agent::agent_send))
    .layer(middleware::from_fn_with_state(state.clone(), api::agent::agent_auth_middleware));
```

Each handler:
- Extracts `ApiKey` from extensions
- Checks permission (`has_permission(key.permission, "read")` etc.)
- Calls existing business logic (reuse `api::search::search`, `message::MessageDetail::get_by_id`, etc.)
- Logs to audit_log via `log_audit()`
- Returns 403 if insufficient permissions

**Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass.

**Step 5: Commit**

```bash
git add src/api/agent.rs src/main.rs
git commit -m "feat(v9): agent auth middleware and REST endpoints — search, read, draft, send"
```

---

## Task 6: Frontend — Trust Badges + Tracking Alerts (U17, U18)

**Files:**
- Modify: `web/src/pages/ThreadView.svelte`
- Create: `web/src/components/TrustBadge.svelte`

**Step 1: Create TrustBadge component**

```svelte
<script lang="ts">
  let { trust, trackingPixels = [] }: {
    trust: { spf?: string; dkim?: string; dmarc?: string };
    trackingPixels?: { url: string; domain: string }[];
  } = $props();

  const allPass = trust.spf === 'pass' && trust.dkim === 'pass' && trust.dmarc === 'pass';
  const anyFail = trust.spf === 'fail' || trust.dkim === 'fail' || trust.dmarc === 'fail';
  const hasTrust = trust.spf || trust.dkim || trust.dmarc;
</script>

{#if hasTrust}
  <div class="flex items-center gap-2 text-xs">
    <!-- Trust badge -->
    <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full
      {allPass ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400'
       : anyFail ? 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400'
       : 'bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400'}"
      title="SPF: {trust.spf || 'unknown'}, DKIM: {trust.dkim || 'unknown'}, DMARC: {trust.dmarc || 'unknown'}">
      {#if allPass}✓ Verified{:else if anyFail}✗ Unverified{:else}? Partial{/if}
    </span>

    <!-- Tracking pixel alert -->
    {#if trackingPixels.length > 0}
      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400"
        title="{trackingPixels.length} tracking pixel(s) detected: {trackingPixels.map(t => t.domain).join(', ')}">
        {trackingPixels.length} tracker{trackingPixels.length > 1 ? 's' : ''} blocked
      </span>
    {/if}
  </div>
{/if}
```

**Step 2: Wire into ThreadView**

In `ThreadView.svelte`, import `TrustBadge` and display it in the message header area (next to from address/date). Each message in the thread gets its own trust badge based on the message's `trust` and `tracking_pixels` fields from the API response.

**Step 3: Verify frontend builds**

Run: `cd web && npm run build`
Expected: Build succeeds with no errors.

**Step 4: Commit**

```bash
git add web/src/components/TrustBadge.svelte web/src/pages/ThreadView.svelte
git commit -m "feat(v9): trust badge and tracking pixel alert UI in ThreadView"
```

---

## Task 7: Frontend — API Key Management + Audit Log (U56)

**Files:**
- Modify: `web/src/pages/Settings.svelte`
- Modify: `web/src/lib/api.ts`

**Step 1: Add API client methods**

In `web/src/lib/api.ts`, add to the `api` object:

```typescript
apiKeys: {
    list: () => request<any[]>('/api/api-keys'),
    create: (data: { name: string; permission: string; account_id?: string }) =>
      request<{ key: string; id: string; name: string; permission: string }>('/api/api-keys', {
        method: 'POST', body: JSON.stringify(data)
      }),
    revoke: (id: string) => request<void>(`/api/api-keys/${id}`, { method: 'DELETE' }),
},
auditLog: {
    list: (params?: { api_key_id?: string; limit?: number; offset?: number }) => {
      const query = new URLSearchParams();
      if (params?.api_key_id) query.set('api_key_id', params.api_key_id);
      if (params?.limit) query.set('limit', String(params.limit));
      if (params?.offset) query.set('offset', String(params.offset));
      return request<{ entries: any[]; total: number }>(`/api/audit-log?${query}`);
    },
},
```

**Step 2: Add API Keys section to Settings**

In `Settings.svelte`, add a new section below existing settings:

```svelte
<!-- API Keys Section -->
<section>
  <h3>API Keys</h3>
  <p class="text-sm text-gray-500">Manage API keys for external agent access.</p>

  <!-- Create key form -->
  <form onsubmit={createKey}>
    <input bind:value={newKeyName} placeholder="Key name (e.g., Claude agent)" />
    <select bind:value={newKeyPermission}>
      <option value="read_only">Read Only</option>
      <option value="draft_only">Draft Only</option>
      <option value="send_with_approval">Send with Approval</option>
      <option value="autonomous">Autonomous</option>
    </select>
    <button type="submit">Create Key</button>
  </form>

  <!-- Show newly created key (once) -->
  {#if createdKey}
    <div class="bg-green-50 dark:bg-green-900/20 p-3 rounded-lg">
      <p class="text-sm font-medium">API Key created! Copy it now — it won't be shown again:</p>
      <code class="block mt-1 p-2 bg-gray-100 dark:bg-gray-800 rounded text-xs font-mono select-all">{createdKey}</code>
    </div>
  {/if}

  <!-- Key list -->
  <table>
    <thead><tr><th>Name</th><th>Permission</th><th>Last Used</th><th>Actions</th></tr></thead>
    <tbody>
      {#each apiKeys as key}
        <tr>
          <td>{key.name} <span class="text-gray-400">{key.key_prefix}...</span></td>
          <td>{key.permission}</td>
          <td>{key.last_used_at ? formatDate(key.last_used_at) : 'Never'}</td>
          <td><button onclick={() => revokeKey(key.id)}>Revoke</button></td>
        </tr>
      {/each}
    </tbody>
  </table>
</section>

<!-- Audit Log Section -->
<section>
  <h3>Audit Log</h3>
  <p class="text-sm text-gray-500">Recent agent activity.</p>
  <table>
    <thead><tr><th>Time</th><th>Agent</th><th>Action</th><th>Resource</th><th>Status</th></tr></thead>
    <tbody>
      {#each auditEntries as entry}
        <tr>
          <td>{formatDate(entry.created_at)}</td>
          <td>{entry.key_name}</td>
          <td>{entry.action}</td>
          <td>{entry.resource_type || ''} {entry.resource_id || ''}</td>
          <td>{entry.status}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</section>
```

**Step 3: Verify frontend builds**

Run: `cd web && npm run build`
Expected: Build succeeds.

**Step 4: Commit**

```bash
git add web/src/pages/Settings.svelte web/src/lib/api.ts
git commit -m "feat(v9): API key management UI and audit log in Settings page"
```

---

## Task 8: Integration Verification

**Step 1: Run all backend tests**

Run: `cargo test`
Expected: All tests pass (54 existing + new V9 tests).

**Step 2: Verify frontend build**

Run: `cd web && npm run build`
Expected: Clean build, no errors.

**Step 3: Update documentation**

Update `docs/SESSION-STATUS.md`:
- Add V9: Agent Connectivity section under Build Phase
- Move pipeline to "verify" stage
- Update test counts and commit counts

Update `~/.claude/projects/-Users-divyekant-Projects-iris/memory/MEMORY.md`:
- Update current state to reflect V9 complete
- Note all 9 slices done

**Step 4: Commit docs**

```bash
git add docs/SESSION-STATUS.md
git commit -m "docs: update session status — V9 complete, all 9 slices done"
```

---

## Summary

| Task | What | Affordances | Tests |
|------|------|-------------|-------|
| 1 | Migration 003 (api_keys + audit_log) | S13, S17 | Existing pass |
| 2 | Trust indicators + tracking pixel detection | U17, U18 | 7 new |
| 3 | Wire trust/tracking into message responses | U17, U18 | 1 new |
| 4 | API key management backend | N43, N25 | 6 new |
| 5 | Auth middleware + agent REST endpoints | N61, N60 | 1+ new |
| 6 | Trust badge + tracking alert UI | U17, U18 | — |
| 7 | API key management + audit log UI | U56, N63 | — |
| 8 | Integration verification + docs | — | All pass |

**Deferred from V9:**
- MCP protocol (N60 as MCP) — REST API provides equivalent agent access
- Webhooks (N62, S16) — can add post-V9
- Keyboard shortcuts (U55, N24, S3) — unrelated to agent connectivity
