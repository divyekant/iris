---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
use-case: read-and-reply
slug: uc-002-read-and-reply
---

# Use Case: Read and Reply to Email

## Summary

A user opens an email thread from the inbox, reads the messages, and sends a reply. The system renders HTML email safely, marks the message as read, and sends the reply via SMTP with proper threading headers.

## Actors

- **User**: The person reading and replying to email.
- **System**: The Iris backend and frontend.
- **Recipient**: The person receiving the reply.

## Preconditions

- At least one email account is connected and synced.
- Messages are visible in the inbox.

## Flow

1. User opens the Iris inbox and sees a list of messages.
2. User clicks on a message to open the ThreadView page.
3. Frontend calls `GET /api/threads/{thread_id}` to load all messages in the thread.
4. Frontend calls `GET /api/messages/{id}` for the specific message, which returns the message detail plus trust indicators (SPF/DKIM/DMARC) and tracking pixel detection.
5. The system marks the message as read via `PUT /api/messages/{id}/read`.
6. Frontend renders the email:
   - If HTML body exists: sanitizes with DOMPurify (stripping scripts, event handlers, style attributes) and renders in a sandboxed iframe.
   - If only plain text: renders as preformatted text.
7. Trust badges (SPF/DKIM/DMARC) appear in the message header. Detected tracking pixels are flagged.
8. User clicks "Reply."
9. ComposeModal opens in reply mode:
   - `To` is pre-filled with the original sender's address.
   - `Subject` is pre-filled with "Re: {original subject}".
   - `In-Reply-To` is set to the original message's Message-ID.
   - `References` is set to the original Message-ID (or the existing references chain).
10. User writes the reply text.
11. Auto-save periodically stores the draft via `POST /api/drafts`.
12. User clicks "Send."
13. Frontend calls `POST /api/send` with the compose request.
14. Backend refreshes the OAuth token if expired.
15. Backend builds the email via `lettre` and sends via SMTP (XOAUTH2 for Gmail/Outlook).
16. Backend stores a copy in the local messages table with folder "Sent."
17. Frontend closes the compose modal and updates the thread view.

## Alternative Flow: Reply All

At step 8, user clicks "Reply All" instead of "Reply." The ComposeModal pre-fills:
- `To` with the original sender.
- `Cc` with all original To and Cc recipients (excluding the user's own address).

## Alternative Flow: Forward

At step 8, user clicks "Forward." The ComposeModal opens with:
- `To` empty (user enters recipients).
- `Subject` pre-filled with "Fwd: {original subject}."
- Body pre-filled with the original message content (quoted).
- No In-Reply-To or References headers.

## Postconditions

- The original message is marked as read in the local database.
- The reply is sent via SMTP to the recipient.
- A copy of the sent reply is stored locally with folder "Sent."
- The draft (if auto-saved) is deleted.

## Error Scenarios

| Scenario | System Response |
|---|---|
| SMTP send fails (auth error) | 502 returned; user sees error message; draft preserved |
| OAuth token refresh fails | 502 returned with token refresh error; user must re-authenticate |
| Invalid recipient address | 400 returned; user corrects the address |
| Message not found | 404 returned when loading thread |

## Related Features

- fh-003-email-reading
- fh-004-compose-send
- fh-012-auth-security (trust indicators)
