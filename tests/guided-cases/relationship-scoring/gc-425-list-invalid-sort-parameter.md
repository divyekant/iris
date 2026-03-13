# GC-425: List Relationships with Invalid Sort Parameter

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-scoring
- **Tags**: contacts, relationships, scoring, strength, list, invalid-param, negative
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Relationship scores may or may not be computed — this test validates parameter rejection regardless of data state

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Send a list request with an unrecognized sort value
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships?sort=hacker&limit=20&offset=0`
   - **Input**: Header `X-Session-Token: {token}`; sort value `hacker` is not a valid sort key
   - **Expected**: 400 Bad Request — server rejects the invalid sort parameter; OR 200 with a default sort applied (if the implementation silently falls back to a safe default)

3. Send a list request with a SQL-like sort injection attempt
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships?sort=score%3B%20DROP%20TABLE%20contacts--&limit=20&offset=0`
   - **Input**: Header `X-Session-Token: {token}`; URL-decoded sort value: `score; DROP TABLE contacts--`
   - **Expected**: 400 Bad Request or 200 with default sort — never an unhandled sort causing database modification

4. Send a list request with an empty sort value
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships?sort=&limit=20&offset=0`
   - **Input**: Header `X-Session-Token: {token}`; sort is an empty string
   - **Expected**: 400 Bad Request or 200 with a default sort (e.g., `score` descending) — no 500

5. Verify server stability after all invalid requests
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships?sort=score&limit=5&offset=0`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK — server responds normally with a valid sort parameter after all invalid attempts

## Success Criteria
- [ ] Steps 2, 3, 4 each return 400 Bad Request OR 200 with a safe default sort (never 500)
- [ ] The SQL injection sort value in step 3 does not cause any table modification or data loss
- [ ] Step 5 (valid request after invalid ones) returns 200
- [ ] No server crash or panic across all steps

## Failure Criteria
- Any of steps 2, 3, 4 returns 500
- Step 3 causes a table DROP or any database modification
- Step 5 returns non-200 (server destabilized by prior bad input)
- Server panics on any of the invalid sort values

## Notes
The `sort` parameter should be validated against an allowlist of known sort keys (e.g., `score`, `email`, `last_received`). If the implementation silently defaults to `score` on unrecognized values, document that behavior and verify no injection is possible. The SQL injection in step 3 is specifically testing that sort values are allowlisted rather than interpolated into ORDER BY clauses.
