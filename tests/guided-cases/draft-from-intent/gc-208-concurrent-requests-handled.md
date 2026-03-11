# GC-208: Draft from Intent — Concurrent Requests Handled

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: draft-from-intent
- **Tags**: draft-from-intent, api, concurrency, edge
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Send 3 concurrent draft-from-intent requests with different intents
   - **Target**: `POST /api/ai/draft-from-intent` (3 parallel requests)
   - **Input**:
     - Request A: `{"intent": "Schedule a team lunch for Friday"}`
     - Request B: `{"intent": "Request access to the staging server"}`
     - Request C: `{"intent": "Thank the client for their feedback"}`
   - **Expected**: All 3 requests return 200 OK within a reasonable time

2. Verify each response matches its input
   - **Target**: Response bodies from all 3 requests
   - **Input**: n/a
   - **Expected**: Each response's subject and body are relevant to their respective intent (no cross-contamination)

3. Verify server stability after concurrent requests
   - **Target**: `GET /api/health`
   - **Input**: n/a
   - **Expected**: 200 OK

## Success Criteria
- [ ] All 3 concurrent requests return 200
- [ ] Each response is relevant to its own intent (no mixing)
- [ ] Server remains healthy after processing concurrent requests
- [ ] No 500 errors or timeouts

## Failure Criteria
- Any request returns 500 or times out
- Responses are mixed up (e.g., lunch topic in server access response)
- Server becomes unhealthy after concurrent load
