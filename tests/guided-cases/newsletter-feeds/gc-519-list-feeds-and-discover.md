# GC-519: List Newsletter Feeds and Discover Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: newsletter-feeds
- **Tags**: newsletter-feeds, list, discover, happy-path, GET, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one email account configured with synced messages (some of which are newsletters)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Discover newsletter feeds from inbox
   - **Target**: `POST http://localhost:3030/api/newsletter-feeds/discover`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON body containing discovered feeds (array of feed objects with `id`, `sender`, `name` fields)

3. List all newsletter feeds
   - **Target**: `GET http://localhost:3030/api/newsletter-feeds`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with array of feed objects matching or including discovered feeds

## Success Criteria
- [ ] POST /api/newsletter-feeds/discover returns 200 with feed results
- [ ] Each feed has `id`, `sender`, and `name` fields
- [ ] GET /api/newsletter-feeds returns 200 with array of feeds
- [ ] Discovered feeds appear in the list

## Failure Criteria
- Discover returns non-200 status
- Feed objects missing required fields
- List endpoint doesn't include discovered feeds
