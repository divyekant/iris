# GC-566: Transitive Similarity — Threads Indirectly Similar Are Grouped

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-clustering
- **Tags**: thread-clustering, transitive, similarity, algorithm
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Three threads: Thread A and B are similar (0.8 similarity), Thread B and C are similar (0.8 similarity), Thread A and C have lower direct similarity (0.5). Threshold set to 0.6.

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Compute clusters with threshold 0.6
   - **Target**: `POST http://localhost:3030/api/thread-clusters/compute`
   - **Input**: Header `X-Session-Token: {token}`, body `{"account_id": "{account_id}", "threshold": 0.6}`
   - **Expected**: 200 OK

3. Retrieve the cluster containing thread A
   - **Target**: `GET http://localhost:3030/api/thread-clusters`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: One cluster containing all 3 threads (A, B, C) due to transitive similarity through B; OR two separate clusters if the algorithm uses direct-only comparison — note actual behavior in test report

## Success Criteria
- [ ] Compute returns 200 OK without error
- [ ] The `members` list of the resulting cluster is documented (transitive grouping behavior verified and recorded)
- [ ] No 5xx errors

## Failure Criteria
- Server crashes on overlapping similarity relationships
- Compute never completes (infinite loop in transitive resolution)
