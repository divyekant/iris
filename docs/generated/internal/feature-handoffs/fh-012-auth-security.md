---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
feature: auth-security
slug: fh-012-auth-security
---

# Feature Handoff: Auth and Security

## What It Does

Authentication and security covers the session-based auth system for the web UI, the same-origin bootstrap mechanism, WebSocket authentication, trust indicators for email verification (SPF/DKIM/DMARC), and tracking pixel detection.

## How It Works

### Session Token Generation

On every server startup, a random 64-character hex session token is generated (`src/main.rs`):

```
(0..32).map(|_| format!("{:02x}", rand::random::<u8>())).collect()
```

This token lives only in memory (and optionally in a file). It is not persisted to the database. A server restart generates a new token, invalidating all previous sessions.

Optionally, the token can be written to a file specified by the `SESSION_TOKEN_FILE` environment variable, useful for Docker/script integration.

### Bootstrap Endpoint

`GET /api/auth/bootstrap` (`src/api/session_auth.rs`):

- Returns the session token as `{ "token": "..." }`.
- Protected by `Sec-Fetch-Site` header validation: only allows `same-origin`, `same-site`, or `none`. This ensures only the browser's own Fetch API (from the same origin) can retrieve the token, preventing cross-origin token theft.
- Returns 403 Forbidden for cross-origin requests.

### Session Auth Middleware

`session_auth_middleware` (`src/api/session_auth.rs`):

- Checks the `X-Session-Token` header on every protected API request.
- Compares the header value against the in-memory session token.
- Returns 401 Unauthorized if the token is missing or does not match.

All `/api/*` routes except `/api/health`, `/api/auth/bootstrap`, and `/api/auth/oauth/{provider}` are protected.

### WebSocket Authentication

WebSocket connections at `/ws` pass the session token as a query parameter. The WebSocket handler validates the token before upgrading the connection.

### Trust Indicators (`src/api/trust.rs`)

When a message is read via `GET /api/messages/{id}`, the system parses the `Authentication-Results` header from raw email headers:

1. Locates the `Authentication-Results:` header line (handles folded/continuation lines).
2. Parses semicolon-separated results for `spf=`, `dkim=`, and `dmarc=` values.
3. Maps each to an `AuthStatus` enum: Pass, Fail, Softfail, None, Neutral.

Returns `TrustIndicators { spf, dkim, dmarc }` alongside the message detail.

### Tracking Pixel Detection (`src/api/trust.rs`)

`detect_tracking_pixels` scans the HTML body for `<img>` tags that are likely tracking pixels:

1. Scans for `<img` tags in the HTML (case-insensitive).
2. For each tag, extracts the `src` attribute.
3. Checks two conditions:
   - **Tiny image**: width <= 1 AND height <= 1 (classic 1x1 pixel tracker).
   - **Known tracker domain**: URL domain matches a list of 16+ known email tracking services (Mailchimp, SendGrid, HubSpot, etc.), including subdomains.
4. Returns a list of `TrackingPixel { url, domain }` for detected trackers.

### Agent API Key Auth

See fh-010-agent-api for the agent authentication system (separate from session auth, uses Bearer tokens with SHA-256 hashed API keys).

## User-Facing Behavior

- The user never manually enters a session token. The SPA automatically calls `/api/auth/bootstrap` on load to retrieve it.
- Trust indicators (SPF/DKIM/DMARC) appear as colored badges in the message header area (green for pass, red for fail, gray for missing).
- Tracking pixels are listed as warnings below the email content.
- If the server restarts, the user must refresh the page (the bootstrap call gets a new token).

## Configuration

| Variable | Default | Description |
|---|---|---|
| `SESSION_TOKEN_FILE` | (none) | Optional file path to write the session token for external tool integration |
| `PUBLIC_URL` | `http://localhost:3000` | Affects CORS allowed origins |

CORS is configured to allow origins: `http://localhost:3000`, `http://localhost:5173`, `http://127.0.0.1:3000`, `http://127.0.0.1:5173`.

## Edge Cases and Limitations

- The session token is randomly generated per server process. Restarting the server invalidates all sessions with no way to recover (by design for a single-user, local-first application).
- The `Sec-Fetch-Site` guard depends on browser behavior. Older browsers or non-browser HTTP clients may not send this header. In that case, `fetch_site` is None and the request is rejected.
- The session token is transmitted via HTTP header on every request. Without HTTPS, it can be intercepted on the network. For local-only deployments (127.0.0.1), this is acceptable.
- Trust indicators depend on the email provider including `Authentication-Results` headers. Some providers strip or do not include these headers. In that case, all indicators will be None.
- The known tracker domain list is static and hardcoded. New tracking services will not be detected until the list is updated.
- Tracking pixel detection uses simple HTML tag parsing, not a full DOM parser. Malformed HTML may cause false negatives.

## Common Questions

**Q: Why does Iris use a per-startup random token instead of user/password auth?**
A: Iris is designed as a single-user, local-first application. A randomly generated session token avoids the complexity of user management, password hashing, and password reset flows. The same-origin bootstrap ensures only the local browser can obtain the token.

**Q: What happens if I access Iris from a different machine on the network?**
A: The `Sec-Fetch-Site` guard will block the bootstrap request if it comes from a different origin. The CORS policy also restricts cross-origin access. For remote access, you would need to configure `PUBLIC_URL` and update CORS origins, but this weakens the security model.

**Q: Can an attacker steal the session token?**
A: The bootstrap endpoint only responds to same-origin requests (enforced by Sec-Fetch-Site). The token cannot be fetched via cross-origin requests, iframes, or script injection from other sites. However, if the attacker has local network access and Iris is not behind HTTPS, they could potentially intercept the token from HTTP traffic.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| 401 on all API requests | Session token mismatch (server restarted) | Refresh the page to re-bootstrap |
| 403 on bootstrap | Cross-origin request or missing Sec-Fetch-Site header | Access Iris from the same origin as PUBLIC_URL |
| Trust badges all gray | Authentication-Results header missing from raw_headers | Provider does not include auth results; no fix needed |
| No tracking pixels detected | HTML does not contain known tracker patterns | Detection is best-effort; unknown trackers are not caught |
| CORS error in browser console | Accessing from an origin not in the allow list | Use one of the configured CORS origins or update the code |

## Related Links

- Source: `src/api/session_auth.rs`, `src/api/trust.rs`, `src/main.rs` (token generation)
- CORS: `src/lib.rs` (build_app CorsLayer)
- Frontend: TrustBadge component, tracking pixel display
