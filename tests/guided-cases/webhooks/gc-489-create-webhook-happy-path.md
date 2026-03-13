# GC-489: Create Webhook Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: webhooks
- **Tags**: webhooks, create, happy-path, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- No pre-existing webhooks required

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Create a webhook
   - **Target**: `POST http://localhost:3030/api/webhooks`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"url": "https://example.com/hook", "events": ["message.received", "message.sent"], "secret": "my-webhook-secret"}`
   - **Expected**: 201 Created with JSON body containing `id`, `url`, `events`, `secret`, `created_at` fields

3. Verify webhook exists via GET
   - **Target**: `GET http://localhost:3030/api/webhooks/{id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with matching `url`, `events`, and `id` fields

4. Verify webhook appears in list
   - **Target**: `GET http://localhost:3030/api/webhooks`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with array containing the created webhook

## Success Criteria
- [ ] POST /api/webhooks returns 201 with webhook object
- [ ] Response contains valid `id`, `url`, `events` fields
- [ ] GET /api/webhooks/{id} returns matching webhook
- [ ] GET /api/webhooks includes the webhook in the list

## Failure Criteria
- POST returns non-201 status
- Response missing required fields (id, url, events)
- GET by ID does not match created webhook
- Webhook not present in list endpoint
