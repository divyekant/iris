# gc-mute-010: XSS Payload in Thread ID

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: mute-thread
- **Tags**: xss, security, injection, sanitization, thread-id, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- Malicious thread ID: `<script>alert('xss')</script>` (source: inline)
- URL-encoded form: `%3Cscript%3Ealert('xss')%3C/script%3E`

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Attempt to mute with XSS thread ID
   - **Target**: `PUT http://localhost:3030/api/threads/%3Cscript%3Ealert('xss')%3C%2Fscript%3E/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: Either 400 Bad Request (input rejected) or 200 OK (stored as plain text, not executed). The response body MUST NOT contain unescaped HTML tags.

3. Check mute status of XSS thread ID
   - **Target**: `GET http://localhost:3030/api/threads/%3Cscript%3Ealert('xss')%3C%2Fscript%3E/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: Response Content-Type is `application/json`. Body contains JSON-escaped string if thread was stored (e.g., `{"muted": true}`) -- no raw HTML in response.

4. List muted threads and inspect response
   - **Target**: `GET http://localhost:3030/api/muted-threads`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with `Content-Type: application/json`. If the XSS thread ID was stored, it appears as a JSON-escaped string (e.g., `"\u003cscript\u003ealert('xss')\u003c/script\u003e"` or literal `"<script>alert('xss')</script>"` inside JSON string). No raw HTML rendered.

5. Clean up: unmute the XSS thread ID
   - **Target**: `DELETE http://localhost:3030/api/threads/%3Cscript%3Ealert('xss')%3C%2Fscript%3E/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": false}`

## Success Criteria
- [ ] No endpoint returns unescaped HTML in a non-JSON content type
- [ ] All responses use `Content-Type: application/json` (XSS cannot execute in JSON responses)
- [ ] If the thread ID is stored, it is stored as a plain text string (not interpreted as HTML)
- [ ] No 500 Internal Server Error from special characters in the path
- [ ] The XSS payload does not appear in any HTML-rendered context (e.g., error pages)

## Failure Criteria
- Response Content-Type is `text/html` and contains unescaped `<script>` tags
- Server crashes (500) on special characters in thread ID path segment
- XSS payload is reflected in a way that could execute in a browser

## Notes
Since Iris uses Axum with JSON serialization (serde_json), JSON responses inherently escape special characters. The primary risk is if any error handler or fallback returns HTML content type with the reflected path. Axum's default error responses use plain text, so this should be safe, but it is critical to verify.
