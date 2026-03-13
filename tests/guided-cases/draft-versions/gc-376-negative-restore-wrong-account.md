# GC-376: Negative — Restore Version with Wrong account_id

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: draft-versions
- **Tags**: draft, versions, negative, restore, account-mismatch, 404, authorization
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Two test accounts: `account_id_A` (owner of the draft) and `account_id_B` (a different account) (source: local-db, setup: 2x POST /api/accounts)
- Test draft created under `account_id_A` with at least 1 saved version (source: local-db, setup: POST /api/drafts with `account_id_A`, then POST /api/drafts/{draft_id}/versions)

## Steps

1. Save a version of the draft under account A
   - **Target**: `POST /api/drafts/{draft_id}/versions`
   - **Input**: `{ "account_id": "{account_id_A}" }`
   - **Expected**: 201 with `version_number` = `1`

2. Attempt to restore that version using account B's ID
   - **Target**: `POST /api/drafts/{draft_id}/versions/1/restore`
   - **Input**: `{ "account_id": "{account_id_B}" }`
   - **Expected**: 404 Not Found — the draft does not belong to account B, so the restore is rejected

3. Confirm the draft body is unchanged after the failed restore
   - **Target**: `GET /api/drafts/{draft_id}` (or equivalent draft fetch with `account_id_A`)
   - **Input**: valid `draft_id`
   - **Expected**: 200; `body` matches the content that was in the draft before the restore attempt — the failed restore did not alter the draft

4. Verify restore succeeds with the correct account_id
   - **Target**: `POST /api/drafts/{draft_id}/versions/1/restore`
   - **Input**: `{ "account_id": "{account_id_A}" }`
   - **Expected**: 200 — restore succeeds when the correct account is used

## Success Criteria
- [ ] Restore with wrong `account_id` returns 404
- [ ] Draft body is unchanged after the failed restore attempt
- [ ] Restore with the correct `account_id` returns 200
- [ ] No cross-account data leak — account B cannot access or modify account A's drafts
- [ ] No 500 errors from any step

## Failure Criteria
- Restore with wrong `account_id` returns 200 (cross-account restore allowed)
- Draft body is modified after the failed restore attempt
- Server returns 500 instead of 404 for wrong account
- Account B can read or list versions for a draft it does not own
