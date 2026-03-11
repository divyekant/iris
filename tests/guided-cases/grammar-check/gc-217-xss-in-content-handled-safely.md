# GC-217: Grammar Check — XSS in Content Handled Safely

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: grammar-check
- **Tags**: grammar-check, api, security, xss
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Send content with XSS payload
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"content": "<img src=x onerror=alert('xss')>Please check this email draft."}`
   - **Expected**: 200 OK or 400 Bad Request; the response does not reflect raw HTML/script in a dangerous way

2. Inspect the response fields
   - **Target**: `improved_content`, `issues`, and other response fields
   - **Input**: n/a
   - **Expected**: If 200, the XSS payload is not present verbatim in `improved_content` or issue descriptions; AI treats it as text or it is sanitized

3. Verify server stability
   - **Target**: `GET /api/health`
   - **Input**: n/a
   - **Expected**: 200 OK

## Success Criteria
- [ ] Server does not crash or return 500
- [ ] XSS payload is not reflected in executable form in the response
- [ ] Server remains healthy after the request

## Failure Criteria
- `<img src=x onerror=alert('xss')>` appears verbatim in response fields
- Server returns 500 or crashes
- XSS payload could be executed if response is rendered in a browser
