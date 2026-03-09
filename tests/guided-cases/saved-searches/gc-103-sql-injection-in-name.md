# GC-SAVED-008: SQL injection in saved search name

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: saved-searches
- **Tags**: security, sql-injection, create
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

2. **Create a saved search with SQL injection payload in name**
   - Target: `POST /api/saved-searches`
   - Input:
     - Header `X-Session-Token: <token from step 1>`
     - Header `Content-Type: application/json`
     - Body: `{"name": "'; DROP TABLE saved_searches; --", "query": "test"}`
   - Expected: 201 Created (parameterized queries prevent injection), response body `name` equals the literal string `"'; DROP TABLE saved_searches; --"`

3. **Verify database integrity — list saved searches**
   - Target: `GET /api/saved-searches`
   - Input:
     - Header `X-Session-Token: <token from step 1>`
   - Expected: 200 OK, response is a valid JSON array (table was NOT dropped), and contains the saved search from step 2 with the injection string stored literally as the name

4. **Verify the saved search can be retrieved with literal name**
   - Target: Inspect the array from step 3
   - Input: Find entry where `name` equals `"'; DROP TABLE saved_searches; --"`
   - Expected: Entry exists with `query` equal to `"test"` and a valid `id` and `created_at`

## Success Criteria
- [ ] POST response status is 201 Created (not 500 or connection error)
- [ ] Response body `name` contains the literal SQL injection string, stored as-is
- [ ] Subsequent GET /api/saved-searches returns 200 (table still exists)
- [ ] The saved_searches table was NOT dropped or corrupted
- [ ] The injection payload is treated as plain text data, not executed as SQL

## Failure Criteria
- POST returns 500 or causes a server crash (injection executed partially)
- GET /api/saved-searches returns 500 or empty (table was dropped)
- The `name` field is silently sanitized or truncated (indicates fragile handling)
- Any evidence of SQL execution from the injected payload
