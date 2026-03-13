# GC-334: Iframe tags stripped

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: markdown-compose
- **Tags**: markdown, preview, sanitization, xss, iframe-injection
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No pre-existing data required (source: inline)

## Steps

1. Send markdown that embeds an inline `<iframe>` tag
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "Check this out:\n\n<iframe src=\"https://evil.example.com\"></iframe>\n\nEnd."
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with JSON body containing an `html` field

2. Verify the iframe tag is absent from the output
   - **Target**: `html` field in response body
   - **Expected**: Does not contain `<iframe` or `</iframe>`

3. Send markdown with a multiline `<iframe>` block
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "Before\n\n<iframe\n  src=\"https://evil.example.com\"\n  width=\"100%\"\n  height=\"400\">\n</iframe>\n\nAfter"
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with the `<iframe>` block fully absent from the `html` field

4. Confirm surrounding content is preserved
   - **Target**: `html` field in response body from both requests
   - **Expected**: Text flanking the iframe ("Check this out:", "End.", "Before", "After") appears in the output

## Success Criteria
- [ ] Response status is 200 for both requests
- [ ] Neither `<iframe` nor `</iframe>` appears anywhere in the `html` output (case-insensitive)
- [ ] The `src` URL is not rendered or exposed in the output
- [ ] Surrounding paragraph text is preserved

## Failure Criteria
- Response contains `<iframe` or `</iframe>` in any form
- The external `src` URL appears in the HTML output
- Server returns an error instead of sanitized HTML

## Notes
Multiline iframe stripping is explicitly required by the implementation spec alongside script tag stripping.
