# GC-323: No co-occurrences found returns empty suggestions

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: cc-suggestions
- **Tags**: cc-suggestions, edge-case, no-data, co-occurrence
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and reachable

### Data
- An email address that has never appeared in any synced thread (e.g., `unknown-new-contact@nowhere.example`) (source: use an address guaranteed to have no history)

## Steps
1. POST to suggest-cc with a `to` recipient that has no co-occurrence history
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": ["unknown-new-contact@nowhere.example"],
       "cc": [],
       "subject": "First contact",
       "body_preview": "Reaching out for the first time."
     }
     ```
   - **Expected**: 200 OK with `{ "suggestions": [] }` — no suggestions fabricated from thin air

2. Verify suggestions array is empty
   - **Target**: Response JSON `suggestions` field
   - **Input**: Array value
   - **Expected**: `[]` — empty array, no hallucinated contacts

## Success Criteria
- [ ] Response status is 200
- [ ] `suggestions` is an empty array
- [ ] No contacts are suggested that have no co-occurrence basis

## Failure Criteria
- Non-200 status code
- Non-empty suggestions array when no co-occurrence data exists
- 500 Internal Server Error

## Notes
When the co-occurrence query returns no candidates, the AI has nothing to reason about. The endpoint should return an empty list rather than hallucinating contacts. This guards against the AI inventing suggestions from subject/body content alone without a data anchor.
