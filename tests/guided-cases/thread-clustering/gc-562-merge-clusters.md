# GC-562: Merge Two Clusters into One

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-clustering
- **Tags**: thread-clustering, merge, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Two distinct clusters exist: cluster_A (3 threads) and cluster_B (2 threads)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Merge cluster_B into cluster_A
   - **Target**: `POST http://localhost:3030/api/thread-clusters/{cluster_a_id}/merge`
   - **Input**: Header `X-Session-Token: {token}`, body `{"merge_from_id": "{cluster_b_id}"}`
   - **Expected**: 200 OK, response includes merged cluster with `thread_count: 5`

3. Verify cluster_B no longer exists
   - **Target**: `GET http://localhost:3030/api/thread-clusters/{cluster_b_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found (cluster_B was consumed by merge)

4. Verify cluster_A now has all 5 threads
   - **Target**: `GET http://localhost:3030/api/thread-clusters/{cluster_a_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `members` array has 5 entries

## Success Criteria
- [ ] Merge returns 200 OK
- [ ] Resulting cluster has combined thread count
- [ ] Source cluster (cluster_B) no longer exists
- [ ] All threads from both clusters are present in merged cluster

## Failure Criteria
- Merge returns error
- Source cluster persists after merge
- Thread count incorrect after merge
