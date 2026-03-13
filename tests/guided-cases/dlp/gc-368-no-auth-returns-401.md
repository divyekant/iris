# GC-368: No Auth Returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: dlp
- **Tags**: auth, 401, security, unauthenticated, scan-dlp, dlp-override
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- No session token (deliberately omitted to test auth enforcement)

## Steps

1. Call the DLP scan endpoint without a session token
   - **Target**: `POST http://localhost:3030/api/compose/scan-dlp`
   - **Input**: No `X-Session-Token` header, Body: `{"subject": "Test", "body": "Hello world", "to": ["a@b.com"]}`
   - **Expected**: 401 Unauthorized

2. Call the DLP scan endpoint with an invalid session token
   - **Target**: `POST http://localhost:3030/api/compose/scan-dlp`
   - **Input**: Header `X-Session-Token: invalid-token-xyz`, Body: `{"subject": "Test", "body": "Hello world", "to": ["a@b.com"]}`
   - **Expected**: 401 Unauthorized

3. Call the DLP override endpoint without a session token
   - **Target**: `POST http://localhost:3030/api/compose/dlp-override`
   - **Input**: No `X-Session-Token` header, Body: `{"findings_acknowledged": true}`
   - **Expected**: 401 Unauthorized

4. Call the DLP override endpoint with an invalid session token
   - **Target**: `POST http://localhost:3030/api/compose/dlp-override`
   - **Input**: Header `X-Session-Token: invalid-token-xyz`, Body: `{"findings_acknowledged": true}`
   - **Expected**: 401 Unauthorized

5. Verify no sensitive data is leaked in any error response
   - **Target**: Response bodies from steps 1–4
   - **Input**: Inspect each response body
   - **Expected**: No email content, scan results, or override tokens appear in any 401 response body

## Success Criteria
- [ ] `POST /api/compose/scan-dlp` without token returns 401
- [ ] `POST /api/compose/scan-dlp` with invalid token returns 401
- [ ] `POST /api/compose/dlp-override` without token returns 401
- [ ] `POST /api/compose/dlp-override` with invalid token returns 401
- [ ] No scan results or tokens are leaked in the 401 response body

## Failure Criteria
- Any DLP endpoint returns 200 without a valid session token
- A DLP scan result or override token is returned in the 401 response
- Server returns 500 instead of 401 for unauthenticated requests

## Notes
Both DLP endpoints require session auth via the `X-Session-Token` header. Since DLP scans process email content (which may itself contain sensitive data), unauthenticated access to these endpoints must be blocked. An attacker must not be able to probe the DLP engine without a valid session.
