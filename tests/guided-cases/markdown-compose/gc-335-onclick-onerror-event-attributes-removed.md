# GC-335: onclick/onerror event attributes removed

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: markdown-compose
- **Tags**: markdown, preview, sanitization, xss, event-handlers
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No pre-existing data required (source: inline)

## Steps

1. Send markdown embedding an HTML element with an `onclick` event attribute
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "Click me: <a href=\"https://example.com\" onclick=\"alert('xss')\">link</a>"
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with JSON body containing an `html` field

2. Verify `onclick` attribute is absent from the output
   - **Target**: `html` field in response body
   - **Expected**: Does not contain `onclick`

3. Send markdown embedding an `<img>` tag with an `onerror` event attribute
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "Image: <img src=\"broken.png\" onerror=\"alert('xss')\">"
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with the `onerror` attribute stripped from the output

4. Verify `onerror` attribute is absent from the output
   - **Target**: `html` field in response body
   - **Expected**: Does not contain `onerror`

5. Verify safe attributes on the same elements are retained (optional)
   - **Target**: `html` field in response body from step 1
   - **Expected**: The `href` attribute on the `<a>` tag is still present (only the `on*` attribute was stripped)

## Success Criteria
- [ ] Response status is 200 for both requests
- [ ] `onclick` does not appear in the HTML output from step 1
- [ ] `onerror` does not appear in the HTML output from step 3
- [ ] No `on*` event handler attribute values (e.g., `alert(...)`) appear in the output
- [ ] Non-event attributes (`href`) are preserved on the same elements

## Failure Criteria
- Response contains any `on*` attribute name in the HTML output
- JS payload from event handlers is rendered or embedded in any form
- Server returns an error instead of sanitized HTML

## Notes
The sanitizer must remove all `on*` attributes (`onclick`, `onerror`, `onload`, `onmouseover`, etc.). This case spot-checks two of the most common attack vectors.
