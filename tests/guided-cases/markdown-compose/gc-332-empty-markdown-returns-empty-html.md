# GC-332: Empty markdown returns empty HTML

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: markdown-compose
- **Tags**: markdown, preview, empty-input, edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No pre-existing data required (source: inline)

## Steps

1. Send a markdown preview request with an empty string
   - **Target**: `POST /api/compose/markdown-preview`
   - **Input**:
     ```json
     {
       "markdown": ""
     }
     ```
   - **Headers**: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
   - **Expected**: HTTP 200 with JSON body containing an `html` field

2. Verify the HTML output is empty or whitespace-only
   - **Target**: `html` field in response body
   - **Expected**: `html` value is `""` or consists entirely of whitespace characters

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains an `html` field
- [ ] `html` value is an empty string or whitespace-only (no HTML tags generated)
- [ ] No error is returned for empty input

## Failure Criteria
- Response status is not 200
- Server returns an error (4xx or 5xx) for empty input
- `html` field contains spurious HTML elements not derived from input

## Notes
Empty string is a valid use case — the compose window may be cleared and a preview re-requested mid-session. The endpoint must handle this gracefully without error.
