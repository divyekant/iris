# GC-232: Contact Topics Empty — No Messages Found for Contact

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: contact-topics
- **Tags**: topics, empty, no-messages
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured (state does not matter for this case)

### Data
- No messages from `unknown-person@nowhere.test` in any synced account (source: verify via search)

## Steps
1. Request topics for a contact with no messages
   - **Target**: `GET /api/contacts/unknown-person@nowhere.test/topics`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with empty topics

2. Validate response structure
   - **Target**: Response body
   - **Input**: Parse JSON
   - **Expected**: `email` equals `unknown-person@nowhere.test`, `topics` is empty array `[]`, `total_emails` is 0

## Success Criteria
- [ ] Response status is 200 (not 404)
- [ ] `email` field matches requested contact
- [ ] `topics` array is empty `[]`
- [ ] `total_emails` is 0
- [ ] No AI call is triggered (nothing to analyze)

## Failure Criteria
- Response status is 404 or 500
- `topics` array contains entries when no messages exist
- AI provider is called unnecessarily

## Notes
The endpoint returns 200 with empty topics rather than 404 when no messages are found. This is by design — the contact may exist but have no analyzable email history. The UI renders an empty state for this case.
