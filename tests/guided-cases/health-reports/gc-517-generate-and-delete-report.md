# GC-517: Generate Report Then Delete and Verify

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: health-reports
- **Tags**: health-reports, generate, delete, lifecycle
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

2. Generate a health report
   - **Target**: `POST http://localhost:3030/api/health-reports/generate`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200/201 with report object containing `id`

3. Verify report in list
   - **Target**: `GET http://localhost:3030/api/health-reports`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, array contains the generated report

4. Delete the report
   - **Target**: `DELETE http://localhost:3030/api/health-reports/{id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK or 204 No Content

5. Verify report removed
   - **Target**: `GET http://localhost:3030/api/health-reports/{id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

6. Verify report not in list
   - **Target**: `GET http://localhost:3030/api/health-reports`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, array does not contain the deleted report

## Success Criteria
- [ ] Report generates successfully
- [ ] Report appears in list after generation
- [ ] DELETE removes the report
- [ ] GET by ID returns 404 after deletion
- [ ] Report no longer in list after deletion

## Failure Criteria
- Any step returns unexpected status code
- Report persists after deletion
