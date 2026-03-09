# GC-SAVED-001: Create a saved search via API

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: saved-searches
- **Tags**: crud, create, happy-path
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- No pre-existing saved searches required

## Steps

1. **Obtain session token**
   - Target: `GET /api/auth/bootstrap`
   - Input: Header `Sec-Fetch-Site: same-origin`
   - Expected: 200 OK, response body contains `{"token": "..."}`

2. **Create a saved search**
   - Target: `POST /api/saved-searches`
   - Input:
     - Header `X-Session-Token: <token from step 1>`
     - Header `Content-Type: application/json`
     - Body: `{"name": "VIP Unread", "query": "from:boss@acme.com is:unread"}`
   - Expected: 201 Created, response body contains:
     - `id` — non-empty string
     - `name` — `"VIP Unread"`
     - `query` — `"from:boss@acme.com is:unread"`
     - `created_at` — valid ISO 8601 timestamp
     - `account_id` — null or absent (not provided in request)

## Success Criteria
- [ ] Response status is 201 Created
- [ ] Response body `name` equals `"VIP Unread"`
- [ ] Response body `query` equals `"from:boss@acme.com is:unread"`
- [ ] Response body contains a non-empty `id`
- [ ] Response body contains a valid `created_at` timestamp

## Failure Criteria
- Response status is not 201
- Response body missing `id`, `name`, `query`, or `created_at`
- `name` or `query` values do not match the request payload
