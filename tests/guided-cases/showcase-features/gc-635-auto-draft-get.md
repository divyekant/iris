# GC-635: Auto-Draft — GET Returns Pre-Generated Draft for Matching Pattern

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: showcase-features
- **Tags**: auto-draft, pattern, pre-generated, draft, routine
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- AI provider available

### Data
- A message exists that matches a known routine pattern (e.g., a recurring meeting request, a standard invoice acknowledgement)
- The auto-draft system has already processed this message and stored a pre-generated draft
- `message_id` of this message is known

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Fetch pre-generated auto-draft for the message
   - **Target**: `GET http://localhost:3030/api/auto-draft/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`, path param `message_id`
   - **Expected**: 200 OK, response contains `draft` object

3. Verify draft structure
   - **Target**: `draft` from step 2
   - **Input**: inspect fields
   - **Expected**: `draft` has `id`, `message_id`, `subject`, `body`, `pattern_id`, `confidence` (float 0–1)

4. Verify confidence reflects a matched pattern
   - **Target**: `draft.confidence` from step 2
   - **Input**: compare against threshold
   - **Expected**: `confidence` ≥ 0.6 (indicating a reasonably strong pattern match)

## Success Criteria
- [ ] GET /api/auto-draft/{message_id} returns 200 OK
- [ ] `draft.id` is present and non-null
- [ ] `draft.body` is non-empty
- [ ] `draft.pattern_id` references a known pattern
- [ ] `draft.confidence` ≥ 0.6

## Failure Criteria
- 404 if no auto-draft exists for the message (acceptable if no pattern matched, but test requires a matching message)
- `draft.body` is empty
- `confidence` = 0 or missing
- Draft references non-existent `pattern_id`
