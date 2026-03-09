# GC-SAVED-006: Delete non-existent saved search returns 404

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: saved-searches
- **Tags**: delete, not-found, error-handling
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- No saved search with id `nonexistent-id-12345` exists

## Steps

1. **Obtain session token**
   - Target: `GET /api/auth/bootstrap`
   - Input: Header `Sec-Fetch-Site: same-origin`
   - Expected: 200 OK, response body contains `{"token": "..."}`

2. **Attempt to delete a non-existent saved search**
   - Target: `DELETE /api/saved-searches/nonexistent-id-12345`
   - Input:
     - Header `X-Session-Token: <token from step 1>`
   - Expected: 404 Not Found, response body contains an error message indicating the saved search was not found

## Success Criteria
- [ ] DELETE response status is 404 Not Found
- [ ] Response body contains a meaningful error message
- [ ] Server does not return 204 (false success) or 500 (server error)

## Failure Criteria
- DELETE response status is 204 (silent success for non-existent resource)
- DELETE response status is 500 (unhandled error)
- Server crashes or logs an unhandled exception
