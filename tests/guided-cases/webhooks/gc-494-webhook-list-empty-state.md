# GC-494: Webhook List Empty State

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: webhooks
- **Tags**: webhooks, empty-state, list, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- No webhooks have been created (clean state or all deleted)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. List webhooks when none exist
   - **Target**: `GET http://localhost:3030/api/webhooks`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with empty array `[]`

## Success Criteria
- [ ] GET /api/webhooks returns 200 with empty array
- [ ] Response is valid JSON (not null or error)

## Failure Criteria
- Returns non-200 status code
- Returns null instead of empty array
- Returns 500 error
