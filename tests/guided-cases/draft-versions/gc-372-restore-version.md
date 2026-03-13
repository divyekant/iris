# GC-372: Restore a Version — Verify Draft Body Is Updated

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: draft-versions
- **Tags**: draft, versions, restore, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Test account (source: local-db, setup: POST /api/accounts)
- Test draft with initial body `"Version one content."` (source: local-db, setup: POST /api/drafts)

## Steps

1. Save version 1 with the initial body
   - **Target**: `POST /api/drafts/{draft_id}/versions`
   - **Input**: `{ "account_id": "{account_id}" }`
   - **Expected**: 201 with `version_number` = `1`; body captured is `"Version one content."`

2. Update the draft body to new content
   - **Target**: `POST /api/drafts`
   - **Input**: `{ "id": "{draft_id}", "body": "Completely different content that replaced version one.", "account_id": "{account_id}" }`
   - **Expected**: 200

3. Save version 2 with the new content
   - **Target**: `POST /api/drafts/{draft_id}/versions`
   - **Input**: `{ "account_id": "{account_id}" }`
   - **Expected**: 201 with `version_number` = `2`

4. Confirm current draft body is from version 2
   - **Target**: `GET /api/drafts/{draft_id}` (or equivalent draft fetch)
   - **Input**: valid `draft_id`
   - **Expected**: 200; `body` = `"Completely different content that replaced version one."`

5. Restore version 1
   - **Target**: `POST /api/drafts/{draft_id}/versions/1/restore`
   - **Input**: `{ "account_id": "{account_id}" }`
   - **Expected**: 200 with confirmation that the draft was restored; response indicates the restored version number or updated draft state

6. Fetch the draft to verify the body was restored
   - **Target**: `GET /api/drafts/{draft_id}` (or equivalent)
   - **Input**: valid `draft_id`
   - **Expected**: 200; `body` = `"Version one content."` — the draft body matches version 1's snapshot exactly

7. Confirm a new version was created for the restore event
   - **Target**: `GET /api/drafts/{draft_id}/versions`
   - **Input**: valid `draft_id`
   - **Expected**: 200; list contains at least 3 entries (v1, v2, and the post-restore state), OR the restore is reflected without creating an additional version — either behavior is acceptable as long as the restored body is correct

## Success Criteria
- [ ] Restore endpoint returns 200 (not 404 or 500)
- [ ] Draft body after restore matches the body captured in version 1
- [ ] The body from the intermediate version 2 is no longer the current draft body
- [ ] Version list reflects the restore operation in a consistent way

## Failure Criteria
- Restore returns a non-200 status
- Draft body after restore still shows version 2 content
- Draft body after restore is empty or corrupted
- Fetch of draft returns 404 after restore
