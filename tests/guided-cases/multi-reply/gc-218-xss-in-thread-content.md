# GC-218: XSS payload in thread content does not execute in response

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: multi-reply
- **Tags**: xss, security, injection, sanitization
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and enabled

### Data
- A thread containing a message with XSS payload in subject or body (source: manually inserted or crafted test message)
- Example message body: `<script>alert('xss')</script><img src=x onerror=alert(1)>`
- Known `thread_id` for the thread containing this message

## Steps
1. Ensure a message with XSS content exists in a thread
   - **Target**: Database or sync
   - **Input**: Thread with message body containing `<script>alert('xss')</script>` and subject containing `<img src=x onerror=alert(1)>`
   - **Expected**: Message stored in DB (email content is stored as-is)

2. Send multi-reply request for the XSS-laden thread
   - **Target**: `POST /api/ai/multi-reply`
   - **Input**: `{ "thread_id": "<xss_thread_id>" }`
   - **Expected**: 200 OK with 3 reply options

3. Inspect response for XSS artifacts
   - **Target**: Response JSON
   - **Input**: Examine `subject` and `body` fields of all 3 options
   - **Expected**: No raw `<script>` tags, no `onerror` handlers, no executable HTML in any field

4. Verify response is valid JSON (not broken by angle brackets)
   - **Target**: Response parsing
   - **Input**: Parse response as JSON
   - **Expected**: Valid JSON — angle brackets in source content do not break JSON serialization

## Success Criteria
- [ ] Response status is 200
- [ ] Response is valid JSON (no parse errors)
- [ ] No `<script>` tags appear in any option's subject or body
- [ ] No event handler attributes (`onerror`, `onclick`, etc.) appear in option fields
- [ ] AI-generated reply text is contextually appropriate (not echoing raw HTML)

## Failure Criteria
- Response contains unescaped `<script>` tags
- Response contains HTML event handlers
- JSON serialization is broken by angle brackets in source content
- Server returns 500 due to malformed prompt construction

## Notes
The multi-reply prompt is built from the last 5 messages in the thread (body truncated to 500 chars). XSS content in the source email passes through `build_multi_reply_prompt` into the AI prompt. The AI should generate reply text (not echo HTML), and Rust's serde_json serialization properly escapes special characters in the JSON response. The frontend (MultiReplyPicker.svelte) renders body text, so this also validates that the API does not pass through raw HTML that could execute in the browser.
