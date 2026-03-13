# GC-378: Edge — Diff Same Version Against Itself

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: draft-versions
- **Tags**: draft, versions, edge, diff, no-changes, idempotent
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Test account (source: local-db, setup: POST /api/accounts)
- Test draft with at least 1 saved version (source: local-db, setup: POST /api/drafts then POST /api/drafts/{draft_id}/versions)

## Steps

1. Save a version of the draft
   - **Target**: `POST /api/drafts/{draft_id}/versions`
   - **Input**: `{ "account_id": "{account_id}" }`
   - **Expected**: 201 with `version_number` = `1`

2. Diff version 1 against itself
   - **Target**: `GET /api/drafts/{draft_id}/versions/diff?from=1&to=1`
   - **Input**: `from=1`, `to=1`
   - **Expected**: 200; diff response indicates zero additions, zero removals, and zero word/character count change — the content is identical

3. Verify the diff response structure for the no-change case
   - **Target**: Inspect the response from step 2
   - **Input**: additions, removals, and any change summary fields
   - **Expected**: Additions list is empty (or absent); removals list is empty (or absent); any `changed` or `has_changes` boolean field is `false`; word count delta is `0`

4. Confirm the same behavior with version 2 diffed against itself (if 2 versions exist)
   - **Target**: Save a second version with different content, then `GET /api/drafts/{draft_id}/versions/diff?from=2&to=2`
   - **Input**: update draft body, save v2, then diff v2 vs v2
   - **Expected**: 200; additions and removals are both empty despite v2 having different content from v1

5. Verify that a legitimate diff between v1 and v2 still shows changes
   - **Target**: `GET /api/drafts/{draft_id}/versions/diff?from=1&to=2`
   - **Input**: `from=1`, `to=2` (versions with different content)
   - **Expected**: 200; non-empty additions or removals confirming the diff engine is working correctly and the no-change result for same-version was accurate

## Success Criteria
- [ ] Diff of a version against itself returns 200 (not 400 or 500)
- [ ] Additions list is empty for same-version diff
- [ ] Removals list is empty for same-version diff
- [ ] Word/character count delta is 0 for same-version diff
- [ ] Any `has_changes` or `changed` boolean is `false` (if present)
- [ ] A legitimate diff between two different versions still shows non-zero changes

## Failure Criteria
- Same-version diff returns 400 (rejecting `from` == `to` as invalid)
- Same-version diff returns 500
- Same-version diff returns non-empty additions or removals
- The no-change diff corrupts the diff engine so subsequent legitimate diffs are broken
