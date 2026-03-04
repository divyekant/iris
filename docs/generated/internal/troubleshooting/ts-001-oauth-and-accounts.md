---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
troubleshooting: oauth-and-accounts
slug: ts-001-oauth-and-accounts
---

# Troubleshooting: OAuth and Accounts

## Overview

This guide covers common issues with OAuth2 authentication, account setup, and token management for Gmail, Outlook, and manual IMAP accounts.

## Diagnostic Checklist

Before diving into specific issues, verify:

1. **Environment variables set**: Check that `GMAIL_CLIENT_ID`, `GMAIL_CLIENT_SECRET` (or Outlook equivalents) are set.
2. **PUBLIC_URL correct**: The redirect URI is `{PUBLIC_URL}/auth/callback`. This must match exactly what is configured in the provider's developer console.
3. **Health endpoint reachable**: `GET /api/health` should return `{"status":"ok"}`.
4. **Server logs**: Start the server with `RUST_LOG=iris_server=debug` for detailed logging.

## Issue: OAuth redirect returns error

**Symptoms**: After clicking "Connect Gmail/Outlook," the provider's page shows an error, or the redirect URL contains `error=` parameters.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Redirect URI mismatch | Verify that `{PUBLIC_URL}/auth/callback` exactly matches the URI registered in the Google Cloud Console or Azure AD app registration. Include protocol, host, port, and path. |
| Invalid client ID | Double-check the `GMAIL_CLIENT_ID` or `OUTLOOK_CLIENT_ID` environment variable. Ensure there are no trailing spaces or newlines. |
| App not verified (Google) | Google shows a warning for unverified apps. Click "Advanced" and "Go to {app name}" to proceed. For production, submit the app for verification. |
| Consent screen missing scopes | In the Google Cloud Console, ensure the OAuth consent screen includes the required scopes: `https://mail.google.com/`, openid, email, profile. |

## Issue: Token exchange fails

**Symptoms**: After consent, the browser redirects to an error page. Server logs show "token exchange failed."

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Invalid client secret | Verify `GMAIL_CLIENT_SECRET` or `OUTLOOK_CLIENT_SECRET`. Regenerate the secret in the developer console if unsure. |
| Authorization code expired | The code is valid for a short time (usually minutes). If the user is slow to consent, the code may expire. Try again. |
| Redirect URI not matching (subtle) | The redirect URI during token exchange must match the one used during authorization. Ensure PUBLIC_URL has not changed between the two requests. |
| Network issue reaching token endpoint | The server needs outbound HTTPS access to `oauth2.googleapis.com` (Gmail) or `login.microsoftonline.com` (Outlook). Check firewalls, proxies, or WARP configuration. |

## Issue: No refresh token after Gmail OAuth

**Symptoms**: Account is created but sync fails shortly after the access token expires (~1 hour). Logs show "no refresh token stored."

**Resolution**: The code hardcodes `access_type=offline` and `prompt=consent` for Gmail. If these are not being sent (unexpected), check that the `start_oauth` function is being called correctly. Also verify in the Google Cloud Console that the OAuth client type is "Web application" (not "Desktop" or "TV").

Note: Google only issues a refresh token on the first authorization grant, or when `prompt=consent` forces re-consent. If the user previously authorized the app without consent prompting, they may need to revoke access at https://myaccount.google.com/permissions and re-authorize.

## Issue: Outlook "no email in Microsoft profile"

**Symptoms**: OAuth completes but account creation fails with "no email in Microsoft profile."

**Resolution**: Some Microsoft accounts (personal vs. work) return different fields. The code handles both `mail` and `userPrincipalName` via serde aliases. If neither is present, the account cannot be created. This can happen with accounts that do not have a primary email set. The user should check their Microsoft profile settings.

## Issue: Token refresh fails periodically

**Symptoms**: Sync stops working. Logs show "token exchange failed" during refresh attempts.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Refresh token revoked | User revoked app access in provider settings. Re-authenticate by reconnecting the account. |
| Client credentials changed | If you rotated the client secret, update the environment variable and restart. |
| Provider outage | Transient issue. The IDLE loop will retry with exponential backoff. |
| Network issue | Check connectivity to the provider's token endpoint. |

## Issue: Manual IMAP account cannot connect

**Symptoms**: Account is created but sync shows "error" status.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Wrong IMAP host or port | Verify the IMAP host (e.g., `imap.fastmail.com`) and port (usually 993 for TLS). |
| Wrong username | Some providers use the full email as username, others use a different format. |
| Wrong password | For Gmail, use an app-specific password (not the account password). For Fastmail, use an app password. |
| IMAP not enabled | Some providers require IMAP access to be enabled in account settings. |
| TLS certificate issue | The IMAP connection uses TLS. If behind a corporate proxy with certificate interception, TLS validation may fail. |
| Firewall blocking port 993 | Ensure outbound TCP connections to port 993 are allowed. |

## Issue: Duplicate account error

**Symptoms**: Attempting to add an account with an email that already exists returns an error.

**Resolution**: The `accounts.email` column has a UNIQUE constraint. Delete the existing account first via `DELETE /api/accounts/{id}` or through the UI, then re-add it.

## Server Log Patterns

| Log Message | Meaning |
|---|---|
| `OAuth account created` (INFO) | Account successfully created after OAuth callback |
| `token exchange failed: ...` (ERROR) | OAuth token exchange failed |
| `failed to fetch user info: ...` (ERROR) | Could not retrieve email/name from provider |
| `OAuth token refreshed` (DEBUG) | Token was successfully refreshed |
| `no refresh token stored` (ERROR) | Refresh attempted but no refresh token in database |
