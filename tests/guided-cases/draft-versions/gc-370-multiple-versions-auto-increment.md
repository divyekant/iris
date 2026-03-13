# GC-370: Multiple Versions — Auto-Incrementing Numbers and List Order

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: draft-versions
- **Tags**: draft, versions, auto-increment, ordering, list
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Test account (source: local-db, setup: POST /api/accounts)
- Test draft with initial body content (source: local-db, setup: POST /api/drafts)

## Steps

1. Save the first version of the draft
   - **Target**: `POST /api/drafts/{draft_id}/versions`
   - **Input**: `{ "account_id": "{account_id}" }`
   - **Expected**: 201 with `version_number` = `1`

2. Update the draft body content
   - **Target**: `POST /api/drafts` (update existing draft)
   - **Input**: `{ "id": "{draft_id}", "body": "Updated body for version 2.", "account_id": "{account_id}" }`
   - **Expected**: 200; if auto-versioning triggers, a new version is created automatically

3. Save a second explicit version
   - **Target**: `POST /api/drafts/{draft_id}/versions`
   - **Input**: `{ "account_id": "{account_id}" }`
   - **Expected**: 201 with `version_number` = `2`

4. Update the draft body again
   - **Target**: `POST /api/drafts` (update existing draft)
   - **Input**: `{ "id": "{draft_id}", "body": "Final body for version 3.", "account_id": "{account_id}" }`
   - **Expected**: 200

5. Save a third explicit version
   - **Target**: `POST /api/drafts/{draft_id}/versions`
   - **Input**: `{ "account_id": "{account_id}" }`
   - **Expected**: 201 with `version_number` = `3`

6. List all versions
   - **Target**: `GET /api/drafts/{draft_id}/versions`
   - **Input**: valid `draft_id`
   - **Expected**: 200 with an array of at least 3 entries; entries are ordered with the latest version first (descending) or in ascending order by `version_number`; `version_number` values are strictly increasing (1, 2, 3, ...)

7. Verify each version number is unique
   - **Target**: Inspect the list response from step 6
   - **Input**: the versions array
   - **Expected**: No duplicate `version_number` values; `version_number` increments by 1 each time

## Success Criteria
- [ ] First save returns `version_number` = `1`
- [ ] Second save returns `version_number` = `2`
- [ ] Third save returns `version_number` = `3`
- [ ] List returns at least 3 versions with unique, strictly increasing `version_number` values
- [ ] List ordering is consistent (all ascending or all descending — not mixed)
- [ ] No duplicate `version_number` values in the list

## Failure Criteria
- Any save returns a `version_number` that is not strictly greater than the previous one
- List contains duplicate `version_number` values
- List ordering is inconsistent (some ascending, some descending)
- Any request returns a 500 error
