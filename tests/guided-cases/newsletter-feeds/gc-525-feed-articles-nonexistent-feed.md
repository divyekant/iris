# GC-525: Get Articles for Nonexistent Feed

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: newsletter-feeds
- **Tags**: newsletter-feeds, articles, not-found, 404
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Non-existent feed ID: `nonexistent-feed-999`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. GET articles for nonexistent feed
   - **Target**: `GET http://localhost:3030/api/newsletter-feeds/nonexistent-feed-999/articles`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

3. POST mark-read for nonexistent feed
   - **Target**: `POST http://localhost:3030/api/newsletter-feeds/nonexistent-feed-999/mark-read`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] GET articles returns 404 for nonexistent feed
- [ ] POST mark-read returns 404 for nonexistent feed

## Failure Criteria
- Returns 200 with empty array (should be 404 since feed doesn't exist)
- Returns 500 Internal Server Error
