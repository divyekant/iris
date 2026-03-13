# GC-401: IP-Based URL Flagged as Danger

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: link-safety
- **Tags**: links, safety, scanning, ip-address, danger, suspicious
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose HTML body contains the following links:
  - `<a href="http://192.168.1.105/verify-account">Verify your account</a>`
  - `<a href="http://185.220.101.45/login">Log in here</a>`
- The message ID is known as `{msg_id}`

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Scan links in the message
   - **Target**: `POST http://127.0.0.1:3000/api/messages/{msg_id}/scan-links`
   - **Input**: Header `X-Session-Token: {token}`; no request body required
   - **Expected**: 200 OK with a JSON body containing a `links` array of 2 entries

3. Verify each IP-based link is classified as danger
   - **Target**: (inspect `links` array from step 2)
   - **Input**: each link entry
   - **Expected**:
     - `192.168.1.105` entry: `safety` = `"danger"`, `reasons` array contains a string indicating IP address URL (no domain name)
     - `185.220.101.45` entry: `safety` = `"danger"`, `reasons` array contains a string indicating IP address URL
     - Both entries: `is_known_trusted` = `false`, `is_shortened` = `false`

4. Verify the summary reflects danger-level overall risk
   - **Target**: (inspect `summary` object from step 2)
   - **Input**: `summary` field
   - **Expected**: `total_links` = 2, `danger_count` = 2, `safe_count` = 0, `caution_count` = 0, `overall_risk` = `"danger"`

## Success Criteria
- [ ] Response status is 200
- [ ] Both IP-address URLs are classified as `"danger"`
- [ ] Each entry's `reasons` array references use of a raw IP address
- [ ] `is_known_trusted` = `false` for both entries
- [ ] `summary.danger_count` = 2 and `summary.overall_risk` = `"danger"`

## Failure Criteria
- Response status is not 200
- Any IP-based URL is classified as `"safe"` or `"caution"`
- `reasons` array does not mention IP address usage
- `summary.overall_risk` is not `"danger"`
- Server returns 500
