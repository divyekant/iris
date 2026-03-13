# GC-330: Tables rendered correctly

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: markdown-compose
- **Tags**: markdown, preview, tables, gfm
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No pre-existing data required (source: inline)

## Steps

1. Send a markdown preview request containing a GFM table
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": "| Name  | Role    |\n|-------|----------|\n| Alice | Engineer |\n| Bob   | Designer |"
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with JSON body containing an `html` field

2. Verify the table wrapper element is present
   - **Target**: `html` field in response body
   - **Expected**: Contains `<table>`

3. Verify header row is rendered as `<th>` cells
   - **Target**: `html` field in response body
   - **Expected**: Contains `<th>Name</th>` and `<th>Role</th>`

4. Verify data rows are rendered as `<td>` cells
   - **Target**: `html` field in response body
   - **Expected**: Contains `<td>Alice</td>`, `<td>Engineer</td>`, `<td>Bob</td>`, `<td>Designer</td>`

## Success Criteria
- [ ] Response status is 200
- [ ] `<table>` tag is present in the HTML output
- [ ] `<thead>` or `<th>` elements render the header row with "Name" and "Role"
- [ ] `<tbody>` or `<td>` elements render both data rows correctly
- [ ] No raw pipe characters (`|`) or dashes remain in the output as literal text

## Failure Criteria
- Response status is not 200
- Output contains raw markdown table syntax instead of HTML table elements
- `<table>` element is absent
- Header cells are not wrapped in `<th>` or equivalent elements
