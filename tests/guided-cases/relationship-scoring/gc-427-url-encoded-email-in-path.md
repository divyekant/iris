# GC-427: URL-Encoded Email in Relationship Path Parameter

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-scoring
- **Tags**: contacts, relationships, scoring, strength, url-encoding, path-param, edge
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Relationship scores computed and a score exists for a contact whose email contains a `+` sign (e.g., `alice+work@example.com`) — plus signs in emails require URL encoding as `%2B` in path segments (source: POST /api/contacts/relationships/compute after sync with a plus-addressed contact)

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Compute relationship scores to ensure data exists
   - **Target**: `POST http://127.0.0.1:3000/api/contacts/relationships/compute`
   - **Input**: Header `X-Session-Token: {token}`, no body
   - **Expected**: 200 OK with `computed` >= 1

3. Fetch relationship for a plus-addressed contact using URL-encoded path
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships/alice%2Bwork%40example.com`
   - **Input**: Header `X-Session-Token: {token}`; URL-decoded path param resolves to `alice+work@example.com`
   - **Expected**: 200 OK with the relationship detail for `alice+work@example.com` — same result as if the email were in the query string

4. Fetch the same contact without URL encoding (raw `+` and `@` in path)
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships/alice+work@example.com`
   - **Input**: Header `X-Session-Token: {token}`; `+` may be interpreted as space by some HTTP clients/routers
   - **Expected**: 200 OK with the same relationship detail — or 404 if the router treats `+` as a space and the decoded address `alice work@example.com` does not match any contact

5. Compare results from steps 3 and 4
   - **Target**: Response bodies from steps 3 and 4
   - **Input**: If both return 200, compare `overall_score` and `strength_label`
   - **Expected**: If both are 200, the scores are identical; if step 4 is 404 that is acceptable (documents that `%2B` encoding is required for plus-addressed contacts)

6. Test with a percent-encoded `@` sign — double encoding
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships/alice%40example.com`
   - **Input**: Header `X-Session-Token: {token}`; `%40` decodes to `@`
   - **Expected**: 200 OK — server correctly decodes the percent-encoded `@` and resolves the contact `alice@example.com`

## Success Criteria
- [ ] Step 3 returns 200 for the percent-encoded plus-addressed email
- [ ] Step 6 returns 200 for the percent-encoded `@` sign
- [ ] Response for step 3 contains valid relationship data for the correct contact
- [ ] Response for step 6 contains valid relationship data for `alice@example.com`
- [ ] No server error (500) on any URL-encoded input

## Failure Criteria
- Step 3 returns 404 when a score for `alice+work@example.com` exists in the database
- Step 6 returns 404 when `alice@example.com` has a computed score
- Server returns 500 for any URL-encoded path parameter
- Double-decoded address causes a server error

## Notes
RFC 3986 requires that path segments are percent-decoded by the HTTP framework before routing. Axum's `Path` extractor performs this decoding automatically. The edge case is `+` — in query strings `+` means space, but in path segments `+` is a literal plus character. Verify that `alice+work@example.com` and `alice%2Bwork%40example.com` resolve to the same contact. If the test environment does not have a plus-addressed contact, use a contact with a subdomain period (e.g., `bob.smith@sub.example.com`) to validate general URL decoding.
