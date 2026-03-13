# GC-399: Happy Path — Scan Message with Known-Safe Links

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: link-safety
- **Tags**: links, safety, scanning, trusted-domains, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose HTML body contains the following links:
  - `<a href="https://www.google.com/search?q=iris">Google Search</a>`
  - `<a href="https://github.com/clevertap/iris">GitHub Repo</a>`
  - `<a href="https://docs.microsoft.com/en-us/azure/">Microsoft Docs</a>`
- The message ID is known as `{msg_id}`

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Scan links in the message
   - **Target**: `POST http://127.0.0.1:3000/api/messages/{msg_id}/scan-links`
   - **Input**: Header `X-Session-Token: {token}`; no request body required
   - **Expected**: 200 OK with a JSON body containing a `links` array of 3 entries and a `summary` object

3. Verify each link is classified as safe
   - **Target**: (inspect response body from step 2)
   - **Input**: `links` array
   - **Expected**:
     - `google.com` entry: `safety` = `"safe"`, `is_known_trusted` = `true`, `reasons` is empty or contains only informational strings
     - `github.com` entry: `safety` = `"safe"`, `is_known_trusted` = `true`
     - `microsoft.com` / `docs.microsoft.com` entry: `safety` = `"safe"`, `is_known_trusted` = `true`
     - All entries: `is_shortened` = `false`

4. Verify the summary
   - **Target**: (inspect `summary` object from step 2 response)
   - **Input**: `summary` field
   - **Expected**: `total_links` = 3, `safe_count` = 3, `caution_count` = 0, `danger_count` = 0, `overall_risk` = `"safe"`

## Success Criteria
- [ ] Response status is 200
- [ ] `links` array contains exactly 3 entries, one per href in the message body
- [ ] Every entry has `safety` = `"safe"` and `is_known_trusted` = `true`
- [ ] Every entry has `is_shortened` = `false`
- [ ] `summary.total_links` = 3, `summary.safe_count` = 3
- [ ] `summary.overall_risk` = `"safe"`

## Failure Criteria
- Response status is not 200
- Any known-trusted domain (google.com, github.com, microsoft.com) is flagged as `caution` or `danger`
- `is_known_trusted` is `false` for any of these domains
- `summary.overall_risk` is anything other than `"safe"`
- Server returns 500
