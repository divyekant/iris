# GC-SAVED-003: Delete a saved search

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: saved-searches
- **Tags**: crud, delete, happy-path
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- A saved search must exist (created in step 2)

## Steps

1. **Obtain session token**
   - Target: `GET /api/auth/bootstrap`
   - Input: Header `Sec-Fetch-Site: same-origin`
   - Expected: 200 OK, response body contains `{"token": "..."}`

2. **Create a saved search to delete**
   - Target: `POST /api/saved-searches`
   - Input:
     - Header `X-Session-Token: <token from step 1>`
     - Header `Content-Type: application/json`
     - Body: `{"name": "Temp Search", "query": "is:starred"}`
   - Expected: 201 Created, note the returned `id`

3. **Delete the saved search**
   - Target: `DELETE /api/saved-searches/<id from step 2>`
   - Input:
     - Header `X-Session-Token: <token from step 1>`
   - Expected: 204 No Content, empty response body

4. **Verify deletion by listing saved searches**
   - Target: `GET /api/saved-searches`
   - Input:
     - Header `X-Session-Token: <token from step 1>`
   - Expected: 200 OK, response array does NOT contain an entry with the deleted `id`

## Success Criteria
- [ ] DELETE response status is 204 No Content
- [ ] DELETE response body is empty
- [ ] Subsequent GET /api/saved-searches does not contain the deleted search
- [ ] No server error occurs during deletion

## Failure Criteria
- DELETE response status is not 204
- Deleted search still appears in subsequent GET listing
- Server returns 500 or other unexpected error
