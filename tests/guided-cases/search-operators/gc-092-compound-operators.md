# gc-search-007: Compound Operators (Multiple Combined)

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: search-operators
- **Tags**: search, operator, compound, multiple, from, is, has
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- At least one message matching ALL of: from "sarah", is_read=0 (unread), has_attachments=1
- At least one message from "sarah" that is read (is_read=1) without attachments
- At least one unread message with attachments from a different sender

## Steps

1. Obtain session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap` with header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `{"token": "<session_token>"}`

2. Search with three combined operators
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=from:sarah%20is:unread%20has:attachment` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with JSON response containing:
     - `parsed_operators` array has exactly 3 entries:
       - `{"key": "from", "value": "sarah"}`
       - `{"key": "is", "value": "unread"}`
       - `{"key": "has", "value": "attachment"}`
     - `query` equals `"from:sarah is:unread has:attachment"`
     - All results satisfy ALL three conditions simultaneously:
       - `from_address` or `from_name` contains "sarah"
       - `is_read` = false
       - `has_attachments` = true
     - Messages matching only 1 or 2 of the conditions are excluded

3. Verify SQL uses AND conjunction for all operators
   - **Target**: Same query as step 2
   - **Expected**: WHERE clause joins all conditions with AND (not OR), verified by absence of partial-match results

## Success Criteria
- [ ] Response status is 200
- [ ] `parsed_operators` has exactly 3 entries with correct keys and values
- [ ] All results match the from filter (contains "sarah")
- [ ] All results are unread (is_read=false)
- [ ] All results have attachments (has_attachments=true)
- [ ] Operator-only path is used (no FTS5 join) since no free text is present
- [ ] Messages matching fewer than all 3 conditions are excluded

## Failure Criteria
- Response returns non-200 status
- Fewer than 3 operators appear in `parsed_operators`
- Results include messages that do not satisfy all three conditions
- Operators are combined with OR instead of AND
