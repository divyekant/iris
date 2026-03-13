# GC-505: Delete Nonexistent Extracted Data Returns 404

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: extracted-data
- **Tags**: extraction, not-found, 404, DELETE
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Non-existent extraction ID: `nonexistent-extraction-999`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Delete nonexistent extracted data
   - **Target**: `DELETE http://localhost:3030/api/extracted-data/nonexistent-extraction-999`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] DELETE returns 404 for nonexistent extraction ID

## Failure Criteria
- Returns 200 or 204 for nonexistent ID
- Returns 500 Internal Server Error
