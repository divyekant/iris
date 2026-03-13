# GC-318: SQL Injection in Email Path Parameter

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-priority
- **Tags**: relationship-priority, sql-injection, security, path-param, injection
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- Relationship scores have been computed (source: POST /api/ai/relationship-priority) so the `relationship_scores` table is non-empty

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Send a classic SQL injection payload in the email path segment
   - **Target**: `GET http://localhost:3000/api/contacts/%27%20OR%201%3D1--%20/relationship`
   - **Input**: Header `X-Session-Token: {token}`; URL-decoded path param is `' OR 1=1-- `
   - **Expected**: 404 Not Found (no matching row for the literal string `' OR 1=1-- `) or 400 Bad Request (invalid email format rejected)

3. Verify no data is returned
   - **Target**: Response body from step 2
   - **Input**: Inspect for any `RelationshipScore` objects or database dump content
   - **Expected**: Response body contains no relationship score data; body is an error message or empty

4. Send a UNION-based injection attempt
   - **Target**: `GET http://localhost:3000/api/contacts/x%40y.com%20UNION%20SELECT%20email%2Cscore%2C0%2C0%2C0%2C0%2C0%2Cnow()%20FROM%20relationship_scores--/relationship`
   - **Input**: Header `X-Session-Token: {token}`; URL-decoded: `x@y.com UNION SELECT email,score,0,0,0,0,0,now() FROM relationship_scores--`
   - **Expected**: 404 Not Found (literal string treated as an email, no row matches) or 400 Bad Request

5. Verify server stability
   - **Target**: `GET http://localhost:3000/api/ai/relationship-priority` (or any other valid endpoint)
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK — server remains operational after injection attempts

## Success Criteria
- [ ] SQL injection payload in step 2 returns 404 or 400, never 200 with data
- [ ] UNION injection in step 4 returns 404 or 400, never 200 with data
- [ ] No relationship score rows are returned via injected query
- [ ] Server remains stable and responsive after all injection attempts
- [ ] Response body reveals no table schema or database content

## Failure Criteria
- Any injection attempt returns 200 with relationship score data
- UNION injection causes the server to return rows from the `relationship_scores` table
- Server crashes or returns 500 due to malformed SQL
- Response body reveals table names, column names, or internal SQL structure

## Notes
Iris uses Axum with rusqlite and parameterized queries via `rusqlite::params![]`. The email path parameter is bound as a query parameter, not interpolated into SQL strings, which prevents injection. This test validates that parameterized binding is actually used rather than string concatenation. URL-encode the payload before sending; curl handles this with `--data-urlencode` on path params or via pre-encoded URLs.
