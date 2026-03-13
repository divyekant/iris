# GC-583: Generate-All Bulk Creates Profiles for All Known Contacts

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: contact-profiles
- **Tags**: contact-profiles, generate-all, bulk, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- An account with messages from at least 3 distinct contacts who don't yet have profiles

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Count existing profiles before bulk generate
   - **Target**: `GET http://localhost:3030/api/contacts/profiles`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, record baseline `profiles.length`

3. Trigger generate-all
   - **Target**: `POST http://localhost:3030/api/contacts/profiles/generate-all`
   - **Input**: Header `X-Session-Token: {token}`, body `{"account_id": "{account_id}"}`
   - **Expected**: 200 OK or 202 Accepted, response includes `profiles_generated` count

4. Verify profiles count increased
   - **Target**: `GET http://localhost:3030/api/contacts/profiles`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `profiles.length` > baseline (at least 3 new profiles)

## Success Criteria
- [ ] generate-all returns success (200 or 202)
- [ ] `profiles_generated` count is non-zero
- [ ] Profile list count increases after generate-all
- [ ] Each new profile has required fields

## Failure Criteria
- generate-all returns error with existing contacts
- Profile count unchanged after generate-all
- generate-all takes > 30 seconds for a small inbox (performance issue)
