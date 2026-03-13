# GC-248: Email with special characters handled correctly

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: response-times
- **Tags**: response-times, edge-case, special-chars, email-format
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- None required (validation and query behavior test)

## Steps
1. Request response times for email with dots and hyphens
   - **Target**: `GET /api/contacts/first.last-name@sub.example.com/response-times`
   - **Input**: email = `first.last-name@sub.example.com`
   - **Expected**: 200 OK (valid email accepted, zero stats if no data)

2. Request response times for email with plus subaddress
   - **Target**: `GET /api/contacts/user+inbox@example.com/response-times`
   - **Input**: email = `user+inbox@example.com`
   - **Expected**: 200 OK (valid email accepted, zero stats if no data)

3. Request response times for email with LIKE metacharacters
   - **Target**: `GET /api/contacts/user%25test@example.com/response-times`
   - **Input**: email = `user%test@example.com` (URL-encoded % as %25)
   - **Expected**: 200 OK, the `%` in the email is treated literally in the LIKE query, not as a wildcard

4. Request response times for email with underscore
   - **Target**: `GET /api/contacts/user_name@example.com/response-times`
   - **Input**: email = `user_name@example.com`
   - **Expected**: 200 OK, the `_` is treated literally in the LIKE query, not as a single-char wildcard

## Success Criteria
- [ ] All requests return 200 (not 400 or 500)
- [ ] Emails with dots, hyphens, plus signs are accepted
- [ ] `%` in email is not interpreted as SQL LIKE wildcard
- [ ] `_` in email is not interpreted as SQL LIKE single-char wildcard
- [ ] `email` field in each response matches the input email exactly

## Failure Criteria
- Valid special-character emails rejected as invalid
- LIKE metacharacters cause unintended wildcard matching (returning other contacts' data)
- Server error due to unescaped special characters
- Response email field differs from input (e.g., truncated or mangled)

## Notes
The response-time algorithm uses LIKE for to/cc matching. SQL LIKE treats `%` as multi-char wildcard and `_` as single-char wildcard. If these are not escaped with the ESCAPE clause, an email like `user%test@example.com` could match `user-anything-test@example.com`. This test confirms proper LIKE escaping.
