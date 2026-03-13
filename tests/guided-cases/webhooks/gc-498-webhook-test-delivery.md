# GC-498: Webhook Test Delivery and Delivery History

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: webhooks
- **Tags**: webhooks, test-delivery, deliveries, POST, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- None (webhook created in test)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Create a webhook
   - **Target**: `POST http://localhost:3030/api/webhooks`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"url": "https://httpbin.org/post", "events": ["message.received"]}`
   - **Expected**: 201 Created with `id` field

3. Send test event
   - **Target**: `POST http://localhost:3030/api/webhooks/{id}/test`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with delivery result (success or failure status, response code)

4. List deliveries
   - **Target**: `GET http://localhost:3030/api/webhooks/{id}/deliveries`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with array containing at least one delivery record with `event`, `status`, `response_code`, `delivered_at` fields

## Success Criteria
- [ ] POST /api/webhooks/{id}/test returns 200 with delivery result
- [ ] GET /api/webhooks/{id}/deliveries returns array with the test delivery
- [ ] Delivery record contains event type, status, and timestamp

## Failure Criteria
- Test endpoint returns non-200 status
- Deliveries list is empty after sending test
- Delivery record missing expected fields
