# gc-search-005: Search with subject: Operator and Quoted Value

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: search-operators
- **Tags**: search, operator, subject, quoted, parsing
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- At least one message with subject containing the exact phrase "quarterly report"
- At least one message with subject containing "quarterly" but not "quarterly report"

## Steps

1. Obtain session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap` with header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `{"token": "<session_token>"}`

2. Search with quoted subject operator
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=subject:"quarterly report"` (URL-encoded: `subject:%22quarterly%20report%22`) with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with JSON response containing:
     - `parsed_operators` array includes `{"key": "subject", "value": "quarterly report"}` (quotes stripped from value)
     - `query` equals `subject:"quarterly report"`
     - All results have `subject` matching LIKE `%quarterly report%` (case-insensitive)
     - No free text is extracted (operator-only query path)

3. Verify unquoted subject operator for comparison
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=subject:quarterly` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with `parsed_operators` including `{"key": "subject", "value": "quarterly"}`; results match LIKE `%quarterly%`

## Success Criteria
- [ ] Response status is 200
- [ ] Quoted value "quarterly report" is parsed as a single operator value (not split into separate tokens)
- [ ] `parsed_operators` value has quotes stripped: "quarterly report" not "\"quarterly report\""
- [ ] SQL LIKE filter uses the full phrase `%quarterly report%`
- [ ] Operator-only path is used (no FTS5 join) since there is no free text

## Failure Criteria
- Quoted value is split at the space, producing two separate tokens
- Quotes are retained in the parsed operator value
- Response returns non-200 status
- Results do not match the subject phrase
