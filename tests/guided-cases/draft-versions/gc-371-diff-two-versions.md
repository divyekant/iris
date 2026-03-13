# GC-371: Diff Two Versions — Verify Additions, Removals, Word Count Changes

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: draft-versions
- **Tags**: draft, versions, diff, additions, removals
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Test account (source: local-db, setup: POST /api/accounts)
- Test draft with initial body `"Hello, please review the attached report."` (source: local-db, setup: POST /api/drafts)

## Steps

1. Save version 1 with the initial body
   - **Target**: `POST /api/drafts/{draft_id}/versions`
   - **Input**: `{ "account_id": "{account_id}" }`
   - **Expected**: 201 with `version_number` = `1`

2. Update the draft body to a meaningfully different value
   - **Target**: `POST /api/drafts`
   - **Input**: `{ "id": "{draft_id}", "body": "Hello, please review the attached Q3 report and share your feedback by Friday.", "account_id": "{account_id}" }`
   - **Expected**: 200

3. Save version 2 with the updated body
   - **Target**: `POST /api/drafts/{draft_id}/versions`
   - **Input**: `{ "account_id": "{account_id}" }`
   - **Expected**: 201 with `version_number` = `2`

4. Request a diff between version 1 and version 2
   - **Target**: `GET /api/drafts/{draft_id}/versions/diff?from=1&to=2`
   - **Input**: `from=1`, `to=2`
   - **Expected**: 200 with a diff response that includes additions (words/lines added in version 2) and removals (words/lines removed from version 1); word count or character count change is present

5. Verify diff direction is from→to (version 1 as base)
   - **Target**: Inspect diff response from step 4
   - **Input**: additions and removals lists
   - **Expected**: The phrase `"Q3"`, `"feedback"`, and `"by Friday"` appear as additions (present in v2, not in v1); no unexpected removals of content that exists in both versions

6. Request the reverse diff (from=2, to=1) to confirm directionality
   - **Target**: `GET /api/drafts/{draft_id}/versions/diff?from=2&to=1`
   - **Input**: `from=2`, `to=1`
   - **Expected**: 200; additions and removals are swapped relative to step 4 response — what was added is now removed and vice versa

## Success Criteria
- [ ] Diff endpoint returns 200 with a structured diff response
- [ ] Additions in the from=1&to=2 diff contain content that is in v2 but not in v1
- [ ] Removals in the from=1&to=2 diff contain content that is in v1 but not in v2
- [ ] Word/character count change is non-zero and reflects the actual content difference
- [ ] Reverse diff (from=2&to=1) produces the inverse additions/removals
- [ ] Diff response does not include content unchanged between both versions as additions or removals

## Failure Criteria
- Diff endpoint returns 404 or 500
- Additions and removals are empty despite content changing between versions
- Reverse diff produces identical output to forward diff
- Word count change is zero when content clearly changed
