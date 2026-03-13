# GC-326: Max 5 suggestions returned

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: cc-suggestions
- **Tags**: cc-suggestions, edge-case, result-cap, max-suggestions
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and reachable

### Data
- A `to` recipient who co-occurs with 8 or more distinct contacts across threads (source: seed or real inbox with a high-traffic sender)
- All 8+ co-occurring contacts are NOT already in `to` or `cc`

## Steps
1. POST to suggest-cc for a high-co-occurrence recipient
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": ["alice@example.com"],
       "cc": [],
       "subject": "All-hands prep",
       "body_preview": "Getting everyone aligned before the meeting."
     }
     ```
   - **Expected**: 200 OK — even if 8+ candidates exist, the `suggestions` array has at most 5 elements

2. Count suggestions and verify cap
   - **Target**: `suggestions` array length
   - **Input**: `suggestions.length`
   - **Expected**: `suggestions.length <= 5`

3. Verify the 5 returned are highest-confidence ones
   - **Target**: `confidence` values of returned suggestions vs. known candidate pool
   - **Input**: Descending confidence ordering
   - **Expected**: The top 5 by confidence are returned (the AI selects the most relevant, not arbitrary 5)

## Success Criteria
- [ ] Response status is 200
- [ ] `suggestions.length` is at most 5 even when more candidates exist
- [ ] Returned suggestions are ordered by descending confidence
- [ ] The cap does not cause a 500 or truncated JSON response

## Failure Criteria
- More than 5 suggestions returned
- Response body malformed or truncated when many candidates exist
- Non-200 status code

## Notes
The AI is instructed to return max 5 suggestions. This cap keeps the UI compact and forces the AI to prioritize. The test requires a seed scenario with 8+ co-occurring contacts to confirm the cap is enforced. If the test environment has fewer co-occurring contacts, adapt by verifying that the count never exceeds 5 for whatever count is present.
