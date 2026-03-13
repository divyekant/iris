# GC-581: Search Contact Profiles by Query Term

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: contact-profiles
- **Tags**: contact-profiles, search, GET, query
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Profiles exist for alice@example.com (topics: engineering, deployment) and bob@sales.com (topics: sales, revenue)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Search profiles with a topic-specific query
   - **Target**: `GET http://localhost:3030/api/contacts/profiles/search?q=engineering`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, results include alice@example.com but not bob@sales.com

3. Search by partial email address
   - **Target**: `GET http://localhost:3030/api/contacts/profiles/search?q=alice`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, results include alice@example.com

## Success Criteria
- [ ] Topic-based search returns relevant profiles
- [ ] Partial email search works correctly
- [ ] Non-matching contact is excluded from results
- [ ] Results include profile fields, not just email addresses

## Failure Criteria
- Search returns all profiles regardless of query
- Partial email search returns no results
- Search returns profiles from other accounts
