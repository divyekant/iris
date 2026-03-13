# GC-527: Update Newsletter Feed and Verify

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: newsletter-feeds
- **Tags**: newsletter-feeds, update, PUT, verify
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one newsletter feed exists (run discover first if needed)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Discover feeds to ensure one exists
   - **Target**: `POST http://localhost:3030/api/newsletter-feeds/discover`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with discovered feeds

3. List feeds and pick one
   - **Target**: `GET http://localhost:3030/api/newsletter-feeds`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with at least one feed, note `id`

4. Update the feed name
   - **Target**: `PUT http://localhost:3030/api/newsletter-feeds/{id}`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"name": "Custom Feed Name"}`
   - **Expected**: 200 OK with updated feed showing `name` = `Custom Feed Name`

5. Verify update in list
   - **Target**: `GET http://localhost:3030/api/newsletter-feeds`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, feed with matching `id` has `name` = `Custom Feed Name`

## Success Criteria
- [ ] PUT returns 200 with updated feed
- [ ] Feed name changed to "Custom Feed Name"
- [ ] GET list confirms the update persisted

## Failure Criteria
- PUT returns non-200
- Name not updated in subsequent GET
- Other feed properties unexpectedly changed
