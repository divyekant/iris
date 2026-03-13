---
id: fh-015
type: feature-handoff
audience: internal
topic: Bounce / Redirect
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.1
---

# FH-015: Bounce / Redirect

## What It Does

Bounce/Redirect forwards an email message to a new recipient while preserving the original sender identity in the headers. Unlike a standard forward (which wraps the original as a new message from the user), a redirect transmits the original message with supplemental `Resent-*` headers per RFC 2822 §3.6.6. The recipient receives the message appearing to originate from the original sender, with the redirected routing visible in mail headers.

Primary use cases: forwarding a support request to the correct team while preserving sender attribution, routing a misaddressed message to its intended recipient.

## How It Works

**Endpoint**: `POST /api/messages/{id}/redirect`

**Request body**:
```json
{"to": "recipient@example.com"}
```

**Processing sequence**:
1. Route handler (`redirect_message` in `src/api/compose.rs`) validates session and parses the request body.
2. Looks up the message by `id` in the database; returns 404 if not found.
3. Confirms the account associated with the message is active; returns 400 if inactive.
4. Validates the `to` address using lettre's address parser; returns 400 for any address that fails parsing.
5. Calls `build_redirect_email` in `src/smtp.rs`, which constructs a new SMTP message containing the original message body and the following additional headers:
   - `Resent-From`: the authenticated user's email address
   - `Resent-To`: the target recipient address
   - `Resent-Date`: current UTC timestamp in RFC 2822 format
6. Sends via the account's configured SMTP connection (XOAUTH2 or password auth, same as compose).
7. Returns `{"redirected": true, "to": "recipient@example.com"}` on success.

**Implementation files**:
- `src/api/compose.rs` — `redirect_message` route handler
- `src/smtp.rs` — `build_redirect_email` function

## User-Facing Behavior

- In ThreadView, a **Redirect** option appears in the message action menu (alongside Reply, Forward, etc.).
- Clicking **Redirect** opens `RedirectDialog.svelte` — a modal with a single email address input and a **Send** button.
- The input accepts a single email address. The **Send** button is disabled until the field is non-empty.
- On success, the dialog closes and a brief confirmation toast appears. No copy of the redirected message is saved to Drafts or Sent.
- On failure (invalid address, inactive account), an inline error message appears in the dialog.

## Configuration

No additional configuration beyond standard SMTP setup (configured per account in Settings). The redirect uses whichever SMTP connection is active for the account that owns the source message. XOAUTH2 token refresh is handled automatically if the access token has expired.

## Error Responses

| HTTP Status | Condition |
|---|---|
| 400 | `to` address fails lettre address validation (malformed syntax) |
| 400 | Account associated with the message is marked inactive |
| 404 | Message `id` does not exist in the database |
| 500 | SMTP send failure (network error, auth rejection, etc.) |

## Edge Cases & Limitations

- **Single recipient only**: The current implementation accepts exactly one `to` address. Multiple recipients in a single redirect call are not supported.
- **No BCC/CC on redirect**: The `Resent-*` headers standard does not define `Resent-CC` or `Resent-BCC` behavior in a way the current build uses. Only `Resent-To` is set.
- **No sent-mail copy**: Redirected messages are not saved to the account's Sent folder or to the local `messages` table. There is no audit record of the redirect beyond server logs.
- **Original attachments**: Attachments included in the original message are forwarded as-is because the SMTP message is built from the original raw content. No stripping or re-encoding of attachments occurs.
- **DKIM/SPF impact**: Adding `Resent-*` headers does not re-sign the message. Receiving mail servers may flag the redirected message if they enforce strict DMARC alignment on the original `From` domain. This is expected behavior per the RFC; support staff should note this when users report delivery issues with redirected messages.
- **HTML content**: The original message body (HTML or plain text) is forwarded verbatim. No sanitization is applied at redirect time.

## Common Questions

**Q: How is redirect different from forward?**
A standard forward creates a new message from the user's address, typically wrapping the original as a quote or attachment. A redirect preserves the original `From` header and adds `Resent-*` headers, so the recipient's mail client shows the original sender. Email servers and headers distinguish the two clearly; the end-user experience varies by recipient mail client.

**Q: Will the original sender know the message was redirected?**
No notification is sent to the original sender. The `Resent-*` headers are visible to technically sophisticated recipients who inspect raw headers, but typical mail clients do not surface them in the UI.

**Q: Can I redirect to a distribution list or group address?**
Yes, as long as the address passes lettre's validation (valid RFC 5321 syntax). Whether the distribution list accepts the message depends on the receiving mail server's policies. The redirect itself will succeed on Iris's side.

**Q: Does redirect work with accounts using OAuth tokens that have expired?**
Yes. The SMTP layer in Iris automatically refreshes the XOAUTH2 access token before sending if it detects the token is expired or close to expiry. If refresh fails (e.g., refresh token revoked), the send fails with a 500 and an SMTP auth error in the logs.

**Q: Is there a size limit on redirected messages?**
No Iris-side size limit is enforced. The receiving SMTP server may reject oversized messages per its own limits (commonly 25–50 MB for most providers).

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Returns 400 "invalid address" | `to` value is malformed — missing `@`, invalid TLD format, or contains disallowed characters | Verify the full email address syntax; try copy-pasting from a sent message |
| Returns 400 "inactive account" | The account that owns the source message has been disconnected or disabled in Iris | Re-authenticate the account in Settings > Accounts |
| Returns 404 | Message ID in the URL does not exist | Confirm the message is visible in the inbox; the ID may have changed after a re-sync |
| Returns 500 / SMTP error in logs | SMTP connection failed — likely auth rejection, expired tokens, or network issue | Check `iris-server` logs for the SMTP error code; re-authenticate if XOAUTH2 token refresh is failing |
| Dialog shows no error but message not delivered | SMTP accepted the message but recipient server rejected it post-accept | Check recipient's spam folder; verify DMARC policy on original sender's domain |
| Redirect succeeds but recipient sees "sent by" attribution differently | Recipient mail client surfaces `Resent-From` differently | Expected behavior — not an Iris bug; varies by mail client |

## Related Links

- Backend: `src/api/compose.rs` (`redirect_message`), `src/smtp.rs` (`build_redirect_email`)
- Frontend dialog: `web/src/components/compose/RedirectDialog.svelte`
- RFC 2822 §3.6.6: Resent fields specification
- Prior handoffs: FH-004 (Compose & Send — shares SMTP infrastructure)
