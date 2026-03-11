# GC-218: Grammar Check — Large Content Returns 413

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: grammar-check
- **Tags**: grammar-check, api, validation, size-limit, negative
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Send grammar check with content exceeding 50KB
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"content": "<51200+ character string>"}` — generate a string of 51,200 characters
   - **Expected**: 413 Payload Too Large (or 400 Bad Request with size-related error message)

2. Verify the server does not process the oversized input
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: Error message about content size; no grammar check is performed

3. Verify server remains responsive
   - **Target**: `GET /api/health`
   - **Input**: n/a
   - **Expected**: 200 OK

## Success Criteria
- [ ] Response status is 413 or 400 with a clear size-limit error
- [ ] No grammar check results are returned
- [ ] Server remains responsive after the request

## Failure Criteria
- Server returns 200 and processes the oversized content
- Server returns 500 or hangs
- No error message about size in the response
