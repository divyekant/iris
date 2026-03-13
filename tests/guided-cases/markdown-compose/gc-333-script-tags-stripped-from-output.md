# GC-333: Script tags stripped from output

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: markdown-compose
- **Tags**: markdown, preview, sanitization, xss, script-injection
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No pre-existing data required (source: inline)

## Steps

1. Send markdown that embeds a `<script>` tag inline
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "Hello\n\n<script>alert('xss')</script>\n\nWorld"
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with JSON body containing an `html` field

2. Verify the script tag is absent from the output
   - **Target**: `html` field in response body
   - **Expected**: Does not contain `<script` or `</script>`

3. Send markdown with a multiline `<script>` block
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "Hello\n\n<script>\nvar x = 1;\nalert(x);\n</script>\n\nWorld"
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with the `<script>` block fully absent from the `html` field

4. Confirm surrounding content is preserved
   - **Target**: `html` field in response body from both requests
   - **Expected**: "Hello" and "World" still appear in the output (content outside the script tag is not discarded)

## Success Criteria
- [ ] Response status is 200 for both requests
- [ ] Neither `<script>` nor `</script>` appears anywhere in the `html` output (case-insensitive)
- [ ] `alert` payload is not present in the output
- [ ] Surrounding paragraph text ("Hello", "World") is preserved

## Failure Criteria
- Response contains `<script` or `</script>` in any form
- `alert('xss')` or any JS payload is present in the HTML output
- Server returns an error instead of sanitized HTML

## Notes
Multiline script tag stripping is explicitly required by the implementation spec. Both inline and multiline variants must be verified.
