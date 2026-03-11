# GC-228: Briefing — Concurrent Requests Handled

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: briefing
- **Tags**: briefing, api, concurrency, edge
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- At least one email account synced with messages
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Send 3 concurrent briefing requests
   - **Target**: `GET /api/ai/briefing` (3 parallel requests)
   - **Input**: Valid `X-Session-Token` header on all 3
   - **Expected**: All 3 requests return 200 OK

2. Verify all responses are valid
   - **Target**: Response bodies from all 3 requests
   - **Input**: n/a
   - **Expected**: Each response has `summary`, `stats`, and `highlights`; stats values are consistent across all responses

3. Verify server stability
   - **Target**: `GET /api/health`
   - **Input**: n/a
   - **Expected**: 200 OK

## Success Criteria
- [ ] All 3 concurrent requests return 200
- [ ] Each response has valid `summary`, `stats`, and `highlights`
- [ ] `stats` values are consistent across responses (same data source)
- [ ] Server remains healthy

## Failure Criteria
- Any request returns 500 or times out
- Responses have inconsistent stats
- Server becomes unhealthy after concurrent requests
