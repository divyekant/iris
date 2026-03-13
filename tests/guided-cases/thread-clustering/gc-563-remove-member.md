# GC-563: Remove a Thread Member from a Cluster

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-clustering
- **Tags**: thread-clustering, remove-member, DELETE
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A cluster with at least 3 member threads; one specific thread_id identified for removal

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Remove a member thread from the cluster
   - **Target**: `DELETE http://localhost:3030/api/thread-clusters/{cluster_id}/members/{thread_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK or 204 No Content

3. Verify the thread is no longer in the cluster
   - **Target**: `GET http://localhost:3030/api/thread-clusters/{cluster_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `members` array does not contain the removed `thread_id`; `thread_count` decremented by 1

4. Confirm the removed thread is not deleted (only removed from cluster)
   - **Target**: `GET http://localhost:3030/api/messages?thread_id={thread_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, thread messages still accessible

## Success Criteria
- [ ] DELETE member returns 200 or 204
- [ ] `members` list no longer contains removed thread
- [ ] `thread_count` decremented correctly
- [ ] Original thread is not deleted, only removed from cluster membership

## Failure Criteria
- DELETE returns error
- Thread still present in cluster members after removal
- Thread count not decremented
