# GC-630: Knowledge Graph — Query Returns Entity with Relations and Connected Threads

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: showcase-features
- **Tags**: knowledge-graph, query, relations, threads, search
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Entity "Sarah" has been previously extracted via POST /api/graph/extract (GC-629 passed)
- At least 2 messages referencing "Sarah" have been extracted so she has multiple connected threads

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Query the knowledge graph for "Sarah"
   - **Target**: `GET http://localhost:3030/api/graph?query=Sarah`
   - **Input**: Header `X-Session-Token: {token}`, query param `query=Sarah`
   - **Expected**: 200 OK, response contains `entities` array with at least one match

3. Verify entity structure includes relations and threads
   - **Target**: response from step 2
   - **Input**: inspect `entities[0]`
   - **Expected**: entity has `name` containing "Sarah", `type: "person"`, a `relations` array (may be empty), and a `thread_ids` array with ≥ 1 entry

4. Verify account isolation
   - **Target**: `GET http://localhost:3030/api/graph?query=Sarah&account_id={other_account_id}` (if multiple accounts exist)
   - **Input**: Header `X-Session-Token: {token}`, different account
   - **Expected**: returns empty or only entities belonging to that account

## Success Criteria
- [ ] GET /api/graph?query=Sarah returns 200 OK
- [ ] `entities` array contains at least one entry
- [ ] Matched entity has `name`, `type`, `relations`, `thread_ids`
- [ ] `thread_ids` is non-empty (references real thread IDs in the database)
- [ ] Results scoped to the requesting account

## Failure Criteria
- 400 if `query` param is missing
- Empty `entities` array when "Sarah" was previously extracted
- Entity missing `thread_ids` or `relations` fields
- Cross-account data leakage
