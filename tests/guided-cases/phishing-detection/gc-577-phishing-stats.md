# GC-577: Phishing Stats Endpoint Returns Accurate Aggregate Counts

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: phishing-detection
- **Tags**: phishing, stats, aggregate, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Several messages scanned with known results: 5 clean, 2 medium-risk, 1 high-risk

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Retrieve phishing stats
   - **Target**: `GET http://localhost:3030/api/security/phishing-stats`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response includes `total_scanned`, `by_risk_level` breakdown, and `is_phishing_count`

3. Verify counts match known data
   - **Expected**: `total_scanned` ≥ 8, `by_risk_level.none` ≥ 5, `by_risk_level.medium` ≥ 2, `by_risk_level.high` ≥ 1

## Success Criteria
- [ ] Stats endpoint returns 200 OK
- [ ] `total_scanned` reflects all scanned messages
- [ ] `by_risk_level` breakdown sums to `total_scanned`
- [ ] Stats data is consistent with individual scan results

## Failure Criteria
- Stats counts do not match actual scan data
- `by_risk_level` values don't sum to `total_scanned`
- Response missing required aggregate fields
