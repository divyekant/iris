# GC-329: Happy path — headings, bold, italic, links

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: markdown-compose
- **Tags**: markdown, preview, headings, bold, italic, links, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No pre-existing data required (source: inline)

## Steps

1. Send a markdown preview request with headings, bold, italic, and a link
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "# Hello World\n\n## Subheading\n\nSome **bold** and *italic* text.\n\n[Visit Iris](https://iris.example.com)"
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with JSON body containing an `html` field

2. Verify heading elements are present
   - **Target**: `html` field in response body
   - **Expected**: Contains `<h1>Hello World</h1>` and `<h2>Subheading</h2>`

3. Verify inline formatting elements are present
   - **Target**: `html` field in response body
   - **Expected**: Contains `<strong>bold</strong>` and `<em>italic</em>`

4. Verify hyperlink is rendered with correct href
   - **Target**: `html` field in response body
   - **Expected**: Contains `<a href="https://iris.example.com">Visit Iris</a>`

## Success Criteria
- [ ] Response status is 200
- [ ] Response body has an `html` field (string)
- [ ] `<h1>Hello World</h1>` is present in the HTML output
- [ ] `<h2>Subheading</h2>` is present in the HTML output
- [ ] `<strong>bold</strong>` is present in the HTML output
- [ ] `<em>italic</em>` is present in the HTML output
- [ ] `<a href="https://iris.example.com">Visit Iris</a>` is present in the HTML output

## Failure Criteria
- Response status is not 200
- `html` field is absent or null
- Any of the expected HTML elements are missing or malformed
- Raw markdown syntax (e.g., `**`, `*`, `#`) appears verbatim in the output
