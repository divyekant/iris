# GC-204: Draft from Intent — XSS in Intent Handled Safely

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: draft-from-intent
- **Tags**: draft-from-intent, api, security, xss
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Send a draft intent containing XSS payload
   - **Target**: `POST /api/ai/draft-from-intent`
   - **Input**: `{"intent": "<script>alert('xss')</script>Ask about project"}`
   - **Expected**: 200 OK or 400 Bad Request; the response does not execute or reflect raw script tags in a dangerous way

2. Inspect the response body
   - **Target**: `subject` and `body` fields in the response
   - **Input**: n/a
   - **Expected**: If 200, the `<script>` tag is not present verbatim in subject or body; AI may have ignored or sanitized the HTML. If 400, the input was rejected as invalid.

3. Verify server stability
   - **Target**: `GET /api/health`
   - **Input**: n/a
   - **Expected**: 200 OK

## Success Criteria
- [ ] Server does not crash or return 500
- [ ] Raw `<script>` tags are not reflected in the generated subject or body
- [ ] Server remains healthy after the request

## Failure Criteria
- `<script>` tag appears verbatim in subject or body
- Server returns 500 or crashes
- XSS payload is executable in the response context
