# GC-195: Needs-Reply No Auth Returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: needs-reply
- **Tags**: needs-reply, api, auth, security, 401
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- No session token used in the request

## Steps

1. Call the needs-reply endpoint without authentication
   - **Target**: `GET /api/messages/needs-reply`
   - **Input**: No `X-Session-Token` header
   - **Expected**: 401 Unauthorized

2. Call with an invalid session token
   - **Target**: `GET /api/messages/needs-reply`
   - **Input**: `X-Session-Token: invalid-token-value`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] Request without token returns 401
- [ ] Request with invalid token returns 401
- [ ] No message data is leaked in the error response

## Failure Criteria
- Response status is 200 or any status other than 401
- Message data is returned without valid authentication
