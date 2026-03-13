# GC-542: Analytics Endpoints Without Auth Return 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: analytics
- **Tags**: analytics, auth, 401, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- None

## Steps
1. GET overview without session token
   - **Target**: `GET http://localhost:3030/api/analytics/overview`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

2. GET volume without session token
   - **Target**: `GET http://localhost:3030/api/analytics/volume`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

3. GET categories without session token
   - **Target**: `GET http://localhost:3030/api/analytics/categories`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

4. GET top-contacts without session token
   - **Target**: `GET http://localhost:3030/api/analytics/top-contacts`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

5. POST snapshot without session token
   - **Target**: `POST http://localhost:3030/api/analytics/snapshot`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] All five endpoints return 401 without X-Session-Token
- [ ] No analytics data leaked in response bodies

## Failure Criteria
- Any endpoint returns 200 without authentication
- Response body contains analytics data
