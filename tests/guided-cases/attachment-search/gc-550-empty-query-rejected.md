# GC-550: Empty Search Query Is Rejected

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: attachment-search
- **Tags**: attachments, search, validation, empty-query
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
   - **Expected**: 200 OK, response body contains `token` field

2. Submit search with an empty query string
   - **Target**: `GET http://localhost:3030/api/attachments/search?q=&account_id={account_id}`
   - **Input**: Header `X-Session-Token: {token}`, `q` is empty string
   - **Expected**: 400 Bad Request with error message indicating the query is required

3. Submit search with only whitespace
   - **Target**: `GET http://localhost:3030/api/attachments/search?q=+++&account_id={account_id}`
   - **Input**: Header `X-Session-Token: {token}`, `q` is URL-encoded whitespace
   - **Expected**: 400 Bad Request

## Success Criteria
- [ ] Empty `q` returns 400 with descriptive error
- [ ] Whitespace-only `q` returns 400
- [ ] No 500 errors triggered

## Failure Criteria
- Server returns 200 with empty or full results for empty query
- Server returns 500 (unhandled)
