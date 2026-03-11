# GC-185: No Auth Token Returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: one-click-unsubscribe
- **Tags**: unsubscribe, auth, 401, security
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Any valid message ID (does not need to have an unsubscribe URL)

## Steps
1. POST to the unsubscribe endpoint without any X-Session-Token header
   - **Target**: POST /api/messages/{id}/unsubscribe
   - **Input**: none; omit `X-Session-Token` header entirely
   - **Expected**: 401 Unauthorized

2. Repeat with an invalid/garbage token value
   - **Target**: POST /api/messages/{id}/unsubscribe
   - **Input**: `X-Session-Token: invalid-token-xyz`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] Both requests return 401
- [ ] No unsubscribe action is performed
- [ ] Response does not leak internal details (stack traces, DB errors)

## Failure Criteria
- Either request returns 200, 404, or 500 instead of 401
- Unauthenticated request causes any side effect
