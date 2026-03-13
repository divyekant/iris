# GC-236: Contact Topics Email URL Encoding — Special Characters

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: contact-topics
- **Tags**: topics, url-encoding, special-characters
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No specific data required (testing URL parsing, not data retrieval)

## Steps
1. Request topics with a plus-addressed email (URL encoded)
   - **Target**: `GET /api/contacts/user%2Btag@example.com/topics`
   - **Input**: Header `X-Session-Token: {token}` (email: `user+tag@example.com`)
   - **Expected**: 200 OK, `email` field is `user+tag@example.com`

2. Request topics with dots in local part
   - **Target**: `GET /api/contacts/first.last@example.com/topics`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `email` field is `first.last@example.com`

3. Request topics with percent-encoded @ symbol
   - **Target**: `GET /api/contacts/user%40example.com/topics`
   - **Input**: Header `X-Session-Token: {token}` (email: `user@example.com` with @ encoded)
   - **Expected**: 200 OK with email resolved to `user@example.com`, OR 400 if double-encoding causes issues

4. Request topics with uppercase email
   - **Target**: `GET /api/contacts/Alice@Example.COM/topics`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, email matching is case-insensitive

## Success Criteria
- [ ] Plus-addressed email is correctly decoded from URL encoding
- [ ] Dots in local part are handled without issues
- [ ] Percent-encoded @ is handled gracefully (200 or 400, not 500)
- [ ] Uppercase email is accepted (no 400 error)
- [ ] No server errors (500) for any encoding variant

## Failure Criteria
- Server returns 500 for any URL-encoded email variant
- Plus sign is stripped or misinterpreted as space
- Email with encoded @ causes a routing error or panic

## Notes
Email addresses in URL paths need proper percent-encoding for special characters. The `+` character is particularly tricky — in query strings it represents a space, but in path segments it is literal. Axum's path extractor should handle this correctly. The `%40` case tests whether the framework decodes the @ before or after route matching.
