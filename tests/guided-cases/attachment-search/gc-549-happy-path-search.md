# GC-549: Happy Path — Search Returns Results for Indexed Attachment

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: attachment-search
- **Tags**: attachments, search, FTS5, index, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one email message with a PDF or plain-text attachment containing known searchable text (e.g., "quarterly revenue")
- The attachment has been indexed via POST /api/attachments/index/{message_id}

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Index the attachment for a known message
   - **Target**: `POST http://localhost:3030/api/attachments/index/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`, path param `message_id` = ID of message with attachment
   - **Expected**: 200 OK, response indicates indexed count ≥ 1

3. Search for text known to be in the attachment
   - **Target**: `GET http://localhost:3030/api/attachments/search?q=quarterly+revenue&account_id={account_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `results` array contains at least one entry with `message_id`, `filename`, `snippet` fields

## Success Criteria
- [ ] Index step returns success with indexed_count ≥ 1
- [ ] Search returns 200 OK
- [ ] At least one result matches the queried message
- [ ] Each result includes `message_id`, `filename`, and `snippet`
- [ ] Snippet contains or highlights the search term

## Failure Criteria
- Index returns error or indexed_count = 0
- Search returns empty results for known text
- Response missing required fields
