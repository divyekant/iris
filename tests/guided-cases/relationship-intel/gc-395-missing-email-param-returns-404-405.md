# GC-395: Negative — missing email parameter in path returns 404 or 405

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-intel
- **Tags**: contacts, intelligence, relationship, missing-param, routing
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- None required

## Steps
1. Request intelligence detail with no email in path (trailing slash only)
   - **Target**: `GET /api/contacts//intelligence`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found or 400 Bad Request (route not matched, or empty segment rejected)

2. Request AI summary with no email in path
   - **Target**: `POST /api/contacts//intelligence/ai-summary`
   - **Input**: Header `X-Session-Token: {token}`, body `{}`
   - **Expected**: 404 Not Found or 400 Bad Request

3. Request intelligence detail with whitespace-only email
   - **Target**: `GET /api/contacts/%20/intelligence`
   - **Input**: Header `X-Session-Token: {token}` (email = space character, URL-encoded)
   - **Expected**: 400 Bad Request (whitespace-only email is invalid) or 404 Not Found

## Success Criteria
- [ ] All requests return 4xx (not 200, not 500)
- [ ] No server panic or unhandled exception
- [ ] Response body contains an error indicator (no empty body on 4xx)
- [ ] Whitespace-only email is not treated as a valid address

## Failure Criteria
- Any request returns 200 with data
- 500 Internal Server Error
- Server crashes or returns no response
- Whitespace email resolves to real contact data

## Notes
Tests router and input validation boundaries. Axum route matching should reject empty path segments; the server must not panic. A whitespace-only email must never match a real contact row in the database.
