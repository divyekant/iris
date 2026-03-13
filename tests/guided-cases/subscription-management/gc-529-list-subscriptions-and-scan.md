# GC-529: List Subscriptions and Scan Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: subscription-management
- **Tags**: subscriptions, list, scan, happy-path, GET, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one email account configured with synced messages (some of which are subscription emails)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Scan for subscriptions
   - **Target**: `POST http://localhost:3030/api/subscriptions/scan`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON body containing discovered subscriptions (count or array)

3. List all subscriptions
   - **Target**: `GET http://localhost:3030/api/subscriptions`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with array of subscription objects with `id`, `sender`, `status` fields

4. Get subscription stats
   - **Target**: `GET http://localhost:3030/api/subscriptions/stats`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with stats object (total count, active/inactive breakdown)

## Success Criteria
- [ ] POST /api/subscriptions/scan returns 200 with scan results
- [ ] GET /api/subscriptions returns 200 with subscription array
- [ ] Each subscription has `id`, `sender`, `status` fields
- [ ] GET /api/subscriptions/stats returns valid stats

## Failure Criteria
- Scan returns non-200 status
- List returns non-200 or missing required fields
- Stats endpoint returns error
