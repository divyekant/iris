# GC-325: Confidence values are 0.0 to 1.0

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: cc-suggestions
- **Tags**: cc-suggestions, confidence, response-contract, range-validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and reachable

### Data
- At least one synced account with multiple threads and co-occurring contacts (source: prior sync)
- A `to` recipient with at least 3 co-occurring contacts so multiple suggestions are returned (source: seed or real inbox)

## Steps
1. POST to suggest-cc and collect multiple suggestions
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": ["alice@example.com"],
       "cc": [],
       "subject": "Team sync",
       "body_preview": "Wanted to loop in the relevant people for tomorrow's planning session."
     }
     ```
   - **Expected**: 200 OK with at least 2 suggestions in the `suggestions` array

2. Validate `confidence` range for each suggestion
   - **Target**: `confidence` field on each suggestion
   - **Input**: Numeric float values
   - **Expected**: All `confidence` values satisfy `0.0 <= confidence <= 1.0`

3. Validate suggestions are ordered by confidence descending
   - **Target**: `suggestions` array ordering
   - **Input**: Sequence of `confidence` values
   - **Expected**: `suggestions[0].confidence >= suggestions[1].confidence >= ...` (highest confidence first)

4. Verify confidence is a float, not an integer or string
   - **Target**: JSON type of `confidence`
   - **Input**: Raw JSON value
   - **Expected**: JSON number (may be `0.9` or `0.90`) — not a quoted string like `"0.9"` and not an integer like `1`

## Success Criteria
- [ ] All `confidence` values are in the range [0.0, 1.0] inclusive
- [ ] Suggestions are ordered by descending confidence
- [ ] `confidence` is a JSON number, not a string
- [ ] No `null` or missing `confidence` field on any suggestion

## Failure Criteria
- Any `confidence` value outside [0.0, 1.0]
- `confidence` returned as a string
- `confidence` field absent or null on a suggestion
- Suggestions not ordered by descending confidence

## Notes
The confidence field conveys the AI's certainty about each suggestion. Values outside [0.0, 1.0] indicate a normalization bug in the AI response parser. Descending order ensures the UI can display the most actionable suggestions at the top without needing to re-sort client-side.
