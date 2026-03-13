# GC-234: Top Contacts List Endpoint

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: contact-topics
- **Tags**: contacts, top-contacts, list
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least one synced account with messages from multiple contacts (source: IMAP sync)
- Minimum 3 distinct sender/recipient email addresses in the database

## Steps
1. Request top contacts list
   - **Target**: `GET /api/contacts/top`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON body containing `contacts` array

2. Validate response structure
   - **Target**: Response body
   - **Input**: Parse JSON
   - **Expected**: `contacts` is a non-empty array of ContactSummary objects

3. Validate each contact entry
   - **Target**: Each entry in `contacts` array
   - **Input**: Iterate entries
   - **Expected**: Each has `email` (string with @), `email_count` (> 0), optional `name` (string or null), optional `last_contact` (ISO timestamp or null)

4. Verify ordering
   - **Target**: `contacts` array order
   - **Input**: Compare `email_count` values
   - **Expected**: Contacts are sorted by `email_count` descending (most frequent first)

## Success Criteria
- [ ] Response status is 200
- [ ] `contacts` array is non-empty
- [ ] Each contact has a valid `email` containing @
- [ ] Each contact has `email_count` > 0
- [ ] `name` is either a string or null
- [ ] `last_contact` is either a valid ISO timestamp or null
- [ ] Contacts are ordered by email_count descending

## Failure Criteria
- Response status is not 200
- `contacts` array is empty when messages exist
- Contact entries missing required fields
- Contacts not sorted by frequency

## Notes
The top contacts endpoint provides the list used to populate the ContactTopicsPanel contact selector. It aggregates across all synced accounts. The `name` field is extracted from the From header display name and may be null if only a bare email address was used.
