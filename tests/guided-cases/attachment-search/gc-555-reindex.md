# GC-555: Reindex All Attachments for Account

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: attachment-search
- **Tags**: attachments, reindex, bulk, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one email account with multiple messages containing supported attachments

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Check current stats before reindex
   - **Target**: `GET http://localhost:3030/api/attachments/search/stats`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with `total_indexed` count (may be 0 or partial)

3. Trigger full reindex
   - **Target**: `POST http://localhost:3030/api/attachments/reindex`
   - **Input**: Header `X-Session-Token: {token}`, body `{"account_id": "{account_id}"}`
   - **Expected**: 200 OK, response includes `indexed_count` and `skipped_count`; operation completes without error

4. Check stats after reindex
   - **Target**: `GET http://localhost:3030/api/attachments/search/stats`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `total_indexed` ≥ the value from step 2 (should be equal or greater)

## Success Criteria
- [ ] Reindex returns 200 OK with `indexed_count` ≥ 0
- [ ] Stats after reindex reflect updated counts
- [ ] Previously-indexed attachments are still searchable after reindex

## Failure Criteria
- Reindex returns error status
- Stats unchanged after reindex when attachments exist
- Previously searchable content no longer findable
