# GC-206: XSS payload in email address — sanitized rejection

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: bounce-redirect
- **Tags**: redirect, security, xss, injection
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- Existing message ID with a valid `from_address` (source: seed or prior sync)
- Active account linked to the message

## Steps
1. Send redirect request with script tag in email
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "<script>alert(1)</script>@example.com" }`
   - **Expected**: 400 Bad Request — email format validation rejects angle brackets

2. Send redirect request with HTML entity injection
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "user@example.com<img src=x onerror=alert(1)>" }`
   - **Expected**: 400 Bad Request — email format validation rejects invalid characters

3. Send redirect request with JavaScript protocol in email
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "javascript:alert(1)@example.com" }`
   - **Expected**: 400 Bad Request — colon in local part fails validation or domain check fails

4. Verify error messages do not reflect unsanitized input
   - **Target**: Response body of all above requests
   - **Input**: Inspect error message text
   - **Expected**: Error messages do not contain raw `<script>`, `<img>`, or `javascript:` strings from the input

## Success Criteria
- [ ] All three requests return 400 status
- [ ] No email is sent via SMTP
- [ ] Error responses do not reflect unsanitized XSS payloads back to the client
- [ ] No script execution vectors in any response body

## Failure Criteria
- Any request succeeds (200) and an XSS payload is sent via SMTP
- Error response reflects raw HTML/script tags without escaping
- Server returns 500 indicating unhandled input

## Notes
Critical security case. Even though the API returns JSON (not HTML), the `to` field value in error messages or in the success response must not contain unescaped injection payloads. Email format validation should reject these before any further processing.
