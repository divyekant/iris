# GC-548: Analytics Volume with Different Period Granularities

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: analytics
- **Tags**: analytics, volume, periods, daily, weekly, monthly
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one email account with synced messages spanning multiple days

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Get daily volume (7 days)
   - **Target**: `GET http://localhost:3030/api/analytics/volume?period=daily&days=7`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with up to 7 data points, each with date and count

3. Get weekly volume (30 days)
   - **Target**: `GET http://localhost:3030/api/analytics/volume?period=weekly&days=30`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with data points grouped by week

4. Get daily volume (30 days)
   - **Target**: `GET http://localhost:3030/api/analytics/volume?period=daily&days=30`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with up to 30 data points

5. Verify consistency: sum of daily counts over 7 days should be consistent
   - **Expected**: Sum of daily counts from step 2 should be a reasonable number (non-negative integers)

## Success Criteria
- [ ] All period types return 200 with valid data
- [ ] Daily period returns one entry per day
- [ ] Weekly period returns one entry per week
- [ ] All counts are non-negative integers
- [ ] Data points are chronologically ordered

## Failure Criteria
- Any period returns non-200
- Counts are negative or non-integer
- Data points not in chronological order
- Different periods with overlapping ranges yield wildly inconsistent totals
