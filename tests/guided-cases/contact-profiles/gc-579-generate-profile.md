# GC-579: Generate Contact Profile for Known Contact

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: contact-profiles
- **Tags**: contact-profiles, generate, POST, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least 5 messages exchanged with a specific contact (e.g., alice@example.com) to provide sufficient history for profile generation

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Generate a profile for the known contact
   - **Target**: `POST http://localhost:3030/api/contacts/profiles/generate/alice@example.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK or 201 Created, response includes profile object with `email`, `summary`, `topics`, `communication_style`, and `generated_at` fields

3. Retrieve the generated profile
   - **Target**: `GET http://localhost:3030/api/contacts/profiles/alice@example.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, profile data matches what was generated

## Success Criteria
- [ ] Generate returns 200 or 201 with profile data
- [ ] Profile includes `email`, `summary`, `generated_at` fields
- [ ] Profile is retrievable via GET after generation
- [ ] `topics` array reflects actual email exchange content

## Failure Criteria
- Generate returns error for a contact with email history
- Profile missing required fields
- GET returns 404 immediately after successful generate
