# GC-377: Edge — List Versions for Draft with No Versions

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: draft-versions
- **Tags**: draft, versions, edge, empty-state, list
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Test account (source: local-db, setup: POST /api/accounts)
- Test draft that has never had a version saved and was created with body content that did NOT trigger auto-versioning (source: local-db, setup: POST /api/drafts with minimal body)

## Steps

1. Create a fresh draft without saving any explicit versions
   - **Target**: `POST /api/drafts`
   - **Input**: `{ "account_id": "{account_id}", "subject": "No versions yet", "body": "Initial draft body.", "to": ["test@example.com"] }`
   - **Expected**: 201 with a new `draft_id`; no versions should be auto-created for the initial save (or if auto-versioning fires, note it — the list should reflect reality)

2. List versions for the newly created draft
   - **Target**: `GET /api/drafts/{draft_id}/versions`
   - **Input**: valid `draft_id`
   - **Expected**: 200 with an empty array `[]` (or `{ "versions": [] }`) — the endpoint returns a 200 with an empty collection, not a 404

3. Verify response structure is correct for the empty state
   - **Target**: Inspect the response from step 2
   - **Input**: the versions list response
   - **Expected**: Response is a valid JSON array (or object containing an array key) that is empty; `Content-Type` is `application/json`

4. Confirm the endpoint behaves the same for a draft that had versions, then had them purged (if purge is supported)
   - **Target**: If a delete-version endpoint exists, create + save + delete the version, then list
   - **Input**: standard CRUD sequence ending in deletion
   - **Expected**: 200 with empty array — the empty state is structurally identical whether versions were never created or all deleted

## Success Criteria
- [ ] List returns 200 (not 404) for a draft with no saved versions
- [ ] Response body is a valid JSON empty array or empty-array-valued object
- [ ] `Content-Type: application/json` is present on the response
- [ ] If auto-versioning fires on initial save, the list reflects exactly those auto-created versions (not zero when one exists)

## Failure Criteria
- List returns 404 when the draft exists but has no versions
- List returns null instead of an empty array
- List returns 200 with malformed JSON
- Response contains stale or phantom version entries for the new draft
