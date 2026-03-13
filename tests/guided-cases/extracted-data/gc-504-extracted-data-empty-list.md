# GC-504: Extracted Data List Empty State

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: extracted-data
- **Tags**: extraction, empty-state, list, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- No extractions have been performed (clean state)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. List extracted data when none exist
   - **Target**: `GET http://localhost:3030/api/extracted-data`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with empty array `[]`

3. Get summary when no data exists
   - **Target**: `GET http://localhost:3030/api/extracted-data/summary`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with empty or zero-count summary object

## Success Criteria
- [ ] GET /api/extracted-data returns 200 with empty array
- [ ] GET /api/extracted-data/summary returns 200 with valid (empty) summary
- [ ] Responses are valid JSON

## Failure Criteria
- Returns non-200 status code
- Returns null instead of empty array/object
- Returns 500 error
