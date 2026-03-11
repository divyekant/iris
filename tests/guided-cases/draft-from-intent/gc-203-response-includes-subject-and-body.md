# GC-203: Draft from Intent — Response Includes Subject and Body

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: draft-from-intent
- **Tags**: draft-from-intent, api, schema, response-format
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Generate a draft from intent
   - **Target**: `POST /api/ai/draft-from-intent`
   - **Input**: `{"intent": "Invite the team to a brainstorming session on Wednesday"}`
   - **Expected**: 200 OK with JSON body

2. Verify subject field
   - **Target**: `subject` key in response
   - **Input**: n/a
   - **Expected**: Non-empty string; relevant to the intent (e.g., contains "brainstorming", "meeting", "Wednesday", or similar)

3. Verify body field
   - **Target**: `body` key in response
   - **Input**: n/a
   - **Expected**: Non-empty string; well-formed email prose (greeting, content, sign-off)

## Success Criteria
- [ ] Response contains `subject` as a non-empty string
- [ ] Response contains `body` as a non-empty string
- [ ] `subject` is relevant to the stated intent
- [ ] `body` reads as a plausible email

## Failure Criteria
- `subject` or `body` is missing from the response
- Either field is an empty string
- `subject` is unrelated to the intent (e.g., generic "Hello")
