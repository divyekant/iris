# GC-501: Extracted Data List with Invalid Type Filter

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: extracted-data
- **Tags**: extraction, validation, filter, query-param
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- None required

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. List extracted data with invalid type filter
   - **Target**: `GET http://localhost:3030/api/extracted-data?type=nonexistent_type`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with empty array (no data matches invalid type) or 400 Bad Request if type is validated

3. List extracted data with invalid since parameter
   - **Target**: `GET http://localhost:3030/api/extracted-data?since=not-a-date`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 400 Bad Request with error indicating invalid date format

## Success Criteria
- [ ] Invalid type filter returns 200 with empty results or 400
- [ ] Invalid since parameter returns 400 with clear error

## Failure Criteria
- Server returns 500 for invalid query parameters
- Invalid since format silently ignored (no filtering applied)
