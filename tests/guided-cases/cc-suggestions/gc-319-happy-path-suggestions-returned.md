# GC-319: Happy path — suggestions returned for known recipients

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: cc-suggestions
- **Tags**: cc-suggestions, happy-path, co-occurrence, ai-reasoning
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and reachable

### Data
- At least one synced account with multiple threads (source: prior inbox sync)
- A known sender (`alice@example.com`) who shares threads with at least one other contact (`bob@example.com`) (source: seed or real inbox)

## Steps
1. POST to suggest-cc with a known `to` recipient
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": ["alice@example.com"],
       "cc": [],
       "subject": "Project update",
       "body_preview": "Here's the latest on the Q2 roadmap and upcoming milestones."
     }
     ```
   - **Expected**: 200 OK, response body contains `{ "suggestions": [...] }` with at least one suggestion

2. Verify suggestion object shape
   - **Target**: Response JSON inspection
   - **Input**: Each element of `suggestions`
   - **Expected**: Each suggestion has `email` (string), `name` (string, may be empty), `reason` (non-empty string), `confidence` (float), `type` (`"cc"` or `"bcc"`)

3. Verify reason is human-readable
   - **Target**: `reason` field on first suggestion
   - **Input**: String value
   - **Expected**: Contains a meaningful phrase such as "frequently appears in threads with" or "past collaborator" — not empty and not a code identifier

## Success Criteria
- [ ] Response status is 200
- [ ] `suggestions` array is present and non-empty
- [ ] Each suggestion includes `email`, `name`, `reason`, `confidence`, and `type`
- [ ] `type` is either `"cc"` or `"bcc"`
- [ ] `reason` is a non-empty human-readable string
- [ ] No duplicate emails in the suggestions list

## Failure Criteria
- Non-200 status code
- `suggestions` key absent from response
- Suggestions returned with missing required fields
- Duplicate email addresses across suggestions

## Notes
Primary happy path. Confirms that the full pipeline (co-occurrence query → AI reasoning → structured response) works end-to-end for a `to` recipient with real co-occurrence history.
