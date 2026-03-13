# GC-540: Analytics Volume with Invalid Period

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: analytics
- **Tags**: analytics, validation, invalid-period, volume
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- None

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Get volume with invalid period
   - **Target**: `GET http://localhost:3030/api/analytics/volume?period=invalid_period`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 400 Bad Request with error indicating invalid period value

3. Get volume with negative days
   - **Target**: `GET http://localhost:3030/api/analytics/volume?period=daily&days=-5`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 400 Bad Request with error indicating invalid days value

4. Get volume with non-numeric days
   - **Target**: `GET http://localhost:3030/api/analytics/volume?period=daily&days=abc`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 400 Bad Request with error indicating invalid days format

## Success Criteria
- [ ] Invalid period returns 400 with descriptive error
- [ ] Negative days returns 400
- [ ] Non-numeric days returns 400

## Failure Criteria
- Any request returns 200 with data
- Server returns 500 instead of 400
- Invalid parameters silently ignored
