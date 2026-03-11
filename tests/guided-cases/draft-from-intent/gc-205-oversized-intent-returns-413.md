# GC-205: Draft from Intent — Oversized Intent Returns 413

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: draft-from-intent
- **Tags**: draft-from-intent, api, validation, size-limit, negative
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Send a draft intent with a body exceeding 50KB
   - **Target**: `POST /api/ai/draft-from-intent`
   - **Input**: `{"intent": "<51200+ character string>"}` — generate a string of 51,200 'a' characters
   - **Expected**: 413 Payload Too Large (or 400 Bad Request with size-related error message)

2. Verify the server does not process the oversized input
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: Error message about input size; no draft is generated

## Success Criteria
- [ ] Response status is 413 or 400 with a clear size-limit error
- [ ] No draft content is generated or returned
- [ ] Server remains responsive after the request

## Failure Criteria
- Server returns 200 and processes the oversized intent
- Server returns 500 or hangs
- No error message about size in the response
