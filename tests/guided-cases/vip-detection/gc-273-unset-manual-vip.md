# GC-273: Unset Manual VIP Resets Score to 0.0

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: vip-detection
- **Tags**: vip, contacts, manual-override, toggle
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token obtained via `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Contact `unset-vip-test@example.com` already set as manual VIP (run `PUT /api/contacts/unset-vip-test%40example.com/vip` with `{"is_vip": true}` before this test, or rely on GC-272 ordering)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Ensure the contact is currently a manual VIP
   - **Target**: `PUT http://localhost:3000/api/contacts/unset-vip-test%40example.com/vip`
   - **Input**:
     ```
     curl -s -X PUT "http://localhost:3000/api/contacts/unset-vip-test%40example.com/vip" \
       -H "X-Session-Token: {token}" \
       -H "Content-Type: application/json" \
       -d '{"is_vip": true}'
     ```
   - **Expected**: 200 OK, `is_vip: true`

3. Unset the manual VIP flag
   - **Target**: `PUT http://localhost:3000/api/contacts/unset-vip-test%40example.com/vip`
   - **Input**:
     ```
     curl -s -X PUT "http://localhost:3000/api/contacts/unset-vip-test%40example.com/vip" \
       -H "X-Session-Token: {token}" \
       -H "Content-Type: application/json" \
       -d '{"is_vip": false}'
     ```
   - **Expected**: 200 OK, response body contains `{"email": "unset-vip-test@example.com", "is_vip": false}`

4. Verify score is reset to 0.0 and is_manual is false
   - **Target**: `GET http://localhost:3000/api/contacts/unset-vip-test%40example.com/vip-score`
   - **Input**:
     ```
     curl -s "http://localhost:3000/api/contacts/unset-vip-test%40example.com/vip-score" \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, `vip_score` equals `0.0`, `is_manual` equals `false`, `is_vip` equals `false`

5. Verify contact no longer appears in VIP list at default threshold
   - **Target**: `GET http://localhost:3000/api/contacts/vip`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/contacts/vip \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: `unset-vip-test@example.com` does NOT appear in `vip_contacts` (score 0.0 < threshold 0.6, is_manual = false)

## Success Criteria
- [ ] PUT with `is_vip: false` returns 200 with `is_vip: false`
- [ ] `vip_score` is reset to `0.0` in the vip-score endpoint
- [ ] `is_manual` is `false` after unsetting
- [ ] Contact no longer appears in the default VIP list (score 0.0 does not meet 0.6 threshold)

## Failure Criteria
- PUT with `is_vip: false` returns non-200 status
- `is_manual` remains `true` after unsetting
- `vip_score` remains `1.0` after unsetting
- Contact still appears in the VIP list with `is_manual = false` and `vip_score = 0.0`

## Notes
The unset path runs `UPDATE vip_contacts SET is_manual = 0, vip_score = 0.0 WHERE email = ?1`. The contact row is kept in the table but will score 0.0 until the next compute run updates it from real message data.
