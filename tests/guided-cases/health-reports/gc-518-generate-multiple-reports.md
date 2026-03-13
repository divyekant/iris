# GC-518: Generate Multiple Health Reports

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: health-reports
- **Tags**: health-reports, multiple, generate, list
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one email account configured

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Generate first health report
   - **Target**: `POST http://localhost:3030/api/health-reports/generate`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200/201 with report object, note `id` as `id_1`

3. Generate second health report
   - **Target**: `POST http://localhost:3030/api/health-reports/generate`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200/201 with report object, note `id` as `id_2`

4. Verify both reports in list
   - **Target**: `GET http://localhost:3030/api/health-reports`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with array containing both `id_1` and `id_2`, ordered by most recent first

5. Verify reports have distinct IDs
   - **Expected**: `id_1` != `id_2`

## Success Criteria
- [ ] Both reports generated successfully with unique IDs
- [ ] List endpoint returns both reports
- [ ] Reports are ordered by generation time (most recent first)

## Failure Criteria
- Second generation overwrites first report
- List missing one of the reports
- Reports have duplicate IDs
