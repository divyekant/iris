# GC-275: Get VIP Score for Unknown Contact Returns Zero Score

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: vip-detection
- **Tags**: vip, contacts, unknown-contact, zero-score
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token obtained via `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A fabricated contact email that has never appeared in messages and does not exist in `vip_contacts`: `nobody-unknown-xyz@notareal.example` (source: inline)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Request VIP score for a non-existent contact
   - **Target**: `GET http://localhost:3000/api/contacts/nobody-unknown-xyz%40notareal.example/vip-score`
   - **Input**:
     ```
     curl -s -w "\n%{http_code}" \
       "http://localhost:3000/api/contacts/nobody-unknown-xyz%40notareal.example/vip-score" \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK (NOT 404), response body is a zero-score VipScoreResponse

3. Verify zero-score response shape
   - **Target**: Response JSON inspection
   - **Input**: Parse the JSON body
   - **Expected**:
     - `email` = `"nobody-unknown-xyz@notareal.example"` (lowercased)
     - `vip_score` = `0.0`
     - `is_manual` = `false`
     - `is_vip` = `false`
     - `stats.message_count` = `0`
     - `stats.reply_count` = `0`
     - `stats.last_contact` = `null`
     - `stats.first_contact` = `null`
     - `stats.avg_reply_time_secs` = `null`

## Success Criteria
- [ ] Response status is 200 (endpoint returns a zero-score object, not 404)
- [ ] `vip_score` is exactly `0.0`
- [ ] `is_manual` is `false`
- [ ] `is_vip` is `false`
- [ ] All `stats` numeric fields are `0` and timestamp fields are `null`
- [ ] `email` field matches the queried address lowercased

## Failure Criteria
- Response status is 404 (the endpoint should return a zero-score stub, not 404)
- Response status is 500
- `vip_score` is non-zero for a contact that has never appeared in messages
- `is_vip` is true for a zero-score, non-manual contact

## Notes
The `get_vip_score` handler explicitly handles `rusqlite::Error::QueryReturnedNoRows` by returning a synthetic zero-score response rather than a 404. This is the intended behavior — the endpoint is a "lookup or default" not a strict existence check. This distinguishes it from typical 404-on-missing REST endpoints.
