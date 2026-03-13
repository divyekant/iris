# GC-403: Negative — Scan Non-Existent Message Returns 404

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: link-safety
- **Tags**: links, safety, scanning, 404, not-found, negative
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A message ID that does not exist in the local database, e.g. `nonexistent-msg-id-99999`

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Attempt to scan links for a non-existent message
   - **Target**: `POST http://127.0.0.1:3000/api/messages/nonexistent-msg-id-99999/scan-links`
   - **Input**: Header `X-Session-Token: {token}`; no request body
   - **Expected**: 404 Not Found with a JSON error body (e.g. `{"error": "Message not found"}` or similar)

3. Verify the response body is a proper error object
   - **Target**: (inspect response body from step 2)
   - **Input**: response body
   - **Expected**: body contains an `error` or `message` field with a human-readable description; body does NOT contain a `links` array or `summary` object

## Success Criteria
- [ ] Response status is exactly 404
- [ ] Response body contains an error description field (`error` or `message`)
- [ ] Response body does not contain `links` or `summary` keys
- [ ] Content-Type is `application/json`

## Failure Criteria
- Response status is 200 (scan succeeds on a non-existent message)
- Response status is 500 (unhandled server error instead of clean 404)
- Response body is empty or not valid JSON
- Response contains a `links` array (even an empty one) instead of an error
