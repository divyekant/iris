# GC-207: Draft from Intent — Body Is Well-Formed Prose

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: draft-from-intent
- **Tags**: draft-from-intent, api, quality, prose
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Generate a draft from a clear intent
   - **Target**: `POST /api/ai/draft-from-intent`
   - **Input**: `{"intent": "Apologize for missing yesterday's meeting and propose rescheduling"}`
   - **Expected**: 200 OK with a `body` field containing coherent email text

2. Evaluate the body content for email structure
   - **Target**: `body` field in the response
   - **Input**: n/a
   - **Expected**: The body contains recognizable email elements: a greeting (e.g., "Hi", "Dear"), the main message addressing the intent, and a closing (e.g., "Best regards", "Thanks")

3. Verify the text is not garbled or truncated
   - **Target**: `body` field
   - **Input**: n/a
   - **Expected**: Complete sentences; no JSON artifacts, markdown syntax, or cut-off text

## Success Criteria
- [ ] `body` contains multiple sentences forming coherent prose
- [ ] Text addresses the stated intent (apology + rescheduling)
- [ ] Email has recognizable structure (greeting, body, closing)
- [ ] No JSON artifacts, raw markdown, or truncation

## Failure Criteria
- `body` is a single word or sentence fragment
- Text is garbled, contains JSON keys, or is clearly AI-failure output
- Body does not relate to the intent at all
