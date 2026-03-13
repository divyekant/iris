# GC-405: Edge — Punycode/IDN Homograph Attack Domain Flagged as Danger

## Metadata
- **Type**: edge
- **Priority**: P0
- **Surface**: api
- **Flow**: link-safety
- **Tags**: links, safety, scanning, homograph, punycode, idn, danger, edge
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose HTML body contains homograph / IDN links:
  - Punycode encoding of a lookalike: `<a href="http://xn--googIe-b2a.com/verify">Verify account</a>` (capital I instead of lowercase l)
  - Unicode lookalike stored as punycode: `<a href="http://xn--pple-43d.com">Apple</a>` (à instead of a, lookalike for apple.com)
- The message ID is known as `{msg_id}`

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Scan links in the message
   - **Target**: `POST http://127.0.0.1:3000/api/messages/{msg_id}/scan-links`
   - **Input**: Header `X-Session-Token: {token}`; no request body
   - **Expected**: 200 OK with a `links` array containing 2 entries

3. Verify each homograph domain is flagged
   - **Target**: (inspect `links` array from step 2)
   - **Input**: each link entry
   - **Expected**:
     - Both entries: `safety` = `"danger"` or at minimum `"caution"`
     - Both entries: `reasons` array contains a string referencing homograph attack, IDN domain, or punycode lookalike
     - Both entries: `is_known_trusted` = `false`

4. Verify the summary reflects elevated risk
   - **Target**: (inspect `summary` from step 2)
   - **Input**: `summary` field
   - **Expected**: `overall_risk` = `"danger"` or `"caution"`; `safe_count` = 0

## Success Criteria
- [ ] Response status is 200
- [ ] Both punycode/IDN links are NOT classified as `"safe"`
- [ ] At least one reason per entry references homograph, IDN, or punycode lookalike behavior
- [ ] `is_known_trusted` = `false` for both entries
- [ ] `summary.safe_count` = 0

## Failure Criteria
- Response status is not 200
- Any homograph/punycode domain is classified as `"safe"` or `is_known_trusted` = `true`
- `reasons` array is empty — no explanation for the elevated safety level
- `summary.overall_risk` = `"safe"`
- Server crashes or returns 500 when processing punycode hostnames

## Notes
Homograph attacks exploit visually identical Unicode characters to spoof legitimate domains (e.g., apple.com vs аpple.com using Cyrillic 'а'). IDN punycode encoding is the wire representation; the scanner must decode punycode before analysis and check for non-ASCII characters in the decoded label.
