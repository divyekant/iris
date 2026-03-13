# GC-331: Strikethrough supported

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: markdown-compose
- **Tags**: markdown, preview, strikethrough, gfm
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No pre-existing data required (source: inline)

## Steps

1. Send a markdown preview request using GFM strikethrough syntax (`~~text~~`)
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "This is ~~deleted~~ text and this is normal."
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with JSON body containing an `html` field

2. Verify the strikethrough element is rendered
   - **Target**: `html` field in response body
   - **Expected**: Contains `<del>deleted</del>` (or `<s>deleted</s>`)

3. Verify surrounding text is preserved as plain text
   - **Target**: `html` field in response body
   - **Expected**: "This is" and "text and this is normal." appear outside the strikethrough tags

## Success Criteria
- [ ] Response status is 200
- [ ] `<del>deleted</del>` (or `<s>deleted</s>`) is present in the HTML output
- [ ] The tilde characters (`~~`) do not appear verbatim in the output
- [ ] Non-struck text is rendered correctly in surrounding paragraph

## Failure Criteria
- Response status is not 200
- Raw `~~deleted~~` syntax is present in the output instead of an HTML tag
- Strikethrough tag is absent entirely
