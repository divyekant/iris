# Deployment Security

This guide is the minimum hardening baseline for self-hosting Iris safely.

## Recommended Baseline

- Keep Iris bound to localhost unless you intentionally place it behind a secure reverse proxy or VPN.
- Set `IRIS_AUTH_PASSWORD_HASH` so the web UI requires a real login before issuing a session cookie.
- Set `IRIS_SECRETS_KEY` so persisted account credentials and provider API keys are encrypted at rest.
- Use HTTPS for any deployment that is reachable from another machine.
- Restrict access with a reverse proxy, VPN, Tailscale, or IP allowlist if the app is not strictly local-only.
- Keep the database, `.env`, and backups out of the repo and out of world-readable paths.

## Required Secrets

### `IRIS_AUTH_PASSWORD_HASH`

- Required for any remote or shared-network deployment.
- Stores the password verifier for the web login gate.
- Without it, anyone who can reach the app may be able to bootstrap a browser session.

### `IRIS_SECRETS_KEY`

- Strongly recommended for every deployment, including local-only setups.
- Accepts either a 32-byte base64 key or a 64-character hex key.
- Encrypts persisted OAuth tokens, IMAP/app passwords, and configured provider API keys at rest.
- Existing plaintext secrets are re-encrypted automatically on startup after the key is configured.
- If encrypted secrets already exist and the key is missing or wrong, Iris should fail fast on startup.

## API Keys (Agent Access)

API keys grant agents access to all 200+ API routes with scoped permissions. Treat them as credentials.

- **Principle of least privilege.** Issue `read_only` keys unless the agent needs to write or send. `autonomous` keys can change server configuration.
- **Account scoping.** Keys can be restricted to a single email account. Use this when an agent only needs access to one mailbox.
- **Revoke unused keys.** Revoked keys are rejected immediately. Check `last_used_at` in Settings to identify stale keys.
- **Rate limits.** Each key gets its own rate limit bucket (500 burst / 5 req/sec). A misbehaving agent cannot starve others.
- **Audit trail.** All agent actions are logged with key ID, action, resource, and timestamp. Review via GET /api/audit-log.
- **Key rotation.** If a key is compromised, revoke it and create a new one. There is no key expiration — revocation is immediate.

## Safe Deployment Patterns

### Local-only

- Bind Iris to `127.0.0.1`.
- Access it only from the same machine.
- HTTP is acceptable only in this mode.

### Remote/self-hosted

- Put Iris behind HTTPS.
- Terminate TLS at a reverse proxy such as Caddy, Nginx, or Traefik.
- Do not expose port `3000` directly to the public internet.
- Add another access boundary when possible, such as VPN or Tailscale, reverse-proxy auth, or an IP allowlist.

## What To Rotate

Rotate these if they were ever stored plaintext, committed, copied into logs, or used before the recent hardening pass:

- Gmail OAuth credentials/tokens for connected accounts
- Outlook OAuth credentials/tokens for connected accounts
- IMAP or app passwords for non-OAuth mail providers
- OpenAI API keys
- Anthropic API keys
- Memories API keys
- Any old `SESSION_TOKEN_FILE` outputs or automation that depended on them

## Backups And Host Security

- Encrypt host and backup storage if possible.
- Treat the SQLite database as sensitive even with at-rest encryption enabled.
- Back up `IRIS_SECRETS_KEY` separately from the database.
- Do not put `IRIS_SECRETS_KEY` in the repo.
- Limit file permissions on `.env`, data directories, and backup artifacts.
- Keep container images and the host OS patched.

## Post-Deploy Validation

After deployment or restart, verify these behaviors:

- The app shows a login screen when `IRIS_AUTH_PASSWORD_HASH` is set.
- Connected accounts still sync and send mail after restart with the same `IRIS_SECRETS_KEY`.
- Persisted secrets in SQLite are stored as `enc:v1:...`, not plaintext.
- Starting with a missing or wrong `IRIS_SECRETS_KEY` fails once encrypted secrets exist.
- The site is only reachable through HTTPS or localhost, not a raw public port.

## Quick Checklist

- `IRIS_AUTH_PASSWORD_HASH` is set
- `IRIS_SECRETS_KEY` is set and backed up
- `IRIS_CORS_ORIGINS` is set (not using dev defaults)
- HTTPS is enabled for non-localhost access
- Iris is not exposed directly on a public port
- API keys use minimum required permission level
- Unused API keys are revoked
- Old secrets have been rotated
- Backups and `.env` files are access-controlled

## Operational Notes

- At-rest encryption protects stored secrets on disk, not secrets already leaked before encryption was enabled.
- HTTPS protects credentials and cookies in transit. It is still required even when secrets are encrypted at rest.
- Reverse-proxy auth or VPN access is still recommended for internet-reachable deployments.
