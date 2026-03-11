# GC-200: Draft from Intent with Optional Context

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: draft-from-intent
- **Tags**: draft-from-intent, api, context, ai
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Generate a draft with an intent and context string
   - **Target**: `POST /api/ai/draft-from-intent`
   - **Input**: `{"intent": "Follow up on the project deadline", "context": "We discussed the Q3 launch timeline in yesterday's standup. The deadline was moved from June 15 to June 30."}`
   - **Expected**: 200 OK with JSON body; the generated `body` reflects details from the context

2. Verify context influenced the output
   - **Target**: Response `body` field
   - **Input**: n/a
   - **Expected**: The body references specifics from the context (e.g., Q3, June, deadline) rather than being purely generic

## Success Criteria
- [ ] Response status is 200
- [ ] `body` text reflects context details (dates, project references, or similar specifics)
- [ ] `subject` is relevant to the intent
- [ ] `suggested_to` is present as an array

## Failure Criteria
- Response status is not 200
- Generated body ignores the context entirely
- Context is echoed verbatim without being incorporated into email prose
