# GC-522: Newsletter Feed Endpoints Without Auth Return 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: newsletter-feeds
- **Tags**: newsletter-feeds, auth, 401, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- None

## Steps
1. GET list feeds without session token
   - **Target**: `GET http://localhost:3030/api/newsletter-feeds`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

2. POST discover feeds without session token
   - **Target**: `POST http://localhost:3030/api/newsletter-feeds/discover`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

3. PUT update feed without session token
   - **Target**: `PUT http://localhost:3030/api/newsletter-feeds/some-id`
   - **Input**: Header `Content-Type: application/json`, Body `{"name": "test"}`
   - **Expected**: 401 Unauthorized

4. DELETE feed without session token
   - **Target**: `DELETE http://localhost:3030/api/newsletter-feeds/some-id`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

5. GET articles without session token
   - **Target**: `GET http://localhost:3030/api/newsletter-feeds/some-id/articles`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] All five endpoints return 401 without X-Session-Token
- [ ] No feed data leaked in response bodies

## Failure Criteria
- Any endpoint returns 200 without authentication
- Response body contains feed or article data
