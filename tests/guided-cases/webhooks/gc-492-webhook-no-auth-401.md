# GC-492: Webhook Endpoints Without Auth Return 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: webhooks
- **Tags**: webhooks, auth, 401, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- None

## Steps
1. POST create webhook without session token
   - **Target**: `POST http://localhost:3030/api/webhooks`
   - **Input**: Header `Content-Type: application/json`, Body `{"url": "https://example.com/hook", "events": ["message.received"]}`
   - **Expected**: 401 Unauthorized

2. GET list webhooks without session token
   - **Target**: `GET http://localhost:3030/api/webhooks`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

3. DELETE webhook without session token
   - **Target**: `DELETE http://localhost:3030/api/webhooks/some-id`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

4. POST test webhook without session token
   - **Target**: `POST http://localhost:3030/api/webhooks/some-id/test`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] All four endpoints return 401 without X-Session-Token
- [ ] No webhook data leaked in response body

## Failure Criteria
- Any endpoint returns 200/201/404 without authentication
- Response body contains webhook data
