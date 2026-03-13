# GC-551: Unsupported Attachment Content Type Is Skipped During Indexing

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: attachment-search
- **Tags**: attachments, index, content-type, unsupported
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message with an attachment of an unsupported type (e.g., image/jpeg, video/mp4, application/zip)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Attempt to index the message with an unsupported attachment type
   - **Target**: `POST http://localhost:3030/api/attachments/index/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`, `message_id` of message with only binary/image attachments
   - **Expected**: 200 OK (not an error), response indicates `indexed_count: 0` and `skipped_count` ≥ 1 with reason noting unsupported content type

3. Confirm the unsupported attachment does not appear in search
   - **Target**: `GET http://localhost:3030/api/attachments/search?q=jpeg&account_id={account_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `results` array is empty

## Success Criteria
- [ ] Index returns 200 (graceful handling, not an error)
- [ ] `indexed_count` is 0 for unsupported types
- [ ] Response includes skipped information or reason
- [ ] Search returns no results for binary attachment content

## Failure Criteria
- Server returns 4xx or 5xx when encountering unsupported content type
- Binary attachment content appears in search results
