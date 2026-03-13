# GC-374: Negative — Get Version with Invalid Version Number

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: draft-versions
- **Tags**: draft, versions, negative, not-found, 404, invalid-version
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

1. Attempt to get a version number that is too high (beyond the saved count)
   - **Target**: `GET /api/drafts/{draft_id}/versions/9999`
   - **Input**: valid `draft_id`, `version_number` = `9999`
   - **Expected**: 404 Not Found with an error indicating the version does not exist

2. Attempt to get version number 0 (versions start at 1)
   - **Target**: `GET /api/drafts/{draft_id}/versions/0`
   - **Input**: valid `draft_id`, `version_number` = `0`
   - **Expected**: 404 Not Found or 400 Bad Request — version 0 does not exist

3. Attempt to get a version with a negative number
   - **Target**: `GET /api/drafts/{draft_id}/versions/-1`
   - **Input**: valid `draft_id`, `version_number` = `-1`
   - **Expected**: 400 Bad Request or 404 Not Found — negative version numbers are invalid

4. Attempt to get a version with a non-numeric string
   - **Target**: `GET /api/drafts/{draft_id}/versions/abc`
   - **Input**: valid `draft_id`, `version_number` = `"abc"`
   - **Expected**: 400 Bad Request — non-numeric version number is rejected; server does not panic or return 500

5. Verify that valid version 1 is still accessible after invalid attempts
   - **Target**: `GET /api/drafts/{draft_id}/versions/1`
   - **Input**: valid `draft_id`, `version_number` = `1`
   - **Expected**: 200 with full version detail — the invalid requests did not corrupt state

## Success Criteria
- [ ] Version 9999 on a draft with only 1 version returns 404
- [ ] Version 0 returns 404 or 400
- [ ] Version -1 returns 400 or 404
- [ ] Non-numeric version string returns 400 (not 500)
- [ ] Valid version 1 remains accessible after all invalid requests
- [ ] No 500 errors from any of the invalid requests

## Failure Criteria
- Any invalid version number returns 200 with version data
- Non-numeric version number causes a server panic or returns 500
- Valid version 1 becomes inaccessible after invalid attempts
- Version 0 or negative numbers return 200
