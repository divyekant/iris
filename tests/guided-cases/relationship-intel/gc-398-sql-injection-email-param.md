# GC-398: Security — SQL injection attempt in email path parameter is rejected safely

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-intel
- **Tags**: contacts, intelligence, relationship, security, sql-injection, input-sanitization
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- None required

## Steps
1. Attempt classic OR 1=1 injection via intelligence detail endpoint
   - **Target**: `GET /api/contacts/x@x.com'%20OR%20'1'='1/intelligence`
   - **Input**: Header `X-Session-Token: {token}` (email = `x@x.com' OR '1'='1`)
   - **Expected**: 200 OK with zero/null stats (treated as literal email, no rows found) OR 400 Bad Request

2. Attempt UNION SELECT to exfiltrate schema
   - **Target**: `GET /api/contacts/x@x.com'%20UNION%20SELECT%201,2,3,4,5--/intelligence`
   - **Input**: Header `X-Session-Token: {token}` (email = `x@x.com' UNION SELECT 1,2,3,4,5--`)
   - **Expected**: 200 OK with zero stats OR 400 — no database schema or row data returned

3. Attempt DROP TABLE via stacked query
   - **Target**: `GET /api/contacts/x@x.com'%3B%20DROP%20TABLE%20relationship_scores%3B--/intelligence`
   - **Input**: Header `X-Session-Token: {token}` (email = `x@x.com'; DROP TABLE relationship_scores;--`)
   - **Expected**: 200 OK with zero stats OR 400 — relationship_scores table remains intact

4. Verify database integrity after all injection attempts
   - **Target**: `GET /api/contacts/intelligence/summary`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with normal data — confirms relationship_scores table was not dropped

5. Attempt injection via AI summary POST endpoint
   - **Target**: `POST /api/contacts/x@x.com'%20OR%20'1'='1/intelligence/ai-summary`
   - **Input**: Header `X-Session-Token: {token}`, body `{}`
   - **Expected**: 200 OK with empty/error response OR 404 — no cross-contact data leakage

## Success Criteria
- [ ] No SQL injection succeeds — no unauthorized data returned from any attempt
- [ ] UNION SELECT does not return schema information or row data from other tables
- [ ] OR 1=1 does not return data from all contacts
- [ ] DROP TABLE does not execute — relationship_scores table intact after all attempts
- [ ] No 500 errors from any injection attempt
- [ ] All responses are 200 (zero/null stats) or 4xx, never 500
- [ ] AI summary endpoint also resistant to injection in path parameter

## Failure Criteria
- Any response contains data from contacts other than the exact injected email string
- UNION SELECT returns sqlite_master or other table data
- relationship_scores table is dropped or corrupted
- 500 Internal Server Error with SQL error details
- Raw SQL error message exposed in response body

## Notes
Rust's rusqlite uses parameterized queries by default, which prevents SQL injection via bound parameters. This test confirms the email path parameter is passed as a parameterized value and never interpolated directly into SQL strings. SQLite also does not support stacked queries (`;` does not execute a second statement via the standard API), providing an additional safety layer. The relationship_scores table is specifically targeted in step 3 since it is the core data store for this feature.
