# GC-202: Draft from Intent — No Auth Returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: draft-from-intent
- **Tags**: draft-from-intent, api, auth, security, 401
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- No session token used in the request

## Steps

1. Call draft-from-intent without authentication
   - **Target**: `POST /api/ai/draft-from-intent`
   - **Input**: `{"intent": "Ask about project status"}` — no `X-Session-Token` header
   - **Expected**: 401 Unauthorized

2. Call with an invalid session token
   - **Target**: `POST /api/ai/draft-from-intent`
   - **Input**: `{"intent": "Ask about project status"}` with `X-Session-Token: bogus-token`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] Request without token returns 401
- [ ] Request with invalid token returns 401
- [ ] No draft content is generated or returned

## Failure Criteria
- Response status is 200 or any status other than 401
- Draft content is returned without valid authentication
