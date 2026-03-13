# GC-567: Re-Compute Clusters After New Threads Arrive Reflects Updated State

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-clustering
- **Tags**: thread-clustering, recompute, incremental, update
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Initial state: 2 threads about "product launch" already clustered
- New state: a 3rd thread about "product launch" has been synced to the inbox

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Record existing cluster state
   - **Target**: `GET http://localhost:3030/api/thread-clusters`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, record existing cluster IDs and thread counts

3. Re-compute clusters
   - **Target**: `POST http://localhost:3030/api/thread-clusters/compute`
   - **Input**: Header `X-Session-Token: {token}`, body `{"account_id": "{account_id}", "threshold": 0.7}`
   - **Expected**: 200 OK

4. Verify the new thread is now clustered
   - **Target**: `GET http://localhost:3030/api/thread-clusters`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, the cluster for "product launch" topics now shows `thread_count` = 3 (or a new cluster was formed including the new thread)

## Success Criteria
- [ ] Re-compute returns 200 OK
- [ ] New thread is included in a cluster after re-compute
- [ ] Thread count for the relevant cluster increased by 1

## Failure Criteria
- New thread not included in any cluster after re-compute
- Re-compute deletes or corrupts previously valid clusters
