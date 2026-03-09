# GC-SAVED-005: Create saved search with empty query rejected

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: saved-searches
- **Tags**: validation, create, empty-query
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- None required

## Steps

1. **Obtain session token**
   - Target: `GET /api/auth/bootstrap`
   - Input: Header `Sec-Fetch-Site: same-origin`
   - Expected: 200 OK, response body contains `{"token": "..."}`

2. **Attempt to create a saved search with empty query**
   - Target: `POST /api/saved-searches`
   - Input:
     - Header `X-Session-Token: <token from step 1>`
     - Header `Content-Type: application/json`
     - Body: `{"name": "Test", "query": ""}`
   - Expected: 400 Bad Request, response body contains an error message indicating the query is required or invalid

3. **Verify no saved search was persisted**
   - Target: `GET /api/saved-searches`
   - Input:
     - Header `X-Session-Token: <token from step 1>`
   - Expected: 200 OK, no entry with `name` equal to `"Test"` and empty `query` exists

## Success Criteria
- [ ] POST response status is 400 Bad Request
- [ ] Response body contains a meaningful error message
- [ ] No saved search with empty query is persisted in the database

## Failure Criteria
- POST response status is 201 (server accepted empty query)
- POST response status is 500 (server error instead of validation error)
- A saved search with empty query appears in the list
