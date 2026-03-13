# GC-585: Regenerate Existing Profile Overwrites with Fresh Data

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: contact-profiles
- **Tags**: contact-profiles, regenerate, overwrite, idempotency
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- An existing profile for alice@example.com generated yesterday; 10 new messages have since been exchanged with alice

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Record existing profile's `generated_at` timestamp
   - **Target**: `GET http://localhost:3030/api/contacts/profiles/alice@example.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, record `generated_at` value

3. Regenerate the profile
   - **Target**: `POST http://localhost:3030/api/contacts/profiles/generate/alice@example.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, profile returned with updated `generated_at` timestamp

4. Verify the profile was updated (not duplicated)
   - **Target**: `GET http://localhost:3030/api/contacts/profiles`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: Only one profile for alice@example.com in the list (no duplicate); `generated_at` is newer than the recorded timestamp

## Success Criteria
- [ ] Regenerate returns 200 OK
- [ ] `generated_at` is updated to current time
- [ ] No duplicate profiles created
- [ ] Profile content reflects newer messages (summary may differ)

## Failure Criteria
- Duplicate profiles created for same email
- `generated_at` not updated after regeneration
- 5xx error on re-generation of existing profile
