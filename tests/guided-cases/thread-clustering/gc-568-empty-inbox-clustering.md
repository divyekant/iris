# GC-568: Compute Clusters on Empty Inbox Returns Zero Clusters Gracefully

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-clustering
- **Tags**: thread-clustering, empty-inbox, compute, edge
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- An account with no synced messages (or a freshly added account with empty inbox)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Compute clusters on empty inbox account
   - **Target**: `POST http://localhost:3030/api/thread-clusters/compute`
   - **Input**: Header `X-Session-Token: {token}`, body `{"account_id": "{empty_account_id}", "threshold": 0.7}`
   - **Expected**: 200 OK, `clusters_created: 0`, `threads_clustered: 0`

3. List clusters for the account
   - **Target**: `GET http://localhost:3030/api/thread-clusters`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `clusters` array is empty (not an error, not 404)

## Success Criteria
- [ ] Compute returns 200 OK with zero counts (not an error)
- [ ] List returns empty array
- [ ] No 5xx errors

## Failure Criteria
- Server errors on empty inbox
- Non-empty clusters list despite no threads
