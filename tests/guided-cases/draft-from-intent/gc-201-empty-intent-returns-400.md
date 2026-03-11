# GC-201: Draft from Intent — Empty Intent Returns 400

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: draft-from-intent
- **Tags**: draft-from-intent, api, validation, negative
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Send request with empty intent string
   - **Target**: `POST /api/ai/draft-from-intent`
   - **Input**: `{"intent": ""}`
   - **Expected**: 400 Bad Request with an error message indicating intent is required

2. Send request with missing intent field
   - **Target**: `POST /api/ai/draft-from-intent`
   - **Input**: `{}`
   - **Expected**: 400 Bad Request (or 422 Unprocessable Entity) with an error message

## Success Criteria
- [ ] Empty intent string returns 400
- [ ] Missing intent field returns 400 or 422
- [ ] Error response includes a meaningful message about the missing/empty intent

## Failure Criteria
- Server returns 200 with an empty or nonsensical draft
- Server returns 500
- No error message in the response body
