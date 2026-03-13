# GC-560: List Clusters Returns All Clusters with Correct Structure

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: thread-clustering
- **Tags**: thread-clustering, list, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least 2 clusters previously computed via POST /api/thread-clusters/compute

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. List all clusters
   - **Target**: `GET http://localhost:3030/api/thread-clusters`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response contains `clusters` array with ≥ 2 entries

3. Validate structure of a single cluster entry
   - Each cluster entry in `clusters` array should include:
     - `id` (string or integer)
     - `label` (non-empty string describing the cluster topic)
     - `thread_count` (integer ≥ 1)
     - `representative_thread_id` (string)
     - `created_at` (timestamp)
   - **Expected**: All fields present and correctly typed

## Success Criteria
- [ ] GET returns 200 OK
- [ ] `clusters` array contains all previously computed clusters
- [ ] Each cluster has required fields (`id`, `label`, `thread_count`, `representative_thread_id`, `created_at`)
- [ ] `thread_count` is accurate

## Failure Criteria
- Response missing `clusters` array
- Cluster entries missing required fields
- `thread_count` is 0 for clusters with members
