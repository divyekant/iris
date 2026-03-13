# GC-406: Edge — data: URI and javascript: URI Links Flagged as Danger

## Metadata
- **Type**: edge
- **Priority**: P0
- **Surface**: api
- **Flow**: link-safety
- **Tags**: links, safety, scanning, data-uri, javascript-uri, danger, edge, xss
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose HTML body contains dangerous URI scheme links:
  - `<a href="javascript:alert('xss')">Click me</a>`
  - `<a href="data:text/html;base64,PHNjcmlwdD5hbGVydCgneHNzJyk8L3NjcmlwdD4=">Open document</a>`
  - `<a href="javascript:void(document.cookie)">Terms</a>`
- The message ID is known as `{msg_id}`

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Scan links in the message
   - **Target**: `POST http://127.0.0.1:3000/api/messages/{msg_id}/scan-links`
   - **Input**: Header `X-Session-Token: {token}`; no request body
   - **Expected**: 200 OK with a `links` array containing 3 entries

3. Verify javascript: URIs are classified as danger
   - **Target**: (inspect `links` array from step 2)
   - **Input**: entries with `url` starting with `javascript:`
   - **Expected**:
     - `safety` = `"danger"`
     - `reasons` array contains a string indicating dangerous URI scheme (`javascript:` protocol)
     - `is_known_trusted` = `false`, `is_shortened` = `false`

4. Verify data: URI is classified as danger
   - **Target**: (inspect `links` array from step 2)
   - **Input**: entry with `url` starting with `data:`
   - **Expected**:
     - `safety` = `"danger"`
     - `reasons` array contains a string indicating dangerous URI scheme (`data:` protocol)
     - `is_known_trusted` = `false`, `is_shortened` = `false`

5. Verify the summary reflects danger-level overall risk
   - **Target**: (inspect `summary` from step 2)
   - **Input**: `summary` field
   - **Expected**: `total_links` = 3, `danger_count` = 3, `safe_count` = 0, `overall_risk` = `"danger"`

## Success Criteria
- [ ] Response status is 200
- [ ] All 3 entries classified as `"danger"`
- [ ] `reasons` for `javascript:` entries explicitly reference the dangerous protocol/URI scheme
- [ ] `reasons` for `data:` entry explicitly reference the dangerous protocol/URI scheme
- [ ] `summary.danger_count` = 3 and `summary.overall_risk` = `"danger"`

## Failure Criteria
- Response status is not 200
- Any `javascript:` or `data:` URI is classified as `"safe"` or `"caution"`
- `reasons` array is empty — no explanation for danger classification
- Scanner omits these URIs from the `links` array entirely without reporting them
- Server returns 500 when parsing non-HTTP URI schemes

## Notes
`javascript:` and `data:` URIs in email links are almost universally malicious — they can execute scripts or embed inline documents to bypass email sandboxing. The scanner should treat any non-http/https/mailto scheme as at least caution, and `javascript:` / `data:` specifically as danger.
