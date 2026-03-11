# GC-221: Briefing — No Auth Returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: briefing
- **Tags**: briefing, api, auth, security, 401
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- No session token used in the request

## Steps

1. Call briefing endpoint without authentication
   - **Target**: `GET /api/ai/briefing`
   - **Input**: No `X-Session-Token` header
   - **Expected**: 401 Unauthorized

2. Call with an invalid session token
   - **Target**: `GET /api/ai/briefing`
   - **Input**: `X-Session-Token: fake-token-123`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] Request without token returns 401
- [ ] Request with invalid token returns 401
- [ ] No briefing data is leaked in the error response

## Failure Criteria
- Response status is 200 or any status other than 401
- Briefing data is returned without valid authentication
