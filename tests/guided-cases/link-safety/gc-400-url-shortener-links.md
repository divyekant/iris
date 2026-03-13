# GC-400: URL Shortener Links Flagged as Caution

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: link-safety
- **Tags**: links, safety, scanning, url-shortener, caution
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose HTML body contains the following links:
  - `<a href="https://bit.ly/3xQzAbc">Click here</a>`
  - `<a href="https://t.co/Xy7mNpQr">Read more</a>`
  - `<a href="https://tinyurl.com/y4e2pq5z">Visit our site</a>`
- The message ID is known as `{msg_id}`

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Scan links in the message
   - **Target**: `POST http://127.0.0.1:3000/api/messages/{msg_id}/scan-links`
   - **Input**: Header `X-Session-Token: {token}`; no request body required
   - **Expected**: 200 OK with a JSON body containing a `links` array of 3 entries

3. Verify each link is classified with `is_shortened` = `true`
   - **Target**: (inspect `links` array from step 2)
   - **Input**: each link entry
   - **Expected**:
     - `bit.ly` entry: `is_shortened` = `true`, `safety` = `"caution"` or `"danger"`, `reasons` array contains a string mentioning URL shortener
     - `t.co` entry: `is_shortened` = `true`, `safety` at minimum `"caution"`
     - `tinyurl.com` entry: `is_shortened` = `true`, `safety` at minimum `"caution"`

4. Verify the summary reflects elevated risk
   - **Target**: (inspect `summary` object from step 2)
   - **Input**: `summary` field
   - **Expected**: `total_links` = 3, `caution_count` + `danger_count` = 3, `safe_count` = 0, `overall_risk` is `"caution"` or `"danger"`

## Success Criteria
- [ ] Response status is 200
- [ ] All 3 URL shortener entries have `is_shortened` = `true`
- [ ] All 3 entries are classified as `"caution"` or `"danger"` (not `"safe"`)
- [ ] Each entry's `reasons` array includes at least one string referencing URL shortener / masked destination
- [ ] `summary.safe_count` = 0
- [ ] `summary.overall_risk` is `"caution"` or `"danger"`

## Failure Criteria
- Response status is not 200
- Any shortener URL is classified as `"safe"` or `is_shortened` = `false`
- `reasons` array is empty for shortener links
- `summary.overall_risk` = `"safe"`
