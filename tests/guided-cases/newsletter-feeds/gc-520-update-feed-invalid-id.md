# GC-520: Update Feed with Invalid ID

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: newsletter-feeds
- **Tags**: newsletter-feeds, validation, not-found, PUT
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

2. Update nonexistent feed
   - **Target**: `PUT http://localhost:3030/api/newsletter-feeds/nonexistent-feed-999`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"name": "Updated Name"}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] PUT returns 404 for nonexistent feed ID

## Failure Criteria
- Returns 200 or creates a new feed
- Returns 500 Internal Server Error
