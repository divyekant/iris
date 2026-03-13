# GC-274: Custom Threshold Filters VIP Contacts

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: vip-detection
- **Tags**: vip, contacts, threshold, filtering, query-params
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token obtained via `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- `POST /api/contacts/vip/compute` has been run and there are contacts with varying scores in `vip_contacts`
- At least one contact with `vip_score >= 0.9` and at least one with `0.5 <= vip_score < 0.9` (for differentiation)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. List with high threshold (0.9) — should return fewer contacts
   - **Target**: `GET http://localhost:3000/api/contacts/vip?threshold=0.9`
   - **Input**:
     ```
     curl -s "http://localhost:3000/api/contacts/vip?threshold=0.9" \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, `threshold` field is `0.9`, only contacts with `vip_score >= 0.9` or `is_manual = true` appear

3. List with low threshold (0.3) — should return more contacts
   - **Target**: `GET http://localhost:3000/api/contacts/vip?threshold=0.3`
   - **Input**:
     ```
     curl -s "http://localhost:3000/api/contacts/vip?threshold=0.3" \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, `threshold` field is `0.3`, contacts with `vip_score >= 0.3` or `is_manual = true` appear; count >= count from step 2

4. List with custom limit (5)
   - **Target**: `GET http://localhost:3000/api/contacts/vip?limit=5`
   - **Input**:
     ```
     curl -s "http://localhost:3000/api/contacts/vip?limit=5" \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, at most 5 contacts in `vip_contacts`

5. List with both threshold and limit
   - **Target**: `GET http://localhost:3000/api/contacts/vip?threshold=0.5&limit=3`
   - **Input**:
     ```
     curl -s "http://localhost:3000/api/contacts/vip?threshold=0.5&limit=3" \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, `threshold` is `0.5`, at most 3 contacts returned, all with `vip_score >= 0.5` or `is_manual = true`

6. Verify threshold=0.9 count is <= threshold=0.3 count (among non-manual contacts)
   - **Target**: Cross-reference counts from steps 2 and 3
   - **Expected**: High-threshold result count (step 2) <= low-threshold result count (step 3)

## Success Criteria
- [ ] `threshold` field in response matches the query parameter value sent
- [ ] With `threshold=0.9`, no non-manual contact with `vip_score < 0.9` appears
- [ ] With `threshold=0.3`, result set is a superset of `threshold=0.9` results (among non-manual contacts)
- [ ] With `limit=5`, at most 5 contacts returned
- [ ] Combined `threshold` and `limit` work together correctly

## Failure Criteria
- `threshold` field in response does not match query parameter
- Contacts below the custom threshold appear in results (excluding manual VIPs)
- `limit` parameter is ignored and more contacts are returned than requested
- Non-200 status

## Notes
The server caps `limit` at 200 (`params.limit.unwrap_or(20).min(200)`). The `threshold` parameter defaults to 0.6 if not provided. Manual VIPs (`is_manual = 1`) always appear regardless of their computed score vs. the threshold.
