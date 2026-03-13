# GC-375: Negative — Diff with Missing from/to Query Parameters

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: draft-versions
- **Tags**: draft, versions, negative, diff, missing-params, 400
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Test account (source: local-db, setup: POST /api/accounts)
- Test draft with at least 2 saved versions (source: local-db, setup: POST /api/drafts then 2x POST /api/drafts/{draft_id}/versions)

## Steps

1. Attempt diff with both `from` and `to` missing
   - **Target**: `GET /api/drafts/{draft_id}/versions/diff`
   - **Input**: no query parameters
   - **Expected**: 400 Bad Request with an error message indicating that `from` and `to` are required

2. Attempt diff with only `from` provided (missing `to`)
   - **Target**: `GET /api/drafts/{draft_id}/versions/diff?from=1`
   - **Input**: `from=1`, `to` omitted
   - **Expected**: 400 Bad Request with an error message indicating that `to` is required

3. Attempt diff with only `to` provided (missing `from`)
   - **Target**: `GET /api/drafts/{draft_id}/versions/diff?to=2`
   - **Input**: `from` omitted, `to=2`
   - **Expected**: 400 Bad Request with an error message indicating that `from` is required

4. Attempt diff with empty string values for both params
   - **Target**: `GET /api/drafts/{draft_id}/versions/diff?from=&to=`
   - **Input**: `from=""`, `to=""`
   - **Expected**: 400 Bad Request — empty strings are not valid version numbers

5. Confirm that a valid diff still works after the invalid attempts
   - **Target**: `GET /api/drafts/{draft_id}/versions/diff?from=1&to=2`
   - **Input**: `from=1`, `to=2`
   - **Expected**: 200 with valid diff response — invalid requests did not break the endpoint

## Success Criteria
- [ ] Missing both params returns 400
- [ ] Missing `to` only returns 400
- [ ] Missing `from` only returns 400
- [ ] Empty string params return 400
- [ ] Error responses include a message explaining which parameter is missing or invalid
- [ ] Valid diff request (from=1&to=2) still returns 200 after all failed attempts
- [ ] No 500 errors from any of the invalid requests

## Failure Criteria
- Any missing-param request returns 200 or 500
- Server silently defaults missing params to 0 or 1 and returns a diff
- Empty string params are accepted and treated as valid version numbers
- Valid diff is broken by the sequence of invalid requests
