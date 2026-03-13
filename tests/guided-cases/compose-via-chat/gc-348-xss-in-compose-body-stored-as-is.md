# GC-348: XSS in compose body is stored as-is (draft, not rendered)

## Metadata
- **Type**: security
- **Priority**: P1
- **Surface**: api
- **Flow**: compose-via-chat
- **Tags**: xss, security, compose, draft, injection, sanitization
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available (ai_enabled = "true")
- At least one active email account configured

### Data
- Fresh session ID (e.g. `gc348-session`) — source: inline
- XSS payload to embed in compose instructions

## Steps

1. Send a compose chat request where the message itself contains an XSS payload in the content being drafted
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc348-session", "message": "Draft an email to hack@example.com. The body should contain exactly this text: <script>alert(document.cookie)</script><img src=x onerror=alert(1)>"}'
     ```
   - **Expected**: 200 OK. The server processes the request without crashing. The response JSON is valid.

2. Inspect the proposed action body for XSS content
   - **Target**: `response.message.proposed_action.data.body`
   - **Expected**: `body` field may contain the XSS payload as a string value (the backend stores compose body as-is — it is not rendered as HTML by the API layer). The JSON is properly escaped by serde_json, so angle brackets appear as `\u003c` in the raw JSON bytes. Parsing the JSON gives back the string including the tags, but this is acceptable: drafts are not rendered server-side.

3. Confirm the compose proposal to save the draft
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc348-session", "message_id": "<assistant_msg_id>"}'
     ```
   - **Expected**: 200 OK with `{ "executed": true, "updated": 1, "draft_id": "<uuid>" }`.

4. Fetch the saved draft and check body_html storage
   - **Target**: `GET http://localhost:3000/api/messages/<draft_id>`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/messages/<draft_id>" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: `body_html` contains the compose body (including any XSS tags) as a raw stored string. The key invariant is that the API response is valid JSON — serde_json serializes the string with proper escaping. `body_text` has HTML tags stripped (the `strip_html_tags` function in `execute_compose_email` removes tags, producing plain text).

5. Verify the chat message content itself does not reflect raw XSS
   - **Target**: `response.message.content` from step 1
   - **Expected**: The AI assistant's reply text does not reproduce the raw `<script>` tag verbatim in a way that would be rendered as HTML. The JSON response body is valid and parseable by `jq .`.

6. Confirm the JSON response is parseable
   - **Target**: Full response from step 1
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc348-session-2", "message": "Draft an email to test@example.com with body <script>alert(1)</script>"}' \
       | jq .
     ```
   - **Expected**: `jq` parses the response without error (exit code 0).

## Success Criteria
- [ ] Server returns 200 for a compose request containing XSS in the message text
- [ ] Response JSON is valid and parseable (no JSON syntax errors from angle brackets)
- [ ] `draft_id` is returned by confirm — draft is stored
- [ ] `body_text` in the saved draft has HTML tags stripped (plain text only)
- [ ] The API JSON response encodes angle brackets properly (serde_json escapes them)
- [ ] Server does not crash or return 500 due to XSS-laden input

## Failure Criteria
- Server returns 500 due to XSS payload in the prompt or body
- Response JSON is malformed or unparseable
- `body_text` contains raw HTML tags (strip_html_tags not applied)
- Draft is not saved despite confirm returning 200

## Notes
The security boundary for compose drafts is the email client (ComposeModal) — drafts are stored and later rendered in a sandboxed context. The API stores the body as-is. This test verifies: (1) the API layer does not crash on XSS input; (2) JSON serialization remains valid; (3) `body_text` has tags stripped. It does NOT assert that `body_html` is sanitized server-side — that sanitization occurs at send time or in the UI.
