# gc-search-001: Search with from: Operator

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: search-operators
- **Tags**: search, operator, from, filter
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- At least one message in the database with `from_address` or `from_name` containing "alice@example.com"
- At least one message from a different sender (to confirm filtering)

## Steps

1. Obtain session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap` with header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `{"token": "<session_token>"}`

2. Search with from: operator
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=from:alice@example.com` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with JSON response containing:
     - `parsed_operators` array includes `{"key": "from", "value": "alice@example.com"}`
     - `query` equals `"from:alice@example.com"`
     - All `results` entries have `from_address` or `from_name` matching `%alice@example.com%` (case-insensitive LIKE)
     - Messages from other senders are excluded

## Success Criteria
- [ ] Response status is 200
- [ ] `parsed_operators` has exactly 1 entry with key "from" and value "alice@example.com"
- [ ] All returned results have from_address or from_name containing "alice@example.com"
- [ ] `total` matches the count of results (no pagination overflow)
- [ ] No free text is passed to FTS5 (operator-only query path used)

## Failure Criteria
- Response returns non-200 status
- `parsed_operators` is empty or does not include the from operator
- Results include messages not from alice@example.com
- Server error due to SQL query construction
