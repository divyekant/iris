# GC-328: XSS in subject field does not execute

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: cc-suggestions
- **Tags**: cc-suggestions, security, xss, injection, input-sanitization
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and reachable

### Data
- At least one synced account with messages (source: prior sync)
- A contact `alice@example.com` with some co-occurrence history (source: seed or real inbox)

## Steps
1. POST to suggest-cc with a script tag in the `subject` field
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": ["alice@example.com"],
       "cc": [],
       "subject": "<script>alert('xss')</script>Project update",
       "body_preview": "Here's the latest."
     }
     ```
   - **Expected**: 200 OK or 400 Bad Request — response body is valid JSON with `Content-Type: application/json`, no script execution triggered

2. POST with an event-handler payload in `subject`
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": ["alice@example.com"],
       "cc": [],
       "subject": "<img src=x onerror=alert('xss')>Q2 update",
       "body_preview": "Here's the latest."
     }
     ```
   - **Expected**: 200 OK or 400 — response is `application/json`, HTML payload is not rendered

3. POST with a `javascript:` URI in `body_preview`
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": ["alice@example.com"],
       "cc": [],
       "subject": "Normal subject",
       "body_preview": "<a href=\"javascript:alert('xss')\">click</a>"
     }
     ```
   - **Expected**: 200 OK or 400 — response is `application/json`, no script execution

4. Verify response Content-Type for all three requests
   - **Target**: Response headers
   - **Input**: `Content-Type` header value
   - **Expected**: `application/json` in all cases — never `text/html`

5. Verify XSS payload does not appear unescaped in `reason` field of suggestions
   - **Target**: `reason` field in any returned suggestions
   - **Input**: String value
   - **Expected**: If the subject influenced the AI's reason text, the payload appears as escaped text (e.g., `&lt;script&gt;`) or is omitted — never as raw HTML that could execute in a browser

## Success Criteria
- [ ] All requests return 200 or 400 (not 500)
- [ ] `Content-Type` is `application/json` in all responses
- [ ] XSS payloads in `subject` and `body_preview` do not appear unescaped in any response field
- [ ] No script execution is triggered by the API response
- [ ] `reason` field, if present, does not echo back raw HTML from the input

## Failure Criteria
- Response `Content-Type` is `text/html`
- Raw HTML/script tags appear unescaped in any response field
- 500 Internal Server Error caused by the XSS payload
- AI echoes back the raw script tag in a `reason` field verbatim in a form that could execute in a browser

## Notes
The `subject` and `body_preview` fields are forwarded to the AI as prompt context. The AI's output (particularly `reason` strings) is returned in the JSON response. The server must ensure `Content-Type: application/json` so browsers treat the response as data, not markup. XSS prevention at the API layer is enforced by the JSON content type — UI rendering safety is a separate concern. The test verifies the API layer does not accidentally reflect HTML payloads in a way that bypasses JSON encoding.
