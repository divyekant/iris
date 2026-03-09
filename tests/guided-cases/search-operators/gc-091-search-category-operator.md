# gc-search-006: Search with category: Operator

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: search-operators
- **Tags**: search, operator, category, ai, case-insensitive
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- At least one message with `ai_category` set to "Promotions" (or any casing variant)
- At least one message with a different `ai_category` (e.g., "Updates")

## Steps

1. Obtain session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap` with header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `{"token": "<session_token>"}`

2. Search with category operator (lowercase)
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=category:promotions` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with JSON response containing:
     - `parsed_operators` array includes `{"key": "category", "value": "promotions"}`
     - `query` equals `"category:promotions"`
     - All results have `ai_category` equal to "promotions" (case-insensitive comparison via `LOWER()`)
     - Messages with other categories are excluded

3. Verify case-insensitivity
   - **Target**: `GET http://127.0.0.1:3000/api/search?q=category:PROMOTIONS` with header `X-Session-Token: <session_token>`
   - **Expected**: 200 OK with same result set as step 2; SQL uses `LOWER(m.ai_category) = LOWER(?)` for comparison

## Success Criteria
- [ ] Response status is 200
- [ ] `parsed_operators` includes category operator with correct value
- [ ] All results match the specified category (case-insensitive)
- [ ] Messages with different categories are excluded
- [ ] Uppercase and lowercase queries return the same results
- [ ] Operator-only path is used (no FTS5 join) since there is no free text

## Failure Criteria
- Response returns non-200 status
- Category matching is case-sensitive (uppercase query returns different results)
- Results include messages from other categories
- `parsed_operators` is empty
