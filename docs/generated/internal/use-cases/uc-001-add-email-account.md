---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
use-case: add-email-account
slug: uc-001-add-email-account
---

# Use Case: Add Email Account

## Summary

A user connects their Gmail or Outlook account via OAuth, or adds a generic IMAP account with app password credentials. After successful connection, the system performs an initial email sync.

## Actors

- **User**: The person adding their email account to Iris.
- **System**: The Iris backend server.
- **Provider**: Gmail, Outlook, or a generic IMAP server.

## Preconditions

- Iris server is running.
- For OAuth: `GMAIL_CLIENT_ID`/`SECRET` or `OUTLOOK_CLIENT_ID`/`SECRET` environment variables are set. Google/Microsoft developer console is configured with the correct redirect URI (`{PUBLIC_URL}/auth/callback`).
- For IMAP: The user has IMAP host, port, username, and app password ready.

## Flow: OAuth (Gmail/Outlook)

1. User navigates to the setup page in the Iris SPA.
2. User clicks "Connect Gmail" or "Connect Outlook."
3. Frontend calls `GET /api/auth/oauth/{provider}`.
4. Backend builds an OAuth2 authorization URL with scopes and CSRF state, returns it to the frontend.
5. Frontend redirects the browser to the provider's consent screen.
6. User reviews permissions and clicks "Allow."
7. Provider redirects browser to `{PUBLIC_URL}/auth/callback?code={code}&state={state}`.
8. Backend parses the provider from the state parameter, exchanges the authorization code for access and refresh tokens.
9. Backend fetches user info (email, display name) from the provider's userinfo endpoint.
10. Backend creates an account record in the database with pre-filled IMAP/SMTP settings.
11. Backend stores OAuth tokens (access, refresh, expires_at).
12. Backend spawns a background initial sync task.
13. Browser redirects to `/setup/success?account_id={id}`.
14. User sees a success page. The inbox begins populating as sync progresses.

## Flow: Manual IMAP

1. User navigates to the setup page and selects "Manual IMAP."
2. User enters IMAP host, IMAP port, SMTP host, SMTP port, username, and password.
3. Frontend calls `POST /api/accounts` with the configuration.
4. Backend creates the account record with the provided settings.
5. Backend spawns a background sync using password-based IMAP login.
6. User is redirected to the inbox. Messages appear as sync progresses.

## Postconditions

- An account record exists in the `accounts` table.
- For OAuth accounts, access and refresh tokens are stored.
- Initial sync has been triggered (newest 100 INBOX messages).
- An IDLE listener is running for real-time push notifications.
- The account appears in the account switcher.

## Error Scenarios

| Scenario | System Response |
|---|---|
| User denies OAuth consent | Callback receives error; server returns token exchange failure; user remains on provider page |
| Invalid client credentials | Token exchange fails with 502; user sees error message |
| IMAP connection fails | Sync status set to "error" with error message; user sees error in UI |
| Duplicate email address | Database rejects with UNIQUE constraint violation; 409 or 500 returned |
| Provider userinfo missing email | OAuthError::UserInfo returned; account not created |

## Related Features

- fh-001-email-accounts
- fh-002-email-sync
