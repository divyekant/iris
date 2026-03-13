# GC-587: Search Profiles with No Matching Results Returns Empty Array

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: contact-profiles
- **Tags**: contact-profiles, search, empty-results, negative
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Existing profiles for contacts with topics like "engineering" and "sales" — no profile contains "underwater basket weaving"

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Search for a term that matches no profiles
   - **Target**: `GET http://localhost:3030/api/contacts/profiles/search?q=underwater+basket+weaving`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `profiles` array is empty (not 404, not error)

3. Search with empty query string
   - **Target**: `GET http://localhost:3030/api/contacts/profiles/search?q=`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: Either 400 Bad Request (query required) OR 200 OK with all profiles (implementation-defined)

## Success Criteria
- [ ] No-match search returns 200 OK with empty array
- [ ] Empty array response is not a 404 or 5xx
- [ ] `total` field (if present) is 0 for no-match case

## Failure Criteria
- Server returns 404 for empty search results
- Server returns 5xx for a valid but non-matching search term
