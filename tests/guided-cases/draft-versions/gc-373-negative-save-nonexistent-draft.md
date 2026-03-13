# GC-373: Negative — Save Version for Non-Existent Draft

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: draft-versions
- **Tags**: draft, versions, negative, not-found, 404
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Test account (source: local-db, setup: POST /api/accounts)
- A fabricated `draft_id` that does not exist in the database (e.g., `"nonexistent-draft-id-00000000"`)

## Steps

1. Attempt to save a version for a non-existent draft
   - **Target**: `POST /api/drafts/nonexistent-draft-id-00000000/versions`
   - **Input**: `{ "account_id": "{account_id}" }`
   - **Expected**: 404 Not Found with an error body indicating the draft was not found

2. Attempt to list versions for the same non-existent draft
   - **Target**: `GET /api/drafts/nonexistent-draft-id-00000000/versions`
   - **Input**: `draft_id` = `"nonexistent-draft-id-00000000"`
   - **Expected**: 404 Not Found (the draft itself does not exist)

3. Attempt to get a specific version for the non-existent draft
   - **Target**: `GET /api/drafts/nonexistent-draft-id-00000000/versions/1`
   - **Input**: `draft_id` = `"nonexistent-draft-id-00000000"`, `version_number` = `1`
   - **Expected**: 404 Not Found

4. Confirm no orphaned version records were created
   - **Target**: Create a real draft, then list its versions
   - **Input**: `POST /api/drafts` with valid body, then `GET /api/drafts/{real_draft_id}/versions`
   - **Expected**: The new draft's version list is empty — no version records from the failed attempts leaked into the database

## Success Criteria
- [ ] Save attempt for non-existent draft returns 404
- [ ] List attempt for non-existent draft returns 404
- [ ] Get-version attempt for non-existent draft returns 404
- [ ] No version records were created for the non-existent draft ID
- [ ] Error response body contains a message indicating the draft was not found (not a generic 500)

## Failure Criteria
- Any of the three operations returns 200 or 201 for a non-existent draft
- Server returns 500 instead of 404
- Version records appear in the DB associated with the fabricated draft_id
- A real draft's version list is contaminated with versions from the failed save attempt
