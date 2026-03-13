# GC-524: Newsletter Feeds List Empty State

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: newsletter-feeds
- **Tags**: newsletter-feeds, empty-state, list, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- No newsletter feeds have been discovered (clean state or all deleted)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. List feeds when none exist
   - **Target**: `GET http://localhost:3030/api/newsletter-feeds`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with empty array `[]`

## Success Criteria
- [ ] GET /api/newsletter-feeds returns 200 with empty array
- [ ] Response is valid JSON

## Failure Criteria
- Returns non-200 status code
- Returns null instead of empty array
- Returns 500 error
