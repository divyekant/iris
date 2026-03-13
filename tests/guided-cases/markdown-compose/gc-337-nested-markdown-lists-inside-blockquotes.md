# GC-337: Nested markdown (lists inside blockquotes)

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: markdown-compose
- **Tags**: markdown, preview, blockquote, lists, nesting
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No pre-existing data required (source: inline)

## Steps

1. Send markdown with an unordered list nested inside a blockquote
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "> Action items:\n>\n> - Buy groceries\n> - Send report\n> - Schedule meeting"
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with JSON body containing an `html` field

2. Verify the blockquote wrapper is present
   - **Target**: `html` field in response body
   - **Expected**: Contains `<blockquote>`

3. Verify the unordered list is rendered inside the blockquote
   - **Target**: `html` field in response body
   - **Expected**: Contains `<ul>` nested within the `<blockquote>` element

4. Verify each list item is rendered as `<li>`
   - **Target**: `html` field in response body
   - **Expected**: Contains `<li>Buy groceries</li>`, `<li>Send report</li>`, `<li>Schedule meeting</li>`

## Success Criteria
- [ ] Response status is 200
- [ ] `<blockquote>` element is present in the HTML output
- [ ] `<ul>` is present and nested inside the `<blockquote>`
- [ ] All three `<li>` items are present with their text content intact
- [ ] No raw `>` or `-` markdown syntax appears verbatim in the output

## Failure Criteria
- Response status is not 200
- `<blockquote>` is absent — content rendered as plain text
- `<ul>` is absent or positioned outside the blockquote
- Any list item text is missing or mangled
