# GC-298: XSS payload in chat message does not execute

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: bulk-chat-ops
- **Tags**: xss, security, injection, sanitization, chat, bulk
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available

### Data
- No special data required; XSS payload is injected via the chat message field itself

## Steps

1. Send a chat message containing an XSS payload in the bulk operation request
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc298-session", "message": "Archive all emails from <script>alert(1)</script><img src=x onerror=alert(document.cookie)> sender"}'
     ```
   - **Expected**: 200 OK — server accepts the request and processes it without crashing; the XSS content is treated as plain text

2. Verify the response body does not contain unescaped script tags
   - **Target**: Response JSON `message.content`
   - **Expected**: No `<script>` tags appear literally in the response JSON value; JSON serialization properly escapes angle brackets (e.g., `\u003cscript\u003e`) or the AI paraphrases the content without reproducing the raw HTML

3. Verify the stored message does not contain executable HTML
   - **Target**: `GET http://localhost:3000/api/ai/chat/history?session_id=gc298-session`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/ai/chat/history?session_id=gc298-session" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: The user message in history contains the text stored safely; the response JSON is valid and parseable; no raw `<script>` or `onerror=` patterns appear as unescaped HTML in the content field values

4. Verify the proposed_action (if any) has safe message_ids
   - **Target**: Response JSON `message.proposed_action.message_ids`
   - **Expected**: If a proposed_action is returned, message_ids are standard UUID strings or empty; no injected script content appears in the message_ids array

5. Confirm that the JSON response is valid (not broken by angle brackets)
   - **Target**: Full response JSON parsing
   - **Input**: Pipe response through `jq .` or equivalent JSON parser
   - **Expected**: Response parses as valid JSON without errors

## Success Criteria
- [ ] Server returns 200 (does not crash on XSS-laden input)
- [ ] Response JSON is valid and parseable
- [ ] No unescaped `<script>` tags appear in any response field value
- [ ] No `onerror`, `onclick`, or other HTML event handlers appear unescaped in the response
- [ ] Stored chat history does not contain executable HTML

## Failure Criteria
- Server returns 500 due to malformed prompt construction from XSS input
- Response JSON is invalid (parse error) due to angle brackets
- `message.content` contains raw `<script>alert(1)</script>` as unescaped HTML
- `message.proposed_action` contains XSS payload in any field

## Notes
The user message is passed into the AI prompt via `build_chat_prompt`. Rust's `serde_json` serialization escapes special characters in string values. The AI should not echo the XSS payload verbatim. The frontend renders `message.content` as text, not HTML; this test ensures the API layer does not introduce a stored XSS vector.
