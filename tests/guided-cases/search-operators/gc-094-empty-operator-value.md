# gc-search-009: Empty Operator Value Treated as Text

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: search-operators
- **Tags**: search, operator, empty, edge-case, parsing
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

2. Search with empty operator value
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=from:%20hello` (query: `from: hello`) with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with JSON response containing:
     - `parsed_operators` is an empty array `[]`
     - `query` equals `"from: hello"`
     - The token "from:" has an empty value after the colon, so `value.is_empty()` is true
     - "from:" is kept as a text token, and the full text "from: hello" is passed to FTS5
     - Parser logic: `from:` tokenized as one token with empty value after colon; since value is empty, it falls to text_parts

3. Verify with other operators having empty values
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=subject:%20test` (query: `subject: test`) with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with `parsed_operators` empty; "subject:" and "test" treated as text

## Success Criteria
- [ ] Response status is 200
- [ ] `parsed_operators` is empty (operator with empty value is not parsed)
- [ ] "from:" token is preserved in the text query (not silently dropped)
- [ ] FTS5 search is performed on the remaining text
- [ ] No server error from empty operator value

## Failure Criteria
- "from:" with empty value is parsed as an operator (should not be per code: `if !value.is_empty()`)
- Response returns non-200 status
- Empty operator causes server error or panic
- Token is silently dropped from query
