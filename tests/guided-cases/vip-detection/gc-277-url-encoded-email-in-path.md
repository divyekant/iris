# GC-277: URL-Encoded Email in Path Parameter

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: vip-detection
- **Tags**: vip, contacts, url-encoding, path-parameter, edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token obtained via `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A contact email with special characters requiring URL encoding: `test+tag@sub.example.com` (source: inline)
- This contact may or may not exist in `vip_contacts`; the zero-score fallback from the vip-score endpoint covers the not-found case

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Set a contact with a plus sign in the email as manual VIP using URL encoding
   - **Target**: `PUT http://localhost:3000/api/contacts/test%2Btag%40sub.example.com/vip`
   - **Input**:
     ```
     curl -s -X PUT "http://localhost:3000/api/contacts/test%2Btag%40sub.example.com/vip" \
       -H "X-Session-Token: {token}" \
       -H "Content-Type: application/json" \
       -d '{"is_vip": true}'
     ```
   - **Expected**: 200 OK, response body contains `email` field with the decoded address `test+tag@sub.example.com` (lowercased)

3. Retrieve VIP score using URL-encoded email
   - **Target**: `GET http://localhost:3000/api/contacts/test%2Btag%40sub.example.com/vip-score`
   - **Input**:
     ```
     curl -s "http://localhost:3000/api/contacts/test%2Btag%40sub.example.com/vip-score" \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, `email` = `"test+tag@sub.example.com"`, `is_manual` = `true`, `vip_score` = `1.0`

4. Verify the VIP list includes the URL-encoded contact
   - **Target**: `GET http://localhost:3000/api/contacts/vip`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/contacts/vip \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: `test+tag@sub.example.com` appears in `vip_contacts` with `is_manual = true`

5. Unset the manual VIP using URL-encoded email (cleanup)
   - **Target**: `PUT http://localhost:3000/api/contacts/test%2Btag%40sub.example.com/vip`
   - **Input**:
     ```
     curl -s -X PUT "http://localhost:3000/api/contacts/test%2Btag%40sub.example.com/vip" \
       -H "X-Session-Token: {token}" \
       -H "Content-Type: application/json" \
       -d '{"is_vip": false}'
     ```
   - **Expected**: 200 OK, `is_vip: false`

## Success Criteria
- [ ] PUT with URL-encoded email (`%40` for `@`, `%2B` for `+`) returns 200 OK
- [ ] The decoded email is stored and returned correctly in response body
- [ ] VIP score lookup via URL-encoded path returns the correct score
- [ ] Contact appears in VIP list with correctly decoded email address

## Failure Criteria
- PUT or GET returns 404 or 400 when using URL-encoded email path
- Email is stored with percent-encoding still intact (e.g., `test%2Btag%40sub.example.com`) instead of decoded
- Server returns 500 due to URL decoding error

## Notes
Axum's `Path` extractor automatically percent-decodes path segments. The `+` character must be encoded as `%2B` in path parameters (not as `+`, which is a space in query strings but not in paths). The `@` must be encoded as `%40`. The handler lowercases the decoded email before DB operations.
