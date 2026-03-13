# GC-338: No auth returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: markdown-compose
- **Tags**: markdown, preview, auth, 401, session-token
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- No session token (or an invalid one) is used

### Data
- No pre-existing data required (source: inline)

## Steps

1. Send a markdown preview request with no `X-Session-Token` header
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "# Test"
     }
     ```
   - **Headers**: `Content-Type: application/json` (X-Session-Token intentionally omitted)
   - **Expected**: HTTP 401 Unauthorized

2. Verify the response body indicates an auth failure
   - **Target**: Response body from step 1
   - **Expected**: Body contains an error message indicating missing or invalid authentication (e.g., `{"error": "..."}`)

3. Send a markdown preview request with an invalid session token
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "# Test"
     }
     ```
   - **Headers**: `X-Session-Token: invalid-token-abc123`, `Content-Type: application/json`
   - **Expected**: HTTP 401 Unauthorized

4. Verify no HTML output is returned for either unauthenticated request
   - **Target**: Response body from steps 1 and 3
   - **Expected**: Neither response contains an `html` field with rendered content

## Success Criteria
- [ ] Step 1 returns HTTP 401
- [ ] Step 3 returns HTTP 401
- [ ] Neither unauthenticated response contains a valid `html` field
- [ ] Error response body is present and indicates an auth failure

## Failure Criteria
- Either request returns HTTP 200 (auth bypass)
- Either request returns HTML preview content without a valid session token
- Server returns 500 instead of 401 for missing/invalid token

## Notes
Session auth is the only auth layer on this endpoint. A missing header and an invalid header must both be rejected.
