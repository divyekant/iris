# GC-588: Profile for Contact with Minimal History Is Generated with Low Confidence

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: contact-profiles
- **Tags**: contact-profiles, minimal-history, confidence, edge
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Exactly 1 email message exchanged with newcontact@example.com

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Generate profile for contact with only 1 message
   - **Target**: `POST http://localhost:3030/api/contacts/profiles/generate/newcontact@example.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, profile generated with minimal data; `confidence` field low or `message_count: 1`; summary indicates limited history

3. Verify profile does not over-extrapolate
   - **Expected**: `topics` array has 0-2 entries; `summary` mentions limited history; no fabricated details about communication patterns

## Success Criteria
- [ ] Profile generated without error for single-message contact
- [ ] Profile indicates low confidence or limited history
- [ ] `message_count` or equivalent reflects the actual single message
- [ ] Profile content is conservative and does not over-extrapolate

## Failure Criteria
- Server errors on single-message contact
- Profile contains detailed fabricated content for one-message history
- `topics` array has many entries for a single email
