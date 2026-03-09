# GC-SAVED-002: List saved searches returns created search

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: saved-searches
- **Tags**: crud, list, happy-path
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- At least one saved search created (run gc-saved-001 first, or create one in step 2)

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
     - Body: `{"name": "Team Updates", "query": "subject:standup OR subject:retro"}`
   - Expected: 201 Created, note the returned `id`

3. **List all saved searches**
   - Target: `GET /api/saved-searches`
   - Input:
     - Header `X-Session-Token: <token from step 1>`
   - Expected: 200 OK, response body is a JSON array containing at least one object where:
     - `id` matches the id from step 2
     - `name` equals `"Team Updates"`
     - `query` equals `"subject:standup OR subject:retro"`
     - `created_at` is present

## Success Criteria
- [ ] GET response status is 200 OK
- [ ] Response body is a JSON array
- [ ] Array contains an entry matching the saved search created in step 2
- [ ] Matching entry has correct `id`, `name`, `query`, and `created_at`

## Failure Criteria
- GET response status is not 200
- Response body is not an array
- Created saved search is not present in the list
- Fields do not match the values from the create request
