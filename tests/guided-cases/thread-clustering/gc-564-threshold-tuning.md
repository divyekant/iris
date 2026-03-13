# GC-564: Threshold Tuning Affects Number of Clusters Formed

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-clustering
- **Tags**: thread-clustering, threshold, compute, tuning
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least 5 threads with varying degrees of similarity

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Compute clusters with a low threshold (more inclusive)
   - **Target**: `POST http://localhost:3030/api/thread-clusters/compute`
   - **Input**: Header `X-Session-Token: {token}`, body `{"account_id": "{account_id}", "threshold": 0.3}`
   - **Expected**: 200 OK, record `clusters_created` (low threshold = fewer, larger clusters)

3. Delete all clusters to reset state
   - Delete each cluster returned in step 2 via `DELETE /api/thread-clusters/{id}`
   - **Expected**: All deletions return 200 or 204

4. Compute clusters with a high threshold (more restrictive)
   - **Target**: `POST http://localhost:3030/api/thread-clusters/compute`
   - **Input**: Header `X-Session-Token: {token}`, body `{"account_id": "{account_id}", "threshold": 0.9}`
   - **Expected**: 200 OK, `clusters_created` ≥ value from step 2 (higher threshold = more, smaller clusters)

## Success Criteria
- [ ] Both compute calls return 200 OK
- [ ] Higher threshold produces equal or more clusters than lower threshold
- [ ] Threshold parameter is actually honored by the algorithm
- [ ] Invalid threshold (e.g., 1.5) returns 400

## Failure Criteria
- Both thresholds produce identical cluster counts despite different inputs
- Threshold outside [0.0, 1.0] range accepted without error
