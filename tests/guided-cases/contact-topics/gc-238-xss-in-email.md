# GC-238: Contact Topics XSS in Email Parameter

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: contact-topics
- **Tags**: topics, security, xss
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No specific data required

## Steps
1. Attempt reflected XSS via script tag in email
   - **Target**: `GET /api/contacts/%3Cscript%3Ealert(1)%3C%2Fscript%3E@evil.com/topics`
   - **Input**: Header `X-Session-Token: {token}` (email: `<script>alert(1)</script>@evil.com`)
   - **Expected**: 400 Bad Request (@ present but malicious), or 200 with email echoed as plain text (not HTML)

2. Verify response Content-Type
   - **Target**: Response headers from step 1
   - **Input**: Check `Content-Type` header
   - **Expected**: `application/json` — never `text/html`

3. Verify email field is not rendered as HTML
   - **Target**: Response body `email` field
   - **Input**: Inspect raw JSON
   - **Expected**: Script tag appears as literal string in JSON, properly escaped (`\u003c` or literal `<` within JSON string)

4. Attempt XSS via event handler in email
   - **Target**: `GET /api/contacts/x%22onmouseover%3Dalert(1)%22@evil.com/topics`
   - **Input**: Header `X-Session-Token: {token}` (email: `x"onmouseover=alert(1)"@evil.com`)
   - **Expected**: 400 or 200 with email as literal string, no attribute injection

5. Attempt XSS via img tag in email
   - **Target**: `GET /api/contacts/%3Cimg%20src%3Dx%20onerror%3Dalert(1)%3E@evil.com/topics`
   - **Input**: Header `X-Session-Token: {token}` (email: `<img src=x onerror=alert(1)>@evil.com`)
   - **Expected**: 400 or 200 with email as literal string

## Success Criteria
- [ ] Response Content-Type is always `application/json`
- [ ] Script tags in email field are never rendered as HTML
- [ ] Email field in JSON response contains the literal input string (properly escaped)
- [ ] No HTML tags are interpreted in the response
- [ ] Event handlers in email parameter are not executed
- [ ] All responses are 200 or 400, never 500

## Failure Criteria
- Response Content-Type is `text/html`
- Script tag appears unescaped in a non-JSON response
- Server returns 500 (unhandled error)
- Any XSS payload is reflected in a way that could execute in a browser

## Notes
The API returns JSON, which inherently prevents reflected XSS as long as the Content-Type header is `application/json`. The frontend (ContactTopicsPanel.svelte) uses Svelte's text interpolation `{email}` which auto-escapes HTML. This case verifies both the API response format and that malicious email strings are treated as opaque data, not markup. The real risk would be if the email were ever inserted into innerHTML — Svelte's reactivity system prevents this by default.
