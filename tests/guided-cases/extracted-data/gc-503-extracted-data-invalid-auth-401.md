# GC-503: Extracted Data Endpoints With Invalid Auth Return 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: extracted-data
- **Tags**: extraction, auth, invalid-token, 401, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- Invalid token: `invalid-token-12345`

## Steps
1. POST extract with invalid token
   - **Target**: `POST http://localhost:3030/api/extract/some-message-id`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

2. GET extracted data list with invalid token
   - **Target**: `GET http://localhost:3030/api/extracted-data`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

3. GET summary with invalid token
   - **Target**: `GET http://localhost:3030/api/extracted-data/summary`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] All endpoints return 401 with invalid session token
- [ ] No data leaked in any response

## Failure Criteria
- Any endpoint returns 200 with invalid token
- Response contains extracted data
