# GC-428: SQL Injection in Relationship Email Path Parameter

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-scoring
- **Tags**: contacts, relationships, scoring, strength, sql-injection, security, path-param
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Relationship scores computed so the `relationship_scores` table (or equivalent) is non-empty (source: POST /api/contacts/relationships/compute)

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Compute scores to populate the relationships table
   - **Target**: `POST http://127.0.0.1:3000/api/contacts/relationships/compute`
   - **Input**: Header `X-Session-Token: {token}`, no body
   - **Expected**: 200 OK with `computed` >= 1

3. Send a classic OR-based SQL injection payload in the email path
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships/%27%20OR%20%271%27%3D%271`
   - **Input**: Header `X-Session-Token: {token}`; URL-decoded path param: `' OR '1'='1`
   - **Expected**: 404 Not Found (literal string treated as email address, no row matches) or 400 Bad Request (invalid email format rejected); never 200 with data

4. Verify no relationship data is returned for the injection payload
   - **Target**: Response body from step 3
   - **Input**: Inspect for `overall_score`, `strength_label`, or any score breakdown fields
   - **Expected**: Response body contains no relationship score data; body is an error message, empty, or `{"error": "..."}`

5. Send a UNION-based injection to attempt data extraction
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships/x%40y.com%20UNION%20SELECT%20email%2C1.0%2C%27strong%27%2C0%2C0%2C0%2C0%2C0%2C0%2C0%2Cnull%2Cnull--`
   - **Input**: Header `X-Session-Token: {token}`; URL-decoded: `x@y.com UNION SELECT email,1.0,'strong',0,0,0,0,0,0,0,null,null--`
   - **Expected**: 404 Not Found (no row for the literal string) or 400 Bad Request; never 200 with injected data

6. Send a stacked-query injection attempt
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships/x%40y.com%3B%20DROP%20TABLE%20contacts--`
   - **Input**: Header `X-Session-Token: {token}`; URL-decoded: `x@y.com; DROP TABLE contacts--`
   - **Expected**: 404 or 400; the `contacts` table is not dropped

7. Verify server stability and data integrity after all injection attempts
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships/stats`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with consistent stats matching step 2's `computed` value — data is intact, server is operational

## Success Criteria
- [ ] Steps 3, 5, 6 each return 404 or 400, never 200 with relationship data
- [ ] No relationship rows are surfaced via injected queries
- [ ] UNION injection does not extract or expose rows from the relationships table
- [ ] Stacked-query injection does not modify or drop any table
- [ ] Step 7 returns 200 with stats consistent with the state before injection attempts
- [ ] Response bodies reveal no SQL, table names, column names, or internal schema

## Failure Criteria
- Any injection payload in steps 3, 5, 6 returns 200 with relationship data
- UNION injection causes the server to return rows from the relationships table
- Stacked-query causes a table to be dropped or rows to be deleted
- Server returns 500 (indicating raw SQL error propagation rather than clean handling)
- Response body reveals SQL structure, Rust panic output, or table schema

## Notes
Iris uses Axum with rusqlite and parameterized queries via `rusqlite::params![]`. The email path parameter is bound as a positional parameter in the SQL WHERE clause, not interpolated as a string. This prevents injection because the entire path segment — including any SQL keywords or quotes — is treated as a literal value. This test validates that parameterized binding is actually in use. rusqlite does not support stacked queries by default (only single-statement execution), which prevents the DROP TABLE attack independently of parameterization. Both defenses should be present. URL-encode all payloads before sending; pre-compute the percent-encoded URLs rather than relying on client-side encoding.
