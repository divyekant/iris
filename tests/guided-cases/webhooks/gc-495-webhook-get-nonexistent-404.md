# GC-495: Get Nonexistent Webhook Returns 404

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: webhooks
- **Tags**: webhooks, not-found, 404, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Non-existent webhook ID: `nonexistent-webhook-999`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. GET nonexistent webhook
   - **Target**: `GET http://localhost:3030/api/webhooks/nonexistent-webhook-999`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

3. DELETE nonexistent webhook
   - **Target**: `DELETE http://localhost:3030/api/webhooks/nonexistent-webhook-999`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

4. GET deliveries for nonexistent webhook
   - **Target**: `GET http://localhost:3030/api/webhooks/nonexistent-webhook-999/deliveries`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] GET /api/webhooks/{id} returns 404 for nonexistent ID
- [ ] DELETE /api/webhooks/{id} returns 404 for nonexistent ID
- [ ] GET /api/webhooks/{id}/deliveries returns 404 for nonexistent ID

## Failure Criteria
- Any endpoint returns 200 with empty data
- Any endpoint returns 500
