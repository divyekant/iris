# GC-396: Edge — URL-encoded email address in path parameter is decoded correctly

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-intel
- **Tags**: contacts, intelligence, relationship, url-encoding, path-param, special-chars
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- A contact with an email containing `+` or other RFC 5321 special characters, e.g., `user+tag@example.com` (source: seed or real inbox)

## Steps
1. Request intelligence using a URL-encoded email in the path
   - **Target**: `GET /api/contacts/user%2Btag%40example.com/intelligence`
   - **Input**: Header `X-Session-Token: {token}` (URL-encoded form of `user+tag@example.com`)
   - **Expected**: 200 OK; server decodes path parameter to `user+tag@example.com` before querying

2. Verify `email` field in response matches decoded address
   - **Target**: Response JSON `email` field
   - **Input**: Inspect string value
   - **Expected**: `email` = `user+tag@example.com` (decoded); not `user%2Btag%40example.com` (raw encoded form)

3. Verify `+` is not decoded as space
   - **Target**: Response JSON `email` field
   - **Input**: Check for space character
   - **Expected**: `email` does not contain a space where the `+` should be (no query-string-style decoding applied to path segment)

4. Request AI summary for the same URL-encoded email
   - **Target**: `POST /api/contacts/user%2Btag%40example.com/intelligence/ai-summary`
   - **Input**: Header `X-Session-Token: {token}`, body `{}`
   - **Expected**: 200 OK or 404; if 200, `email` field in response = `user+tag@example.com`

## Success Criteria
- [ ] GET intelligence returns 200 (not 400)
- [ ] `email` in response = `user+tag@example.com` (decoded)
- [ ] `+` character is preserved as literal `+`, not decoded to space
- [ ] AI summary endpoint also handles URL-encoded path correctly
- [ ] No double-encoding artifacts in any response field

## Failure Criteria
- Server returns 400 rejecting the URL-encoded path
- `email` in response contains `%2B` or `%40` (not decoded)
- `email` contains a space character (incorrect `+`-to-space decoding)
- 500 error from path parsing failure

## Notes
Email addresses with `+` subaddress tags are valid per RFC 5321 and common in practice (e.g., Gmail filtering). When placed in a URL path, `@` must be encoded as `%40` and `+` as `%2B`. Axum auto-decodes path parameters — this test confirms the decoding is correct and that `+` is not mis-decoded as space (which only applies in query strings, not path segments).
