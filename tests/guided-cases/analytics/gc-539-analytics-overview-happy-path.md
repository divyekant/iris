# GC-539: Analytics Overview Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: analytics
- **Tags**: analytics, overview, happy-path, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one email account configured with synced messages

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Get analytics overview
   - **Target**: `GET http://localhost:3030/api/analytics/overview`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON body containing overview metrics (total messages, unread count, categories breakdown, response rate)

3. Get volume analytics
   - **Target**: `GET http://localhost:3030/api/analytics/volume?period=daily&days=7`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with array of volume data points (each with `date`/`period` and `count` fields)

4. Get category analytics
   - **Target**: `GET http://localhost:3030/api/analytics/categories`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with category breakdown (category names with counts)

5. Get top contacts
   - **Target**: `GET http://localhost:3030/api/analytics/top-contacts`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with array of contact objects (email, message count)

6. Get hourly distribution
   - **Target**: `GET http://localhost:3030/api/analytics/hourly-distribution`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with 24-element array or object mapping hours (0-23) to counts

7. Get response times
   - **Target**: `GET http://localhost:3030/api/analytics/response-times`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with response time metrics (average, median, distribution)

## Success Criteria
- [ ] All seven analytics endpoints return 200
- [ ] Overview contains meaningful metric fields
- [ ] Volume data has date/count structure
- [ ] Categories has name/count structure
- [ ] Top contacts lists contacts with counts
- [ ] Hourly distribution covers 24 hours
- [ ] Response times contain timing metrics

## Failure Criteria
- Any endpoint returns non-200 status
- Response bodies missing expected metric fields
- Data structures inconsistent with expected format
