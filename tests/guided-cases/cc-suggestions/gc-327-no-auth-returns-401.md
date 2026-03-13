# GC-327: No auth returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: cc-suggestions
- **Tags**: cc-suggestions, security, authentication, 401, session-token
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- No session token (or an invalid/expired session token)

### Data
- No specific data required

## Steps
1. POST to suggest-cc with no session token header
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": ["alice@example.com"],
       "cc": [],
       "subject": "Project update",
       "body_preview": "Latest status."
     }
     ```
   - **Headers**: Omit `X-Session-Token` header entirely
   - **Expected**: 401 Unauthorized — no suggestions returned, no data leaked

2. POST to suggest-cc with a forged/invalid session token
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**: Same request body as step 1
   - **Headers**: `X-Session-Token: invalid-token-value-abc123`
   - **Expected**: 401 Unauthorized

3. POST to suggest-cc with an empty session token header
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**: Same request body as step 1
   - **Headers**: `X-Session-Token: ` (empty string)
   - **Expected**: 401 Unauthorized

4. Verify response body for unauthenticated requests
   - **Target**: Response body for all three 401 responses
   - **Input**: JSON body
   - **Expected**: Error body does not expose internal details (stack trace, DB schema, session internals)

## Success Criteria
- [ ] All three unauthenticated requests return 401
- [ ] No suggestion data returned in any 401 response
- [ ] Error body is safe (no internal information leaked)
- [ ] `WWW-Authenticate` header may be present but is not required

## Failure Criteria
- Any unauthenticated request returns 200 with data
- Any unauthenticated request returns 403 instead of 401
- Internal implementation details exposed in the error body
- 500 Internal Server Error on missing/invalid token

## Notes
Session auth is enforced via `X-Session-Token` header, validated by the auth middleware. The suggest-cc endpoint must not be reachable without a valid session. This mirrors the auth pattern verified across all other authenticated endpoints in the Iris API.
