# GC-580: List Contact Profiles Returns All Generated Profiles

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: contact-profiles
- **Tags**: contact-profiles, list, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least 2 contact profiles previously generated (e.g., alice@example.com and bob@example.com)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. List all contact profiles
   - **Target**: `GET http://localhost:3030/api/contacts/profiles`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `profiles` array with ≥ 2 entries

3. Validate structure of a profile entry
   - Each profile should include: `email`, `summary` (string), `topics` (array), `generated_at` (timestamp)
   - **Expected**: All required fields present and correctly typed

## Success Criteria
- [ ] List returns 200 OK
- [ ] `profiles` array contains all generated profiles
- [ ] Each entry has at minimum `email` and `generated_at`
- [ ] List does not return duplicate profiles for the same email

## Failure Criteria
- `profiles` array empty despite generated profiles existing
- Missing required fields in list entries
- Duplicate profiles for same email
