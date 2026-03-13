# GC-491: Create Webhook Empty Events Rejected

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: webhooks
- **Tags**: webhooks, validation, empty-events, POST
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

2. Create webhook with empty events array
   - **Target**: `POST http://localhost:3030/api/webhooks`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"url": "https://example.com/hook", "events": []}`
   - **Expected**: 400 Bad Request with error indicating events cannot be empty

3. Create webhook with missing events field
   - **Target**: `POST http://localhost:3030/api/webhooks`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"url": "https://example.com/hook"}`
   - **Expected**: 400 Bad Request with error indicating missing events

## Success Criteria
- [ ] Empty events array returns 400
- [ ] Missing events field returns 400
- [ ] Error messages clearly describe the validation issue

## Failure Criteria
- Either request returns 201 or 200
- Webhook created with no events subscribed
