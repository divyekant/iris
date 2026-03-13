# GC-270: List VIP Contacts with Default Threshold

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: vip-detection
- **Tags**: vip, contacts, list, threshold
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token obtained via `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- `POST /api/contacts/vip/compute` has been run at least once so `vip_contacts` table is populated (source: prior step or prior test run)
- At least one contact with `vip_score >= 0.6` exists, or at least one manual VIP exists

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. List VIP contacts with no query parameters (default threshold)
   - **Target**: `GET http://localhost:3000/api/contacts/vip`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/contacts/vip \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, response body is JSON with `vip_contacts` array and `threshold` field

3. Verify default threshold is 0.6
   - **Target**: Response JSON inspection
   - **Input**: Check `threshold` field in response
   - **Expected**: `threshold` equals `0.6`

4. Verify all listed contacts meet the threshold
   - **Target**: Response JSON inspection
   - **Input**: Iterate `vip_contacts` array
   - **Expected**: Each contact has `vip_score >= 0.6` OR `is_manual = true`; no contacts with `vip_score < 0.6` and `is_manual = false` appear in the list

5. Verify response ordering
   - **Target**: Response JSON inspection
   - **Input**: Check `vip_score` values across the array
   - **Expected**: Contacts are ordered by `vip_score` descending (highest score first)

6. Verify default limit is applied
   - **Target**: Response JSON inspection
   - **Input**: Count elements in `vip_contacts`
   - **Expected**: At most 20 contacts returned (default `limit` is 20)

## Success Criteria
- [ ] Response status is 200
- [ ] `threshold` field equals `0.6`
- [ ] All contacts in `vip_contacts` have `vip_score >= 0.6` or `is_manual = true`
- [ ] Results are ordered by `vip_score` descending
- [ ] At most 20 contacts in the response
- [ ] Each contact object contains `email`, `vip_score`, `is_manual`, `message_count`, `reply_count`, `is_vip`

## Failure Criteria
- Non-200 status
- `threshold` field missing or not 0.6
- A contact with `vip_score < 0.6` and `is_manual = false` appears in the list
- More than 20 contacts returned when not specified

## Notes
The list endpoint SQL is `WHERE vip_score >= ?1 OR is_manual = 1 ORDER BY vip_score DESC LIMIT ?2`. Manual VIPs always appear regardless of computed score.
