# gc-search-010: XSS in Operator Value

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: search-operators
- **Tags**: search, operator, xss, security, injection
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- Any messages in the database (results not required to match)

## Steps

1. Obtain session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap` with header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `{"token": "<session_token>"}`

2. Search with XSS payload in from: operator
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=from:<script>alert(1)</script>` (URL-encoded: `from:%3Cscript%3Ealert(1)%3C/script%3E`) with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with JSON response containing:
     - `parsed_operators` includes `{"key": "from", "value": "<script>alert(1)</script>"}`
     - The value is treated as a literal SQL LIKE pattern `%<script>alert(1)</script>%`
     - No script execution occurs (JSON response is not HTML)
     - Response Content-Type is `application/json` (not text/html)
     - No SQL injection (value is parameterized, not interpolated)

3. Search with SQL injection attempt in operator value
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=from:'; DROP TABLE messages; --` (URL-encoded) with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK (or empty results); no database error or table drop. Parameterized queries prevent SQL injection.

4. Search with XSS in subject: operator with quotes
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=subject:"<img onerror=alert(1) src=x>"` (URL-encoded) with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK; value parsed as literal string `<img onerror=alert(1) src=x>`, treated as LIKE pattern, no code execution

5. Verify response Content-Type header
   - **Target**: Inspect response headers from any of the above requests
   - **Expected**: Content-Type is `application/json` (Axum's `Json` extractor sets this automatically)

## Success Criteria
- [ ] All responses return 200 status (no server crash)
- [ ] Response Content-Type is application/json (not text/html)
- [ ] XSS payloads appear as literal strings in parsed_operators values, not as executable HTML
- [ ] SQL injection payloads do not cause database errors or data loss
- [ ] Parameterized SQL queries prevent any injection (values passed via `?N` placeholders)
- [ ] The messages table remains intact after SQL injection attempts

## Failure Criteria
- Server returns 500 Internal Server Error
- Response Content-Type is text/html (enabling XSS)
- SQL injection causes database error or data modification
- Server panics or crashes on malicious input
- XSS payload is reflected in a way that could execute in a browser context
