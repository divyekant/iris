# GC-528: Mark All Feed Articles as Read

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: newsletter-feeds
- **Tags**: newsletter-feeds, mark-read, articles, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one newsletter feed with unread articles

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Discover and list feeds
   - **Target**: `POST http://localhost:3030/api/newsletter-feeds/discover`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK

3. Get a feed with articles
   - **Target**: `GET http://localhost:3030/api/newsletter-feeds`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with at least one feed, note `id`

4. List articles before marking read
   - **Target**: `GET http://localhost:3030/api/newsletter-feeds/{id}/articles`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with array of articles

5. Mark all articles as read
   - **Target**: `POST http://localhost:3030/api/newsletter-feeds/{id}/mark-read`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with confirmation (e.g., `{"updated": N}` where N >= 0)

6. Verify articles marked as read
   - **Target**: `GET http://localhost:3030/api/newsletter-feeds/{id}/articles`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, all articles show read status

## Success Criteria
- [ ] POST mark-read returns 200 with count of updated articles
- [ ] Articles show as read after mark-read operation
- [ ] Idempotent: calling mark-read again succeeds (returns 0 updated)

## Failure Criteria
- mark-read returns error status
- Articles still show as unread after operation
