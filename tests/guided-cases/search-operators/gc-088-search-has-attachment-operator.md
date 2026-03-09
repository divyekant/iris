# gc-search-003: Search with has:attachment Operator

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: search-operators
- **Tags**: search, operator, has, attachment, fts5, combined
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- At least one message with `has_attachments=1` containing "invoice" in subject or body
- At least one message without attachments containing "invoice"
- Messages indexed in FTS5

## Steps

1. Obtain session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap` with header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `{"token": "<session_token>"}`

2. Search with has:attachment and free text
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=has:attachment%20invoice` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with JSON response containing:
     - `parsed_operators` array includes `{"key": "has", "value": "attachment"}`
     - `query` equals `"has:attachment invoice"`
     - All `results` entries have `has_attachments` equal to `true`
     - Results match FTS5 text "invoice"

3. Verify plural variant also works
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=has:attachments%20invoice` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with identical filtering behavior; `parsed_operators` includes `{"key": "has", "value": "attachments"}`

## Success Criteria
- [ ] Response status is 200 for both queries
- [ ] `parsed_operators` includes has operator with value "attachment" (or "attachments")
- [ ] All returned results have `has_attachments` = true
- [ ] Messages without attachments containing "invoice" are excluded
- [ ] Both "attachment" and "attachments" values produce the same SQL condition (m.has_attachments = 1)

## Failure Criteria
- Response returns non-200 status
- Results include messages where has_attachments is false
- Plural variant "has:attachments" is not recognized as an operator
- Server error in SQL query
