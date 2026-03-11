# GC-199: Draft from Intent — Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: draft-from-intent
- **Tags**: draft-from-intent, api, happy-path, ai
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Generate an email draft from a plain English intent
   - **Target**: `POST /api/ai/draft-from-intent`
   - **Input**: `{"intent": "Ask my manager for a day off next Friday"}`
   - **Expected**: 200 OK with JSON body containing `subject`, `body`, and `suggested_to` fields

2. Verify the response is a well-formed draft
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: `subject` is a non-empty string; `body` is a non-empty string with coherent email prose; `suggested_to` is an array

## Success Criteria
- [ ] Response status is 200
- [ ] `subject` is a non-empty string relevant to the intent
- [ ] `body` is a non-empty string forming a coherent email
- [ ] `suggested_to` is present as an array

## Failure Criteria
- Response status is not 200
- `subject` or `body` is empty or missing
- Response body is not valid JSON
