# GC-246: Email with URL-encoded characters in path

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: response-times
- **Tags**: response-times, url-encoding, path-param, special-chars
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- None required (validation behavior test)

## Steps
1. Request response times with a URL-encoded email
   - **Target**: `GET /api/contacts/user%2Btag%40example.com/response-times`
   - **Input**: URL-encoded form of `user+tag@example.com` (+ encoded as %2B, @ encoded as %40)
   - **Expected**: 200 OK, server correctly decodes the email to `user+tag@example.com` and uses it for the query

2. Verify email field in response matches decoded value
   - **Target**: Response JSON inspection
   - **Input**: Check `email` field
   - **Expected**: `email` = `user+tag@example.com` (decoded, not the raw URL-encoded form)

## Success Criteria
- [ ] Response status is 200 (not 400)
- [ ] Server correctly URL-decodes the path parameter
- [ ] `email` field in response is the decoded address `user+tag@example.com`
- [ ] The `+` character is preserved (not interpreted as space)

## Failure Criteria
- Server rejects the URL-encoded email as invalid
- Email decoded incorrectly (e.g., `+` becomes space)
- Server returns 500 due to path parsing error
- Double-encoding issue (email stored as `user%2Btag%40example.com`)

## Notes
Email addresses in URL path segments must be URL-encoded when they contain reserved characters like `+` and `@`. The Axum router should auto-decode path parameters, but this test confirms the full round-trip. The `+` subaddress tag is particularly important since `+` means space in query strings but should be literal in path segments.
