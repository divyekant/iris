# GC-336: javascript: URLs neutralized in links

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: markdown-compose
- **Tags**: markdown, preview, sanitization, xss, javascript-url
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No pre-existing data required (source: inline)

## Steps

1. Send markdown with a `javascript:` URL in a markdown link
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "[Click me](javascript:alert('xss'))"
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with JSON body containing an `html` field

2. Verify the `javascript:` scheme is absent from the `href`
   - **Target**: `html` field in response body
   - **Expected**: Does not contain `href="javascript:` (case-insensitive)

3. Send markdown with a `javascript:` URL embedded directly in an HTML `<a>` tag
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "<a href=\"javascript:alert('xss')\">raw link</a>"
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with the `javascript:` href stripped or the attribute removed

4. Verify the `javascript:` scheme is absent from the raw-HTML link output
   - **Target**: `html` field in response body
   - **Expected**: Does not contain `javascript:` anywhere in the output

5. Verify that legitimate `https://` links are not affected
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "[Safe link](https://example.com)"
     }
     ```
   - **Expected**: `html` contains `href="https://example.com"` intact

## Success Criteria
- [ ] Response status is 200 for all requests
- [ ] `javascript:` does not appear anywhere in the HTML output for steps 1–4 (case-insensitive)
- [ ] The link text ("Click me", "raw link") may still be rendered as text or a neutralized anchor
- [ ] Legitimate `https://` href from step 5 is preserved unchanged

## Failure Criteria
- `javascript:` appears in any `href` attribute in the output
- The JS payload executes or is otherwise present in the HTML
- Legitimate HTTPS links are stripped or broken by the sanitizer

## Notes
Sanitization must cover both markdown-syntax links (`[text](javascript:...)`) and raw HTML anchor tags with `javascript:` hrefs.
