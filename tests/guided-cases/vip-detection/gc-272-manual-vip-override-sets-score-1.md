# GC-272: Manual VIP Override Sets Score to 1.0

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: vip-detection
- **Tags**: vip, contacts, manual-override, scoring
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token obtained via `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A contact email to designate as VIP (source: inline — use `manual-vip-test@example.com` which need not exist in messages)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Manually set a contact as VIP
   - **Target**: `PUT http://localhost:3000/api/contacts/manual-vip-test%40example.com/vip`
   - **Input**:
     ```
     curl -s -X PUT "http://localhost:3000/api/contacts/manual-vip-test%40example.com/vip" \
       -H "X-Session-Token: {token}" \
       -H "Content-Type: application/json" \
       -d '{"is_vip": true}'
     ```
   - **Expected**: 200 OK, response body contains `{"email": "manual-vip-test@example.com", "is_vip": true}`

3. Verify VIP score is 1.0 via the score endpoint
   - **Target**: `GET http://localhost:3000/api/contacts/manual-vip-test%40example.com/vip-score`
   - **Input**:
     ```
     curl -s "http://localhost:3000/api/contacts/manual-vip-test%40example.com/vip-score" \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, `vip_score` equals `1.0`, `is_manual` equals `true`, `is_vip` equals `true`

4. Verify contact appears in VIP list
   - **Target**: `GET http://localhost:3000/api/contacts/vip`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/contacts/vip \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: `manual-vip-test@example.com` appears in `vip_contacts` with `is_manual = true` and `vip_score = 1.0`

5. Run compute to confirm manual override survives recomputation
   - **Target**: `POST http://localhost:3000/api/contacts/vip/compute`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3000/api/contacts/vip/compute \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK

6. Re-check score after compute
   - **Target**: `GET http://localhost:3000/api/contacts/manual-vip-test%40example.com/vip-score`
   - **Input**:
     ```
     curl -s "http://localhost:3000/api/contacts/manual-vip-test%40example.com/vip-score" \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: `vip_score` still equals `1.0`, `is_manual` still `true` — manual flag was not overwritten by compute

## Success Criteria
- [ ] PUT returns 200 with `is_vip: true`
- [ ] `vip_score` is exactly `1.0` after setting manual VIP
- [ ] `is_manual` is `true`
- [ ] Contact appears in `GET /api/contacts/vip` list regardless of computed score threshold
- [ ] After running compute, `vip_score` remains `1.0` and `is_manual` remains `true`

## Failure Criteria
- PUT returns non-200 status
- `vip_score` is not 1.0 after manual override
- `is_manual` is false after setting `is_vip: true`
- Compute run resets `is_manual` to false or changes `vip_score` away from 1.0

## Notes
The upsert SQL for set_vip uses `vip_score = 1.0, is_manual = 1`. The compute SQL preserves manual overrides via `vip_score = CASE WHEN vip_contacts.is_manual = 1 THEN 1.0 ELSE excluded.vip_score END`.
