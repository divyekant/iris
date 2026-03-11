# GC-211: Grammar Check — Empty Content Returns 400

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: grammar-check
- **Tags**: grammar-check, api, validation, negative
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Send grammar check with empty content string
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"content": ""}`
   - **Expected**: 400 Bad Request with error message indicating content is required

2. Send grammar check with missing content field
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{}`
   - **Expected**: 400 Bad Request (or 422 Unprocessable Entity)

## Success Criteria
- [ ] Empty content string returns 400
- [ ] Missing content field returns 400 or 422
- [ ] Error response includes a meaningful message

## Failure Criteria
- Server returns 200 with empty or default results
- Server returns 500
- No error message in the response body
