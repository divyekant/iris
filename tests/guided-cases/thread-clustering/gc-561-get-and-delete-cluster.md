# GC-561: Get Cluster by ID and Delete It

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-clustering
- **Tags**: thread-clustering, get, delete, lifecycle
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one existing cluster (ID known from list or compute step)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Get cluster details
   - **Target**: `GET http://localhost:3030/api/thread-clusters/{cluster_id}`
   - **Input**: Header `X-Session-Token: {token}`, path param `cluster_id` from precondition
   - **Expected**: 200 OK, response includes cluster fields plus `members` array of thread IDs

3. Delete the cluster
   - **Target**: `DELETE http://localhost:3030/api/thread-clusters/{cluster_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK or 204 No Content

4. Verify deletion
   - **Target**: `GET http://localhost:3030/api/thread-clusters/{cluster_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] GET cluster returns full details including `members` list
- [ ] DELETE returns 200 or 204
- [ ] Subsequent GET returns 404
- [ ] Cluster no longer appears in list after deletion

## Failure Criteria
- DELETE returns error
- Cluster still retrievable after deletion
- `members` array missing from GET response
