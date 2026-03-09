# gc-search-008: Unknown Operator Kept as Text

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: search-operators
- **Tags**: search, operator, unknown, fallback, text, edge-case
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- At least one message indexed in FTS5 containing the text "hello"
- Messages indexed in FTS5

## Steps

1. Obtain session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap` with header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `{"token": "<session_token>"}`

2. Search with unknown operator
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=label:important%20hello` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with JSON response containing:
     - `parsed_operators` is an empty array `[]` (label is not a recognized operator)
     - `query` equals `"label:important hello"`
     - The entire query "label:important hello" is treated as FTS5 text search
     - Results match FTS5 for the terms "label:important" and "hello"

3. Verify another unknown operator
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=tag:work%20project` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with `parsed_operators` as empty array; "tag:work project" treated as text

## Success Criteria
- [ ] Response status is 200
- [ ] `parsed_operators` is empty (no operators parsed)
- [ ] "label:important" is kept as part of the text query, not silently dropped
- [ ] FTS5 path is used (free text is present)
- [ ] No server error from unrecognized operator syntax
- [ ] Results reflect FTS5 matching on the full text including the unknown operator token

## Failure Criteria
- Response returns non-200 status
- "label:important" is parsed as an operator (should not be)
- Unknown operator token is silently dropped from the text query
- Server crashes or returns 500 on unknown operator
