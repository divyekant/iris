---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
feature: email-accounts
slug: fh-001-email-accounts
---

# Feature Handoff: Email Accounts

## What It Does

The email accounts feature manages how users connect their email providers to Iris. It supports two connection methods: OAuth2 for Gmail and Outlook, and manual IMAP/SMTP configuration with app passwords for other providers. Each account stores credentials, IMAP/SMTP server details, OAuth tokens, and sync state.

## How It Works

### OAuth2 Flow (Gmail / Outlook)

1. The frontend calls `GET /api/auth/oauth/{provider}` which returns an authorization URL.
2. The server builds an OAuth2 client using the `oauth2` crate (v5, type-state generics via `ConfiguredClient` type alias). It encodes the provider name into the CSRF state parameter as `{provider}:{csrf_random}`.
3. The browser redirects the user to the provider's consent screen.
4. On callback at `GET /auth/callback?code=...&state=...`, the server exchanges the authorization code for tokens using the oauth2 crate's `exchange_code`.
5. The server fetches user info (email, display name) from the provider's userinfo endpoint.
6. An account record is created in the `accounts` table with IMAP/SMTP settings pre-filled for the provider.
7. OAuth tokens (access, refresh, expires_at) are stored in the account row.
8. An initial IMAP sync is spawned in a background Tokio task, followed by an IDLE listener.

Provider-specific details:
- **Gmail**: Forces `access_type=offline` and `prompt=consent` to guarantee a refresh token. Uses `imap.gmail.com:993` and `smtp.gmail.com:587`. Scopes include `https://mail.google.com/`.
- **Outlook**: Uses `outlook.office365.com:993` for IMAP and `smtp.office365.com:587` for SMTP. Requests `IMAP.AccessAsUser.All` and `SMTP.Send` scopes with `offline_access`.

### Manual IMAP/SMTP Accounts

Users provide IMAP host, port, SMTP host, port, username, and password. The account is created via `POST /api/accounts` with these fields. Password is stored in the `password_encrypted` column (currently plaintext -- flagged for hardening).

### Token Refresh

The `ensure_fresh_token` function in `src/auth/refresh.rs` checks whether the stored access token is still valid (with a 60-second buffer). If expired, it uses the refresh token to obtain a new access token from the provider and persists the updated tokens.

### Account CRUD

- `GET /api/accounts` -- list all accounts
- `POST /api/accounts` -- create (manual IMAP or triggered post-OAuth)
- `GET /api/accounts/{id}` -- get single account
- `DELETE /api/accounts/{id}` -- delete account (soft or hard depending on implementation)

## User-Facing Behavior

- On the setup page, the user clicks "Connect Gmail" or "Connect Outlook" to start OAuth.
- After consent, the browser redirects to `/setup/success?account_id={id}`.
- For manual accounts, the user fills in server details and tests the connection.
- The account switcher in the sidebar lists all active accounts.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `GMAIL_CLIENT_ID` | (none) | Google OAuth2 Client ID |
| `GMAIL_CLIENT_SECRET` | (none) | Google OAuth2 Client Secret |
| `OUTLOOK_CLIENT_ID` | (none) | Microsoft OAuth2 Client ID |
| `OUTLOOK_CLIENT_SECRET` | (none) | Microsoft OAuth2 Client Secret |
| `PUBLIC_URL` | `http://localhost:3000` | Used to construct the OAuth redirect URI (`{PUBLIC_URL}/auth/callback`) |

## Edge Cases and Limitations

- If the user denies consent, the callback receives an error parameter instead of a code. The current implementation does not handle this gracefully -- it will return a token exchange error.
- Gmail requires the `prompt=consent` parameter to issue a refresh token. Without it, only an access token is returned, and the refresh flow will fail.
- Outlook's userinfo may return `userPrincipalName` instead of `mail` for some accounts. The code handles both via serde aliasing.
- Duplicate accounts (same email) are rejected by the database UNIQUE constraint on `accounts.email`.
- Password-based accounts store credentials in plaintext in the `password_encrypted` column. Encryption is planned but not yet implemented.
- The OAuth redirect URI must exactly match what is configured in the Google/Microsoft developer console.

## Common Questions

**Q: What happens if the OAuth refresh token is revoked by the provider?**
A: The `ensure_fresh_token` call will fail with a `TokenExchange` error. IMAP sync will stop working for that account. The user needs to re-authenticate by reconnecting the account through the OAuth flow.

**Q: Can I connect multiple accounts from the same provider?**
A: No. The `accounts.email` column has a UNIQUE constraint. Each email address can only be connected once. Attempting to add a duplicate will fail at the database level.

**Q: How do I change the OAuth redirect URI for production?**
A: Set the `PUBLIC_URL` environment variable to the production URL (e.g., `https://iris.example.com`). The redirect URI is automatically constructed as `{PUBLIC_URL}/auth/callback`. Update the Google/Microsoft developer console to match.

**Q: What scopes does Iris request?**
A: Gmail: `https://mail.google.com/`, `openid`, `email`, `profile`. Outlook: `IMAP.AccessAsUser.All`, `SMTP.Send`, `offline_access`, `openid`, `email`, `profile`.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| OAuth redirects to error page | Missing `GMAIL_CLIENT_ID` or `OUTLOOK_CLIENT_ID` | Set the environment variables and restart |
| "token exchange failed" error | Redirect URI mismatch, invalid client secret, or expired auth code | Verify `PUBLIC_URL` matches the redirect URI in the developer console |
| No refresh token after Gmail auth | Missing `access_type=offline` or `prompt=consent` | These are hardcoded; check that the OAuth flow is using `start_oauth` |
| "no email in Microsoft profile" | Outlook account type lacks `mail` field | Check that the account has a primary email address set |
| Account created but no sync starts | IMAP host/port not set in the account record | Ensure the OAuth callback sets IMAP/SMTP config for the provider |

## Related Links

- Source: `src/auth/oauth.rs`, `src/auth/refresh.rs`, `src/api/accounts.rs`
- Database: `migrations/001_initial.sql` (accounts table)
- Config: `src/config.rs`
