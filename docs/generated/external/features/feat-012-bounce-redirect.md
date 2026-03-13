---
id: feat-012
type: feature-doc
audience: external
topic: bounce-redirect
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.1
---

# Bounce / Redirect

## Overview

Redirect lets you forward an email to a new recipient while preserving the original sender information. Unlike a standard forward, a redirect sends the message so that it appears to the recipient as if it came directly from the original sender — not from you.

This is useful when you receive an email that was sent to the wrong person and you want to pass it along without rewriting it or losing attribution.

---

## How to Use It

1. Identify the message ID you want to redirect. You can find this in any thread or message detail response.
2. Send a POST request to `/api/messages/{message_id}/redirect` with the destination email address.
3. On success, the response confirms the redirect was sent and echoes the recipient address.

---

## Configuration

No additional configuration is required. Redirect uses the SMTP account linked to the message being redirected. Ensure at least one email account with send permissions is connected in Settings.

---

## Examples

### Redirect a message to a colleague

```bash
curl -X POST http://localhost:3000/api/messages/msg_abc123/redirect \
  -H "Content-Type: application/json" \
  -H "x-session-token: YOUR_SESSION_TOKEN" \
  -d '{"to": "colleague@example.com"}'
```

**Response:**

```json
{
  "redirected": true,
  "to": "colleague@example.com"
}
```

---

### Redirect with a distribution list address

```bash
curl -X POST http://localhost:3000/api/messages/msg_xyz789/redirect \
  -H "Content-Type: application/json" \
  -H "x-session-token: YOUR_SESSION_TOKEN" \
  -d '{"to": "support-team@example.com"}'
```

**Response:**

```json
{
  "redirected": true,
  "to": "support-team@example.com"
}
```

---

### Invalid email address (error case)

```bash
curl -X POST http://localhost:3000/api/messages/msg_abc123/redirect \
  -H "Content-Type: application/json" \
  -H "x-session-token: YOUR_SESSION_TOKEN" \
  -d '{"to": "not-an-email"}'
```

**Response (400 Bad Request):**

```json
{
  "error": "Invalid email address"
}
```

---

## Limitations

- Redirect sends to exactly one recipient per request. To redirect to multiple addresses, make multiple requests.
- The `to` field must be a valid RFC 5321 email address. Malformed addresses return a 400 error.
- Redirect does not add a note or label to the original message — the thread appears unchanged in your inbox.
- Delivery success depends on your outbound SMTP configuration and the recipient's mail server. Iris confirms the message was submitted but cannot guarantee delivery.

---

## Related

- [Compose & Send](../concepts/compose-send.md)
- [Account Setup](../concepts/account-setup.md)
