# GC-531: Bulk Action with Empty IDs Rejected

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: subscription-management
- **Tags**: subscriptions, validation, bulk-action, empty-ids
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

2. Bulk action with empty IDs array
   - **Target**: `POST http://localhost:3030/api/subscriptions/bulk-action`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"ids": [], "action": "unsubscribe"}`
   - **Expected**: 400 Bad Request with error indicating IDs cannot be empty

3. Bulk action with missing IDs field
   - **Target**: `POST http://localhost:3030/api/subscriptions/bulk-action`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"action": "unsubscribe"}`
   - **Expected**: 400 Bad Request with error indicating missing IDs

4. Bulk action with missing action field
   - **Target**: `POST http://localhost:3030/api/subscriptions/bulk-action`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"ids": ["some-id"]}`
   - **Expected**: 400 Bad Request with error indicating missing action

## Success Criteria
- [ ] Empty IDs returns 400
- [ ] Missing IDs returns 400
- [ ] Missing action returns 400
- [ ] Error messages describe the validation failure

## Failure Criteria
- Any request returns 200 or 201
- Server returns 500 instead of 400
