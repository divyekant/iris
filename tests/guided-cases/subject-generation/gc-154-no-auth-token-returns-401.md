# GC-154: No Auth Token Returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: subject-generation
- **Tags**: subject-generation, security, auth, 401, unauthorized, api
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- No session token (deliberately omitted)

## Steps
1. Submit suggest-subject request with no X-Session-Token header
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: No `X-Session-Token` header; body `{"body": "This should not reach the AI."}`
   - **Expected**: 401 Unauthorized

2. Submit suggest-subject request with an invalid/forged token
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: forged-token-xyz-999`; body `{"body": "This should not reach the AI."}`
   - **Expected**: 401 Unauthorized

3. Submit suggest-subject request with an empty token value
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: `; body `{"body": "Empty token value."}`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] Missing token returns 401
- [ ] Forged token returns 401
- [ ] Empty token value returns 401
- [ ] No subject suggestions are returned for any unauthenticated request
- [ ] No AI invocation occurs (no side-effects)

## Failure Criteria
- Any unauthenticated request returns 200 with suggestions
- Server returns 403 instead of 401 (wrong HTTP semantics for missing credentials)
- Server returns 500
