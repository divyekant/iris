# GC-547: Create Analytics Snapshot and Verify

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: analytics
- **Tags**: analytics, snapshot, create, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one email account with synced messages

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Create analytics snapshot
   - **Target**: `POST http://localhost:3030/api/analytics/snapshot`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK or 201 Created with snapshot object containing `id`, `created_at`, and captured metrics

3. Verify snapshot data matches current overview
   - **Target**: `GET http://localhost:3030/api/analytics/overview`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, metrics should be consistent with snapshot (values may differ slightly due to timing)

## Success Criteria
- [ ] POST /api/analytics/snapshot returns success with snapshot data
- [ ] Snapshot contains `id` and `created_at` fields
- [ ] Snapshot metrics are consistent with current overview

## Failure Criteria
- POST returns error status
- Snapshot missing required fields
- Snapshot data wildly inconsistent with overview
