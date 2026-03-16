# GC-639: Evolving Categories — POST /api/categories/analyze Suggests New Categories from Behavior

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api+ui
- **Flow**: showcase-features
- **Tags**: evolving-categories, suggestions, behavior, ai, inbox
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- AI provider available

### Data
- Account `{account_id}` has ≥ 20 messages spread across multiple behavioral clusters (e.g., a set of GitHub notifications, a set of financial summaries, a set of HR announcements)
- No custom categories currently exist for this account

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Trigger category analysis
   - **Target**: `POST http://localhost:3030/api/categories/analyze/{account_id}`
   - **Input**: Header `X-Session-Token: {token}`, path param `account_id`
   - **Expected**: 200 OK, response contains `suggestions` array

3. Verify suggestions structure
   - **Target**: `suggestions` from step 2
   - **Input**: inspect each suggestion
   - **Expected**: each suggestion has `name` (human-readable label), `description` (rationale), `message_count` (how many messages fit this category), `confidence` (float 0–1)

4. Verify suggestions are non-trivial
   - **Target**: `suggestions` from step 2
   - **Input**: check count and confidence
   - **Expected**: at least 1 suggestion with `confidence` ≥ 0.5 and `message_count` ≥ 3

5. Accept one suggestion to create a custom category
   - **Target**: `POST http://localhost:3030/api/categories`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     { "name": "{suggestion.name}", "account_id": "{account_id}", "source": "ai_suggestion" }
     ```
   - **Expected**: 201 Created, custom category created

## Success Criteria
- [ ] POST /api/categories/analyze/{account_id} returns 200 OK
- [ ] `suggestions` array has ≥ 1 entry
- [ ] Each suggestion has `name`, `description`, `message_count`, `confidence`
- [ ] At least one suggestion has `confidence` ≥ 0.5
- [ ] Accepting a suggestion via POST /api/categories creates a custom category

## Failure Criteria
- 404 if account not found
- Empty `suggestions` array when ≥ 20 varied messages exist
- Suggestions missing required fields
- `confidence` always 0 or 1 (not meaningful)
