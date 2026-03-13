# GC-321: Empty to list returns empty suggestions

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: cc-suggestions
- **Tags**: cc-suggestions, edge-case, empty-input, validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and reachable

### Data
- No specific data required

## Steps
1. POST to suggest-cc with an empty `to` array
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": [],
       "cc": [],
       "subject": "Project update",
       "body_preview": "Here's the latest."
     }
     ```
   - **Expected**: 200 OK with `{ "suggestions": [] }` (empty array), OR 400 Bad Request with a validation error message

2. If 200: verify suggestions is an empty array
   - **Target**: Response JSON
   - **Input**: `suggestions` value
   - **Expected**: `[]` — no suggestions fabricated without any recipients to anchor co-occurrence lookup

3. If 400: verify error message is informative
   - **Target**: Response JSON
   - **Input**: Error body
   - **Expected**: Message describes that `to` must be non-empty

## Success Criteria
- [ ] Response is either 200 with `{ "suggestions": [] }` or 400 with a clear error
- [ ] No suggestions are fabricated without a `to` anchor
- [ ] Response body is valid JSON in either case

## Failure Criteria
- 500 Internal Server Error
- Suggestions returned that have no basis (no `to` recipient to anchor co-occurrence)
- Response is not valid JSON

## Notes
Co-occurrence lookup is anchored to the `to` recipients. Without any `to` addresses there is no meaningful basis for suggestions. The endpoint should either return an empty result gracefully or reject the request with 400.
