# GC-502: Extracted Data Endpoints Without Auth Return 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: extracted-data
- **Tags**: extraction, auth, 401, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- None

## Steps
1. POST extract without session token
   - **Target**: `POST http://localhost:3030/api/extract/some-message-id`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

2. GET extracted data list without session token
   - **Target**: `GET http://localhost:3030/api/extracted-data`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

3. GET extracted data summary without session token
   - **Target**: `GET http://localhost:3030/api/extracted-data/summary`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

4. DELETE extracted data without session token
   - **Target**: `DELETE http://localhost:3030/api/extracted-data/some-id`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] All four endpoints return 401 without X-Session-Token
- [ ] No extracted data leaked in response bodies

## Failure Criteria
- Any endpoint returns 200 without authentication
- Response body contains extracted data
