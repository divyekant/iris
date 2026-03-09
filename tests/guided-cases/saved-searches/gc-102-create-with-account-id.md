# GC-SAVED-007: Create saved search with account_id

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: saved-searches
- **Tags**: crud, create, account-scoped
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- None required (account_id is an optional field; it does not need to reference an existing account for this test)

## Steps

1. **Obtain session token**
   - Target: `GET /api/auth/bootstrap`
   - Input: Header `Sec-Fetch-Site: same-origin`
   - Expected: 200 OK, response body contains `{"token": "..."}`

2. **Create a saved search with account_id**
   - Target: `POST /api/saved-searches`
   - Input:
     - Header `X-Session-Token: <token from step 1>`
     - Header `Content-Type: application/json`
     - Body: `{"name": "Work Only", "query": "project update", "account_id": "acc-test-123"}`
   - Expected: 201 Created, response body contains:
     - `id` — non-empty string
     - `name` — `"Work Only"`
     - `query` — `"project update"`
     - `account_id` — `"acc-test-123"`
     - `created_at` — valid ISO 8601 timestamp

3. **Verify in list**
   - Target: `GET /api/saved-searches`
   - Input:
     - Header `X-Session-Token: <token from step 1>`
   - Expected: 200 OK, array contains the created search with `account_id` equal to `"acc-test-123"`

## Success Criteria
- [ ] POST response status is 201 Created
- [ ] Response body `account_id` equals `"acc-test-123"`
- [ ] Response body `name` equals `"Work Only"`
- [ ] Response body `query` equals `"project update"`
- [ ] Saved search with `account_id` appears in GET listing

## Failure Criteria
- Response status is not 201
- `account_id` is missing or null in the response
- `account_id` value does not match the request payload
- Saved search does not appear in the list with correct `account_id`
