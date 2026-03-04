---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
feature: compose-send
slug: fh-004-compose-send
---

# Feature Handoff: Compose and Send

## What It Does

The compose and send feature allows users to write new emails, reply to threads, forward messages, and manage drafts. It uses the `lettre` crate for building RFC 2822 messages and sending them via SMTP with XOAUTH2 authentication.

## How It Works

### Email Building (`src/smtp.rs`)

The `build_email` function constructs an RFC 2822 email using `lettre::Message::builder()`:

1. Sets the `From` address with optional display name.
2. Adds `To`, `Cc`, and `Bcc` recipients (each parsed as a `Mailbox`).
3. Sets the `Subject`.
4. For replies, sets `In-Reply-To` and `References` headers.
5. If HTML body is provided, creates a `multipart/alternative` message with both text/plain and text/html parts. Otherwise, creates a plain text message.
6. `lettre` automatically generates a `Message-ID` header.

### SMTP Sending (`src/smtp.rs`)

The `send_email` function handles SMTP transport:

- **OAuth2 providers (Gmail, Outlook)**: Uses `AsyncSmtpTransport::starttls_relay` with XOAUTH2 authentication mechanism. The access token is passed as the password in `Credentials`.
- **Password-based accounts**: Uses standard STARTTLS with username/password credentials.
- SMTP port defaults to 587 (STARTTLS).

Before sending, `ensure_fresh_token` is called to refresh expired OAuth tokens.

### Draft Management

Drafts are stored in a `drafts` table (created in migration 001). API endpoints:

- `POST /api/drafts` -- save or update a draft (upsert by id)
- `GET /api/drafts` -- list all drafts for the user
- `DELETE /api/drafts/{id}` -- delete a draft

Drafts store: account_id, to/cc/bcc (JSON), subject, body_text, body_html.

### Send API

`POST /api/send` accepts a `ComposeRequest` body:

1. Looks up the account by `account_id`.
2. Refreshes the OAuth token if needed.
3. Builds the email via `build_email`.
4. Sends via `send_email`.
5. Stores a copy in the local `messages` table with folder "Sent".
6. Returns the generated message ID.

## User-Facing Behavior

- The ComposeModal component supports new message, reply, reply-all, and forward modes.
- Reply pre-fills the `To`, `Subject` (with "Re:" prefix), `In-Reply-To`, and `References` headers.
- Reply-all includes all original recipients in `To` and `Cc`.
- Forward pre-fills the `Subject` with "Fwd:" prefix and includes the original message body.
- Auto-save triggers periodically while composing, saving a draft to the server.
- On successful send, the compose modal closes and the draft (if any) is deleted.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `PUBLIC_URL` | `http://localhost:3000` | Used for OAuth redirect during token refresh |
| `GMAIL_CLIENT_ID` / `SECRET` | (none) | Required for Gmail XOAUTH2 SMTP |
| `OUTLOOK_CLIENT_ID` / `SECRET` | (none) | Required for Outlook XOAUTH2 SMTP |

SMTP host and port are stored per-account: Gmail uses `smtp.gmail.com:587`, Outlook uses `smtp.office365.com:587`.

## Edge Cases and Limitations

- Attachment sending is not implemented. The `build_email` function creates text-only or multipart/alternative messages.
- If the OAuth token refresh fails during send, the error is returned to the user and the email is not sent.
- Sent messages are stored locally but not uploaded to the provider's Sent folder via IMAP APPEND.
- The `In-Reply-To` and `References` headers are set by the frontend based on the thread being replied to. If these are missing, the reply will not be threaded correctly on the recipient's end.
- BCC recipients are stored in the local sent message record. This is a privacy consideration for local-first usage (single user).

## Common Questions

**Q: Why does Gmail reject my send with "Username and Password not accepted"?**
A: This typically means the XOAUTH2 token is invalid or expired. Check that the OAuth token refresh succeeded. Also verify the Gmail OAuth client has the `https://mail.google.com/` scope.

**Q: Are sent messages synced back from the provider?**
A: No. Sent messages are stored locally in the `messages` table with folder "Sent" but are not uploaded to the IMAP Sent folder. The local copy and the provider copy are independent.

**Q: How does auto-save work?**
A: The ComposeModal frontend component periodically calls `POST /api/drafts` with the current compose state. The draft is upserted by id, so subsequent saves update the same draft row.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| "SMTP not configured for this account" | Account missing smtp_host | Check account record for SMTP configuration |
| "no access token available" | OAuth token missing or refresh failed | Re-authenticate the account via OAuth flow |
| "SMTP send failed" | Network issue, auth failure, or provider rejection | Check SMTP host/port; verify credentials; check provider-specific SMTP limits |
| "invalid to address" | Malformed recipient email | Verify recipient email format |
| Reply not threading on recipient's side | Missing In-Reply-To or References headers | Check that the frontend passes these headers from the original thread |

## Related Links

- Source: `src/smtp.rs`, `src/api/compose.rs`
- Auth: `src/auth/refresh.rs`
- Frontend: `web/src/components/ComposeModal.svelte`
