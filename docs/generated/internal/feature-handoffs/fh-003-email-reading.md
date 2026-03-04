---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
feature: email-reading
slug: fh-003-email-reading
---

# Feature Handoff: Email Reading

## What It Does

Email reading covers how Iris parses, stores, and displays email message content. It includes MIME parsing for extracting text, HTML, and attachments; thread grouping via RFC headers; and safe HTML rendering in the browser using DOMPurify and a sandboxed iframe.

## How It Works

### MIME Parsing (`src/imap/sync.rs`)

The `parse_mime_body` function concatenates raw headers and body, then uses the `mailparse` crate (v0.15) to parse the MIME structure:

1. Concatenates raw headers + `\r\n\r\n` + raw body to form a complete RFC 2822 message.
2. Calls `mailparse::parse_mail` on the combined bytes.
3. Recursively walks the MIME tree via `extract_mime_parts`:
   - **text/plain** parts are captured as `body_text` (first occurrence wins).
   - **text/html** parts are captured as `body_html` (first occurrence wins).
   - Parts with `Content-Disposition: attachment` are recorded as `AttachmentMeta` (filename, mime_type, size) but content is not stored.
4. If MIME parsing fails entirely, the raw body is returned as plaintext.

### Thread ID Extraction

The `extract_thread_id` function in `src/imap/sync.rs` determines which thread a message belongs to:

1. Parse the `References` header. If present, use the **first** message-id (thread root).
2. Else, parse the `In-Reply-To` header and use that message-id.
3. Else, fall back to the message's own `Message-ID`.

All message-ids have angle brackets stripped (`<`, `>`). This approach groups all messages in a conversation under the thread root's message-id.

### Message Detail Model

The `MessageDetail` struct (from `src/models/message.rs`) includes all message fields: id, account_id, thread_id, folder, from/to/cc addresses, subject, snippet, date, body_text, body_html, flags (read, starred, attachments), and AI metadata (intent, priority, category, summary).

### API Endpoints

- `GET /api/messages/{id}` -- returns `MessageDetailResponse` which includes the full `MessageDetail` plus `TrustIndicators` (SPF/DKIM/DMARC) and detected `TrackingPixel` list.
- `GET /api/threads/{id}` -- returns all messages in a thread, ordered chronologically.

### HTML Rendering (Frontend)

Email HTML is rendered in the Svelte frontend using:

1. **DOMPurify** sanitizes the HTML, stripping dangerous elements (scripts, event handlers) and removing inline `style` attributes for security.
2. A **sandboxed iframe** (`sandbox` attribute) isolates the rendered HTML from the parent page, preventing script execution and form submission.

## User-Facing Behavior

- Clicking a message in the inbox opens the ThreadView page.
- The thread view shows all messages in the conversation, with the most recent expanded.
- HTML emails render inside a sandboxed iframe. Plain-text emails are displayed as preformatted text.
- Attachment metadata (filename, type, size) is shown but downloading is not yet implemented.
- Trust indicators (SPF/DKIM/DMARC badges) appear in the message header.
- Detected tracking pixels are flagged visually.

## Configuration

No specific configuration. MIME parsing and thread extraction are automatic during sync.

## Edge Cases and Limitations

- Attachment content is not stored -- only metadata (filename, mime_type, size). Downloading attachments is not implemented.
- Only the first text/plain and first text/html part are captured. Multipart emails with multiple text parts may lose secondary content.
- Thread ID extraction relies on `References` and `In-Reply-To` headers. If a message lacks these headers (e.g., some newsletter replies), it will be treated as a standalone thread.
- The 200-character snippet is derived from the extracted plain text body, with control characters filtered out.
- DOMPurify removes inline styles, which means some visual formatting in HTML emails may be lost.
- External images in HTML emails are loaded directly by the iframe. There is no image proxy or blocking mechanism.
- If `mailparse` fails to parse the MIME structure, the raw body text is used as fallback. HTML content will be lost in this case.

## Common Questions

**Q: Why do some emails look plain even though they were sent as HTML?**
A: DOMPurify strips inline `style` attributes for security. This can remove formatting that relies heavily on inline CSS. The HTML structure (headings, links, images) is preserved.

**Q: How does thread grouping work for forwarded messages?**
A: Forwarded messages typically do not include `In-Reply-To` or `References` headers from the original thread. They will be treated as new standalone threads unless the mail client adds these headers.

**Q: Why are attachment downloads not available?**
A: The current implementation stores only attachment metadata during IMAP sync. The actual attachment content is not fetched or stored. This is a known limitation planned for future work.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Email body shows raw MIME headers | MIME parsing failed; raw body used as fallback | Check server logs for `mailparse` errors; inspect the email's raw structure |
| Thread shows single message despite being a reply | Missing `References`/`In-Reply-To` headers | This is a sender-side issue; the mail client did not set threading headers |
| HTML email appears unstyled | DOMPurify stripped inline styles | Expected behavior for security; no workaround |
| Tracking pixel detected but email looks fine | Tracking detection is informational only | The pixel is flagged but not blocked |
| Garbled text in email body | Character encoding issue in MIME parsing | Check the email's charset declaration; `mailparse` handles common encodings |

## Related Links

- Source: `src/imap/sync.rs` (parse_mime_body, extract_thread_id), `src/api/messages.rs`, `src/api/threads.rs`, `src/api/trust.rs`
- Models: `src/models/message.rs`
- Frontend: `web/src/pages/ThreadView.svelte`, `web/src/components/EmailBody.svelte`
