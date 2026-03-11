# GC-156: XSS in Body Parameter Is Handled Safely

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: subject-generation
- **Tags**: subject-generation, security, xss, injection, api
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- XSS payloads prepared (see step inputs)

## Steps
1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 with `token` field

2. Submit suggest-subject with a classic script-injection body
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": "<script>alert('xss')</script>Please schedule the meeting."}`
   - **Expected**: 200 or 400 — if 200, the `suggestions` array contains plain strings; no script tags appear in the JSON response; Content-Type is application/json

3. Submit suggest-subject with an event-handler injection body
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": "<img src=x onerror=alert(1)> Regarding the invoice."}`
   - **Expected**: 200 or 400 — if 200, suggestions contain plain strings; no HTML attribute injection in the JSON output

4. Verify response Content-Type for both payloads
   - **Target**: inspect HTTP response headers from steps 2 and 3
   - **Input**: none
   - **Expected**: `Content-Type: application/json` (not text/html)

## Success Criteria
- [ ] Server does not crash (no 500)
- [ ] If 200: response Content-Type is application/json
- [ ] If 200: suggestions are plain strings — no `<script>` or event-handler attributes in the returned JSON
- [ ] If 400: rejection is consistent and does not expose internal details
- [ ] No reflected XSS possible through the API response

## Failure Criteria
- Response Content-Type is text/html
- Suggestions array contains raw HTML tags or script blocks from the input
- Server returns 500
