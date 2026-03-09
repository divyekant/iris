# GC-081: Set Notifications with Invalid Body (Missing enabled Field)

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: per-account-notifications
- **Tags**: notifications, put, validation, invalid-body, negative
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one account exists (source: local-db, fetch via `GET /api/accounts` and use first element's `id`)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Fetch account list to get a valid account ID
   - **Target**: `GET http://localhost:3030/api/accounts`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response is a JSON array with at least one account object

3. Send PUT with empty JSON body
   - **Target**: `PUT http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{}`
   - **Expected**: 400 Bad Request or 422 Unprocessable Entity

4. Send PUT with wrong field name
   - **Target**: `PUT http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"notify": true}`
   - **Expected**: 400 Bad Request or 422 Unprocessable Entity

5. Send PUT with non-boolean enabled value
   - **Target**: `PUT http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"enabled": "yes"}`
   - **Expected**: 400 Bad Request or 422 Unprocessable Entity

6. Verify notification state unchanged
   - **Target**: `GET http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, notification state is unchanged from before steps 3-5

## Success Criteria
- [ ] Empty body returns 400 or 422
- [ ] Wrong field name returns 400 or 422
- [ ] Non-boolean value returns 400 or 422
- [ ] None of the invalid requests modify the notification preference

## Failure Criteria
- Any of the invalid bodies returns 200 OK
- Notification state is changed by an invalid request
- Server returns 500 Internal Server Error (indicates unhandled deserialization)
