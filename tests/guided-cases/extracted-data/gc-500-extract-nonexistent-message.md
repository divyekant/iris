# GC-500: Extract Data from Nonexistent Message

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: extracted-data
- **Tags**: extraction, validation, not-found, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Non-existent message ID: `nonexistent-msg-999`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Extract from nonexistent message
   - **Target**: `POST http://localhost:3030/api/extract/nonexistent-msg-999`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] POST returns 404 for nonexistent message ID
- [ ] Response does not contain extracted data

## Failure Criteria
- Returns 200 with empty data (should be 404)
- Returns 500 Internal Server Error
