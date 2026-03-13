# GC-559: Compute Thread Clusters Groups Similar Threads

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: thread-clustering
- **Tags**: thread-clustering, compute, similarity, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least 3 threads with similar subjects/content about "Q4 budget planning" and 2 unrelated threads

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Compute thread clusters
   - **Target**: `POST http://localhost:3030/api/thread-clusters/compute`
   - **Input**: Header `X-Session-Token: {token}`, body `{"account_id": "{account_id}", "threshold": 0.7}`
   - **Expected**: 200 OK, response includes `clusters_created` count ≥ 1 and `threads_clustered` count

3. List clusters to verify results
   - **Target**: `GET http://localhost:3030/api/thread-clusters`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, at least one cluster in `clusters` array; cluster contains `id`, `label`, `thread_count`, `representative_thread_id`

## Success Criteria
- [ ] Compute returns 200 OK with `clusters_created` ≥ 1
- [ ] Similar threads are grouped into the same cluster
- [ ] Cluster objects contain `id`, `label`, and `thread_count` fields
- [ ] `thread_count` reflects the number of grouped threads

## Failure Criteria
- No clusters created despite similar threads
- 5xx error during compute
- List returns empty after successful compute
