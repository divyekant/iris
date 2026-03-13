# GC-493: Webhook Endpoints With Invalid Auth Return 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: webhooks
- **Tags**: webhooks, auth, invalid-token, 401, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- Invalid token: `invalid-token-12345`

## Steps
1. POST create webhook with invalid token
   - **Target**: `POST http://localhost:3030/api/webhooks`
   - **Input**: Header `X-Session-Token: invalid-token-12345`, Header `Content-Type: application/json`, Body `{"url": "https://example.com/hook", "events": ["message.received"]}`
   - **Expected**: 401 Unauthorized

2. GET list webhooks with invalid token
   - **Target**: `GET http://localhost:3030/api/webhooks`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

3. GET webhook deliveries with invalid token
   - **Target**: `GET http://localhost:3030/api/webhooks/some-id/deliveries`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] All endpoints return 401 with invalid session token
- [ ] No data leaked in any response

## Failure Criteria
- Any endpoint returns 200/201 with invalid token
- Response contains webhook or delivery data
