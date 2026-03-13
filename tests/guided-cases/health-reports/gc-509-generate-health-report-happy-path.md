# GC-509: Generate Health Report Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: health-reports
- **Tags**: health-reports, generate, happy-path, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one email account configured with synced messages (for report data)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Generate a health report
   - **Target**: `POST http://localhost:3030/api/health-reports/generate`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK or 201 Created with JSON body containing `id`, `generated_at`, and report data fields (metrics, scores, recommendations)

3. Verify report exists via GET
   - **Target**: `GET http://localhost:3030/api/health-reports/{id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with matching report data

4. Verify report appears in list
   - **Target**: `GET http://localhost:3030/api/health-reports`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with array containing the generated report

## Success Criteria
- [ ] POST /api/health-reports/generate returns success with report object
- [ ] Report contains `id` and `generated_at` fields
- [ ] GET /api/health-reports/{id} returns matching report
- [ ] GET /api/health-reports list includes the report

## Failure Criteria
- POST returns non-200/201 status
- Response missing report ID or timestamp
- GET by ID does not match generated report
- Report not in list endpoint
