# gc-search-002: Search with is:unread Operator and Free Text

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: search-operators
- **Tags**: search, operator, is, unread, fts5, combined
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- At least one unread message (is_read=0) with "meeting" in subject or body text
- At least one read message (is_read=1) with "meeting" in subject or body text
- Messages indexed in FTS5

## Steps

1. Obtain session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap` with header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `{"token": "<session_token>"}`

2. Search with is:unread and free text
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=is:unread%20meeting` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with JSON response containing:
     - `parsed_operators` array includes `{"key": "is", "value": "unread"}`
     - `query` equals `"is:unread meeting"`
     - All `results` entries have `is_read` equal to `false`
     - All results match the FTS5 text query "meeting"
     - Read messages containing "meeting" are excluded

## Success Criteria
- [ ] Response status is 200
- [ ] `parsed_operators` has exactly 1 entry with key "is" and value "unread"
- [ ] All returned results have `is_read` = false
- [ ] Results contain matches for "meeting" (visible in snippet or subject)
- [ ] FTS5 path is used (has_fts=true since free text "meeting" is present)
- [ ] Read messages with "meeting" are not in results

## Failure Criteria
- Response returns non-200 status
- Results include read messages (is_read=true)
- No FTS5 matching occurs for "meeting" free text
- `parsed_operators` does not contain the is:unread operator
