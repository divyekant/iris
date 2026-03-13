# GC-582: Delete Contact Profile Removes It from Storage

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: contact-profiles
- **Tags**: contact-profiles, delete, DELETE, lifecycle
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- An existing profile for alice@example.com

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Verify profile exists before deletion
   - **Target**: `GET http://localhost:3030/api/contacts/profiles/alice@example.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK

3. Delete the profile
   - **Target**: `DELETE http://localhost:3030/api/contacts/profiles/alice@example.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK or 204 No Content

4. Verify profile is gone
   - **Target**: `GET http://localhost:3030/api/contacts/profiles/alice@example.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

5. Confirm it does not appear in list
   - **Target**: `GET http://localhost:3030/api/contacts/profiles`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: alice@example.com absent from `profiles` array

## Success Criteria
- [ ] DELETE returns 200 or 204
- [ ] GET after DELETE returns 404
- [ ] Profile not present in list after deletion
- [ ] Underlying email messages are NOT deleted (profile only)

## Failure Criteria
- Profile still retrievable after DELETE
- DELETE returns error for existing profile
