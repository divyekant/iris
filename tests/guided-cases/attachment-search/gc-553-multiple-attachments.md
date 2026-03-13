# GC-553: Message with Multiple Attachments — All Indexed and Searchable

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: attachment-search
- **Tags**: attachments, index, multiple, search
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message with two supported attachments: one PDF named "budget.pdf" containing "Q3 budget allocation", and one plain-text named "notes.txt" containing "action items review"

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Index the message
   - **Target**: `POST http://localhost:3030/api/attachments/index/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `indexed_count: 2`

3. Search for text from the first attachment
   - **Target**: `GET http://localhost:3030/api/attachments/search?q=Q3+budget+allocation&account_id={account_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, result with `filename: "budget.pdf"`

4. Search for text from the second attachment
   - **Target**: `GET http://localhost:3030/api/attachments/search?q=action+items+review&account_id={account_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, result with `filename: "notes.txt"`

## Success Criteria
- [ ] `indexed_count` equals the number of supported attachments (2)
- [ ] Each attachment's text is independently searchable
- [ ] Results correctly identify the originating filename
- [ ] Both results reference the same `message_id`

## Failure Criteria
- indexed_count less than 2
- Only one attachment's text is searchable
- Filename missing from results
