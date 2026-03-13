# GC-523: Newsletter Feed Endpoints With Invalid Auth Return 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: newsletter-feeds
- **Tags**: newsletter-feeds, auth, invalid-token, 401, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- Invalid token: `invalid-token-12345`

## Steps
1. GET list feeds with invalid token
   - **Target**: `GET http://localhost:3030/api/newsletter-feeds`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

2. POST discover feeds with invalid token
   - **Target**: `POST http://localhost:3030/api/newsletter-feeds/discover`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

3. POST mark-read with invalid token
   - **Target**: `POST http://localhost:3030/api/newsletter-feeds/some-id/mark-read`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] All endpoints return 401 with invalid session token
- [ ] No data leaked in any response

## Failure Criteria
- Any endpoint returns 200 with invalid token
- Response contains feed or article data
