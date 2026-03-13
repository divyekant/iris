# GC-565: Compute with No Similar Threads Returns Zero Clusters

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-clustering
- **Tags**: thread-clustering, compute, empty, no-similarity
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- An account where all threads have completely unrelated topics (or an empty inbox)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Compute clusters with high threshold on account with dissimilar threads
   - **Target**: `POST http://localhost:3030/api/thread-clusters/compute`
   - **Input**: Header `X-Session-Token: {token}`, body `{"account_id": "{account_id}", "threshold": 0.95}`
   - **Expected**: 200 OK, `clusters_created: 0` (no similar threads found)

3. List clusters to confirm empty state
   - **Target**: `GET http://localhost:3030/api/thread-clusters`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `clusters` array is empty

## Success Criteria
- [ ] Compute returns 200 OK (not an error) when no clusters are formed
- [ ] `clusters_created` is 0
- [ ] List returns empty array (not 404)

## Failure Criteria
- Server returns error when no similar threads exist
- Clusters incorrectly formed from unrelated threads
