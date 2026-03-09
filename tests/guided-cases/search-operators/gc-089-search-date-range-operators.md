# gc-search-004: Search with Date Range Operators

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: search-operators
- **Tags**: search, operator, after, before, date, range, fts5
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- At least one message with `date` timestamp within 2026-01-01 to 2026-02-28 containing "report" in subject or body
- At least one message with `date` outside that range (e.g., before 2026-01-01 or after 2026-03-01) containing "report"
- Messages indexed in FTS5

## Steps

1. Obtain session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap` with header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `{"token": "<session_token>"}`

2. Search with date range and free text
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=after:2026-01-01%20before:2026-03-01%20report` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with JSON response containing:
     - `parsed_operators` array has 2 entries: `{"key": "after", "value": "2026-01-01"}` and `{"key": "before", "value": "2026-03-01"}`
     - `query` equals `"after:2026-01-01 before:2026-03-01 report"`
     - All results have `date` >= 2026-01-01T00:00:00 local timestamp AND `date` <= 2026-03-01T23:59:59 local timestamp
     - Results match FTS5 text "report"

3. Verify relative date "today" works
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=before:today` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with `parsed_operators` including `{"key": "before", "value": "today"}`; all results have date <= end of today (23:59:59 local)

4. Verify relative date "yesterday" works
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=after:yesterday` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with `parsed_operators` including `{"key": "after", "value": "yesterday"}`; all results have date >= start of yesterday (00:00:00 local)

## Success Criteria
- [ ] Response status is 200 for all queries
- [ ] `parsed_operators` correctly includes both after and before entries for the range query
- [ ] All results in the range query fall within the specified date boundaries
- [ ] Messages outside the date range are excluded
- [ ] Relative dates "today" and "yesterday" are parsed to valid timestamps
- [ ] `after:` uses start of day (00:00:00), `before:` uses end of day (23:59:59)

## Failure Criteria
- Response returns non-200 status
- Results include messages outside the specified date range
- Relative date values ("today", "yesterday") fail to parse
- Date parsing produces incorrect timestamps (wrong timezone or boundary)
