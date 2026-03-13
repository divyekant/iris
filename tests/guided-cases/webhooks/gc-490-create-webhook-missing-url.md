# GC-490: Create Webhook Missing URL Rejected

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: webhooks
- **Tags**: webhooks, validation, missing-field, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- None

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Create webhook without URL
   - **Target**: `POST http://localhost:3030/api/webhooks`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"events": ["message.received"]}`
   - **Expected**: 400 Bad Request with error message indicating missing `url` field

3. Create webhook with empty URL
   - **Target**: `POST http://localhost:3030/api/webhooks`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"url": "", "events": ["message.received"]}`
   - **Expected**: 400 Bad Request with error message indicating invalid URL

## Success Criteria
- [ ] Missing URL returns 400
- [ ] Empty URL returns 400
- [ ] Error messages clearly indicate the validation failure

## Failure Criteria
- Either request returns 201 or 200
- Server returns 500 instead of 400
