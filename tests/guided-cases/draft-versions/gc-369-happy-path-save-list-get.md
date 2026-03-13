# GC-369: Happy Path — Save Version, List, Get Detail, Verify Fields

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: draft-versions
- **Tags**: draft, versions, happy-path, save, list, get
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Test account (source: local-db, setup: POST /api/accounts)
- Test draft with body content (source: local-db, setup: POST /api/drafts with `subject`, `body`, `to`)

## Steps

1. Save a version snapshot of the draft
   - **Target**: `POST /api/drafts/{draft_id}/versions`
   - **Input**: `{ "account_id": "{account_id}" }`
   - **Expected**: 201 with a version object containing `version_number`, `draft_id`, `created_at`, and a summary of content (subject/body snapshot or diff metadata)

2. List all versions for the draft
   - **Target**: `GET /api/drafts/{draft_id}/versions`
   - **Input**: valid `draft_id` in path, `X-Session-Token` header
   - **Expected**: 200 with an array containing exactly 1 version entry; each entry has `version_number`, `draft_id`, `created_at` but does NOT include the full body

3. Get the specific version detail
   - **Target**: `GET /api/drafts/{draft_id}/versions/1`
   - **Input**: `draft_id` in path, `version_number` = `1`
   - **Expected**: 200 with the full version object including `version_number` = `1`, `draft_id` matching the draft, `subject`, `body`, `to`, and `created_at`

4. Verify field consistency between list and detail
   - **Target**: Compare responses from steps 2 and 3
   - **Input**: `version_number`, `draft_id`, `created_at` from both responses
   - **Expected**: `version_number`, `draft_id`, and `created_at` match exactly between list entry and detail response

## Success Criteria
- [ ] POST returns 201 with version metadata
- [ ] GET list returns 200 with exactly 1 entry and no full body in list items
- [ ] GET detail returns 200 with `version_number` = `1` and full `body` field populated
- [ ] `draft_id` in detail response matches the draft used in step 1
- [ ] `created_at` is a valid ISO 8601 timestamp
- [ ] `version_number`, `draft_id`, `created_at` are consistent between list and detail

## Failure Criteria
- Save returns anything other than 201
- List returns 404 or empty array after a successful save
- Detail response is missing `body`, `subject`, or `version_number`
- Field values differ between list metadata and detail response
- Any step returns a 500 error
