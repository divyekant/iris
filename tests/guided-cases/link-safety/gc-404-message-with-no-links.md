# GC-404: Negative — Scan Message with No Links Returns Empty Result

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: link-safety
- **Tags**: links, safety, scanning, empty, no-links, negative
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose body is plain text with no anchor tags or URLs — e.g.:
  ```
  Hi,

  Just wanted to say hello. No links here.

  Thanks
  ```
- The message ID is known as `{msg_id}`

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Scan links in the link-free message
   - **Target**: `POST http://127.0.0.1:3000/api/messages/{msg_id}/scan-links`
   - **Input**: Header `X-Session-Token: {token}`; no request body
   - **Expected**: 200 OK with a JSON body — the `links` array is empty and the `summary` reflects zero links

3. Verify the empty response structure
   - **Target**: (inspect response body from step 2)
   - **Input**: `links` and `summary` fields
   - **Expected**:
     - `links` = `[]`
     - `summary.total_links` = 0, `summary.safe_count` = 0, `summary.caution_count` = 0, `summary.danger_count` = 0
     - `summary.overall_risk` = `"safe"` (or an equivalent neutral value indicating no risk)

## Success Criteria
- [ ] Response status is 200
- [ ] `links` is an empty array `[]`
- [ ] `summary.total_links` = 0
- [ ] `summary.safe_count`, `summary.caution_count`, `summary.danger_count` all = 0
- [ ] `summary.overall_risk` = `"safe"` or equivalent neutral value

## Failure Criteria
- Response status is not 200
- `links` contains any entries when the message has no URLs
- `summary.total_links` is non-zero
- Server returns 500 when processing a link-free message
