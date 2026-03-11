# GC-212: Grammar Check — No Auth Returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: grammar-check
- **Tags**: grammar-check, api, auth, security, 401
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- No session token used in the request

## Steps

1. Call grammar-check without authentication
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"content": "Check this text please."}` — no `X-Session-Token` header
   - **Expected**: 401 Unauthorized

2. Call with an invalid session token
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"content": "Check this text please."}` with `X-Session-Token: invalid-token`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] Request without token returns 401
- [ ] Request with invalid token returns 401
- [ ] No grammar check results are returned

## Failure Criteria
- Response status is 200 or any status other than 401
- Grammar check results are returned without valid authentication
