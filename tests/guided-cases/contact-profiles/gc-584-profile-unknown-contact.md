# GC-584: Generate Profile for Unknown Contact Returns Empty or Error

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: contact-profiles
- **Tags**: contact-profiles, generate, unknown-contact, negative
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- No messages in the inbox from "stranger@nowhere.example.com"

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Attempt to generate a profile for an unknown contact
   - **Target**: `POST http://localhost:3030/api/contacts/profiles/generate/stranger@nowhere.example.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: Either 404 Not Found (contact not in system) OR 200 OK with minimal/empty profile indicating no history

3. Verify no phantom profile created
   - **Target**: `GET http://localhost:3030/api/contacts/profiles/stranger@nowhere.example.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found (if generate returned 404) or consistent with generate response

## Success Criteria
- [ ] Server does not return 5xx for unknown contact
- [ ] Response clearly indicates no email history exists
- [ ] If a minimal profile is created, it has `message_count: 0` or equivalent

## Failure Criteria
- Server crashes or returns 5xx for unknown contact
- Profile generated with fabricated content for unknown contact
