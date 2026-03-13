# GC-271: Get VIP Score for Specific Contact

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: vip-detection
- **Tags**: vip, contacts, scoring, per-contact
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token obtained via `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- `POST /api/contacts/vip/compute` has been run so that contact scores are stored in `vip_contacts`
- A known contact email that exists in the `vip_contacts` table (e.g., a sender who has emailed the user — retrieve from `GET /api/contacts/vip` first)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. List VIP contacts to find a known scored contact
   - **Target**: `GET http://localhost:3000/api/contacts/vip`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/contacts/vip \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, at least one contact in `vip_contacts`; record the `email` of the first contact

3. Fetch VIP score for that specific contact
   - **Target**: `GET http://localhost:3000/api/contacts/{email}/vip-score`
   - **Input**:
     ```
     curl -s "http://localhost:3000/api/contacts/contact%40example.com/vip-score" \
       -H "X-Session-Token: {token}"
     ```
     (replace `contact%40example.com` with the URL-encoded email from step 2)
   - **Expected**: 200 OK, response body contains `email`, `vip_score`, `is_manual`, `is_vip`, and `stats` object

4. Verify response fields
   - **Target**: Response JSON inspection
   - **Input**: Check all fields
   - **Expected**:
     - `email` matches the queried address (lowercased)
     - `vip_score` is a float between 0.0 and 1.0
     - `is_manual` is a boolean
     - `is_vip` is `true` if `vip_score >= 0.6` or `is_manual = true`, otherwise `false`
     - `stats` object contains `message_count`, `reply_count`, `last_contact`, `first_contact`, `avg_reply_time_secs`

5. Verify score consistency with list endpoint
   - **Target**: Cross-reference
   - **Input**: Compare `vip_score` from step 3 against the `vip_score` for the same contact in the list from step 2
   - **Expected**: Scores match

## Success Criteria
- [ ] Response status is 200
- [ ] `email` field matches the queried contact (lowercased)
- [ ] `vip_score` is between 0.0 and 1.0 inclusive
- [ ] `is_vip` correctly reflects `vip_score >= 0.6 OR is_manual`
- [ ] `stats.message_count` is a non-negative integer
- [ ] `stats.reply_count` is a non-negative integer <= `stats.message_count`
- [ ] Score value matches what is returned by the list endpoint for the same contact

## Failure Criteria
- Non-200 status for a contact that exists in vip_contacts
- `vip_score` outside [0.0, 1.0] range
- `is_vip` does not agree with the threshold logic
- `stats` object missing or contains null for `message_count` or `reply_count`
