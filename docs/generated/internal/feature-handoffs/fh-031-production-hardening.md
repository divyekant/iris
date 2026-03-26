---
id: fh-031
type: feature-handoff
audience: internal
topic: production-hardening
status: current
generated: 2026-03-26
source-tier: direct
hermes-version: 1.0.1
---

# Feature Handoff: Production Hardening

## What It Does

This handoff covers the infrastructure and security measures that prepare Iris for production deployment: security headers on all responses, non-root Docker container execution, a CI/CD pipeline with format/lint/test/audit/build stages, request body size limits, health check timeouts, auth endpoint rate limiting, AI provider hot-reload, and CORS configuration with a development warning.

These are not user-visible features but are critical for safe, reliable operation.

## How It Works

### Security Headers (`src/api/security_headers.rs`)

A middleware applied to every response adds four security headers:

| Header | Value | Purpose |
|---|---|---|
| `x-content-type-options` | `nosniff` | Prevents MIME-type sniffing |
| `x-frame-options` | `DENY` | Prevents embedding in iframes (clickjacking protection) |
| `referrer-policy` | `strict-origin-when-cross-origin` | Limits referrer information leakage |
| `permissions-policy` | `camera=(), microphone=(), geolocation=()` | Disables browser APIs Iris does not need |

The middleware runs as an Axum layer on the top-level router, so it applies to all routes including static file serving, API endpoints, and WebSocket upgrades.

### Non-Root Docker (`Dockerfile`)

The Dockerfile uses a multi-stage build:

1. **Builder stage** (`rust:1.93-slim`): Compiles the Rust binary with `cargo build --release`.
2. **Frontend stage** (`node:20-slim`): Builds the web UI with `npm ci && npm run build`.
3. **Runtime stage** (`debian:bookworm-slim`):
   - Installs only `ca-certificates` and `curl` (for health checks).
   - Creates a non-root user `iris` (UID 1001).
   - Copies the binary, frontend dist, and migrations.
   - Sets ownership of `/app` to the `iris` user.
   - Runs as `USER iris` (not root).
   - Exposes port 3000.
   - Includes a Docker HEALTHCHECK: `curl -sf http://127.0.0.1:3000/api/health` every 30s with 5s timeout, 10s start period, 3 retries.

### CI/CD Pipeline (`.github/workflows/ci.yml`)

The pipeline runs on pushes to `main` and on all pull requests. Four parallel jobs:

| Job | Steps | Purpose |
|---|---|---|
| **Backend** | `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` | Code formatting, lint (warnings are errors), unit/integration tests |
| **Frontend** | `npm ci`, `npm run check`, `npm run build` | Dependency install, type checking, production build |
| **Docker Build** | `docker build .` | Verify the Dockerfile builds successfully |
| **Security Audit** | `cargo audit`, `npm audit --audit-level=moderate` | Check for known vulnerabilities in Rust and Node dependencies |

Caching: Cargo registry/target and npm modules are cached via `actions/cache@v4` keyed by lock file hashes.

### Request Body Limit (`src/lib.rs`)

```rust
.layer(DefaultBodyLimit::max(25 * 1024 * 1024)) // 25 MB
```

Applied to the top-level router. Limits all request bodies to 25 MB. This covers email composition with attachments while preventing memory exhaustion from oversized payloads.

### Health Check (`src/api/health.rs`)

The health endpoint (`GET /api/health`) checks three subsystems:

| Check | Method | Timeout |
|---|---|---|
| Database | `SELECT 1` via `spawn_blocking` | 5 seconds (`tokio::time::timeout`) |
| AI providers | `providers.any_healthy()` | Per-provider timeout (varies) |
| Memories | `memories.health()` | 5 seconds (built into client) |

Response:
```json
{
  "status": "ok",       // or "degraded" if DB is down
  "version": "0.3.0",   // hardcoded
  "db": true,
  "ai": true,
  "memories": true
}
```

The version is hardcoded (not from `env!("CARGO_PKG_VERSION")`) because the `env!` macro does not update reliably in Docker layer-cached builds.

The `status` field is `"ok"` if the database is healthy, `"degraded"` otherwise. AI and Memories being down does not degrade the status since they are optional services.

### Auth Rate Limiting (`src/api/rate_limit.rs`)

Two separate rate limiters using `tower-governor` (GCRA token-bucket algorithm):

| Limiter | Burst | Sustained Rate | Applied To |
|---|---|---|---|
| General | 500 requests | ~5/sec (12s replenish period) | All protected API routes |
| Auth | 10 requests | 1/sec | `/auth/bootstrap`, `/auth/login`, `/auth/logout`, `/auth/oauth/*` |

Both use the `SessionTokenKeyExtractor` which:
1. Checks for `Authorization: Bearer` -- uses `agent:{prefix}` as the bucket key.
2. Falls back to session token from header or cookie.
3. Falls back to `__anonymous__` for unauthenticated requests.

Agent keys get their own rate limit buckets, separate from the UI session. Anonymous requests share a single bucket.

When exceeded, the response is HTTP 429 with `retry-after` and `x-ratelimit-after` headers. Successful responses include `x-ratelimit-limit` and `x-ratelimit-remaining`.

### Provider Hot-Reload (`src/ai/provider.rs`)

The `ProviderPool` holds providers behind an `Arc<RwLock<Vec<LlmProvider>>>`. The `reload()` method replaces the entire provider list at runtime:

```rust
pub fn reload(&self, providers: Vec<LlmProvider>) {
    let mut lock = self.providers.write().expect("provider pool lock poisoned");
    *lock = providers;
}
```

This is called when AI configuration changes in the Settings UI. The pool uses round-robin with fallback: if provider N fails, it tries N+1, wrapping around. A `snapshot()` method clones the provider list and releases the lock immediately so reads do not block writes.

Supported providers: Ollama, Anthropic, OpenAI. Each implements `health()`, `generate()`, and `generate_with_tools()`.

### CORS Configuration (`src/lib.rs`)

CORS is configured via the `IRIS_CORS_ORIGINS` environment variable (comma-separated origins). If unset, the system falls back to development defaults (`http://localhost:1420,http://localhost:5173`) and logs a warning:

```
IRIS_CORS_ORIGINS not set -- using dev defaults (localhost:1420, localhost:5173). Set this in production.
```

Allowed methods: GET, POST, PUT, PATCH, DELETE, OPTIONS.
Allowed headers: Accept, Authorization, Content-Type, Origin, sec-fetch-site, x-session-token.
Credentials: allowed.

## User-Facing Behavior

- Users see no visible change from security headers -- they work silently.
- The health endpoint is public (no auth required), useful for monitoring dashboards.
- Rate limit headers on responses help agents implement proper backoff.
- Provider configuration changes take effect immediately without restarting the server.
- The CORS warning in server logs alerts operators to set origins before production deployment.

## Configuration

| Setting | Source | Default | Description |
|---|---|---|---|
| `IRIS_CORS_ORIGINS` | Environment variable | `http://localhost:1420,http://localhost:5173` | Comma-separated allowed CORS origins |
| Body limit | Hardcoded | 25 MB | Maximum request body size |
| Health check timeout | Hardcoded | 5 seconds | Database connectivity check timeout |
| General rate limit | Hardcoded | 500 burst / 5 per sec | Token-bucket rate limiter for API routes |
| Auth rate limit | Hardcoded | 10 burst / 1 per sec | Token-bucket rate limiter for auth endpoints |
| Docker user | Dockerfile | `iris` (UID 1001) | Non-root runtime user |
| Docker health interval | Dockerfile | 30s (5s timeout, 10s start) | Container health check frequency |
| Version | Hardcoded | `0.3.0` | Reported in health endpoint |

## Edge Cases & Limitations

- **Version is hardcoded.** The `env!("CARGO_PKG_VERSION")` macro was not updating in Docker builds due to layer caching. The version must be updated manually in `src/api/health.rs` on each release.
- **Rate limit state is in-memory.** Server restarts reset all counters. Clustered deployments would need shared state (not currently supported).
- **CORS fallback to dev origins is intentional.** This makes local development work out of the box but is a misconfiguration in production. The warning log is the only alert.
- **25 MB body limit is global.** There is no per-route override. Endpoints that should accept less (e.g., config updates) do not have tighter limits.
- **Security headers do not include CSP.** Content-Security-Policy is not set because the SPA needs to load inline styles and scripts from the build output.
- **Provider hot-reload replaces all providers.** There is no add/remove for individual providers. The entire list is swapped atomically.
- **Health check only flags DB as degraded.** AI and Memories being down is considered acceptable -- the client is still functional for basic email operations.

## Common Questions

**Q: Why is the version hardcoded instead of using Cargo metadata?**
A: The `env!("CARGO_PKG_VERSION")` macro captures the version at compile time. With Docker layer caching, if `Cargo.toml` is unchanged but the code changes, the cached layer retains the old compiled binary's metadata. Hardcoding ensures the version is always correct.

**Q: How do I add a new AI provider without restarting?**
A: Configure the provider in Settings > AI. The UI calls the config endpoint, which triggers `ProviderPool::reload()` with the new provider list. The change takes effect on the next AI request.

**Q: What happens when both rate limiters apply?**
A: Auth endpoints have their own limiter (10 burst). The general limiter (500 burst) also applies to auth endpoints since they are nested under the rate-limited router. In practice, the auth limiter is the binding constraint on auth endpoints.

**Q: Is the Docker container rootless?**
A: Yes. The runtime stage creates user `iris` (UID 1001) and all processes run under that user. The `/app` directory is owned by `iris`. No `sudo` or privilege escalation is available.

**Q: What does the CI pipeline block on?**
A: Any of these failures block the PR: formatting violations (`cargo fmt --check`), Clippy warnings (treated as errors with `-D warnings`), test failures, frontend type errors, Docker build failures, or known vulnerabilities in dependencies.

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| Health endpoint returns `"degraded"` | Database unreachable or `SELECT 1` takes >5s | Check disk space, SQLite file permissions, and I/O load. |
| Health reports wrong version | Version not updated in `health.rs` | Update the hardcoded `"0.3.0"` string in `src/api/health.rs`. |
| CORS errors in browser console | `IRIS_CORS_ORIGINS` not set or missing the frontend origin | Set `IRIS_CORS_ORIGINS=https://your-domain.com` in the environment. |
| 429 on normal usage | Rate limit too aggressive for the use case | Current limits (500 burst) are generous. Check for runaway clients or polling loops. |
| CI fails on `cargo audit` | Known vulnerability in a dependency | Update the affected crate or add an advisory ignore if the vulnerability is not applicable. |
| Docker container unhealthy | Health check failing | Check container logs for startup errors. Verify port 3000 is exposed. |

## Related

- [fh-012-auth-security.md](fh-012-auth-security.md) -- Authentication and security foundations
- [fh-027-agent-platform.md](fh-027-agent-platform.md) -- Agent auth and rate limiting
- [fh-014-ai-integration.md](fh-014-ai-integration.md) -- AI provider setup
- [fh-013-job-queue.md](fh-013-job-queue.md) -- Background job processing
