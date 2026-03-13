# GC-497: Update and Delete Webhook

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: webhooks
- **Tags**: webhooks, update, delete, PUT, DELETE
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
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"url": "https://example.com/original", "events": ["message.received"]}`
   - **Expected**: 201 Created with `id` field

3. Update the webhook URL and events
   - **Target**: `PUT http://localhost:3030/api/webhooks/{id}`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"url": "https://example.com/updated", "events": ["message.received", "message.sent"]}`
   - **Expected**: 200 OK with updated `url` and `events`

4. Verify update persisted
   - **Target**: `GET http://localhost:3030/api/webhooks/{id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with `url` = `https://example.com/updated` and `events` containing both event types

5. Delete the webhook
   - **Target**: `DELETE http://localhost:3030/api/webhooks/{id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK or 204 No Content

6. Verify deletion
   - **Target**: `GET http://localhost:3030/api/webhooks/{id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] PUT updates webhook URL and events correctly
- [ ] GET confirms updated values persisted
- [ ] DELETE removes the webhook
- [ ] GET after DELETE returns 404

## Failure Criteria
- PUT returns non-200 status
- Updated values not reflected in GET
- DELETE returns error
- Webhook still accessible after deletion
