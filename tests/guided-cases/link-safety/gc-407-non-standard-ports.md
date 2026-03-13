# GC-407: Edge — Links with Non-Standard Ports Flagged as Caution or Danger

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: link-safety
- **Tags**: links, safety, scanning, port, non-standard, caution, edge
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose HTML body contains links with non-standard ports:
  - Standard port (control): `<a href="https://example.com:443/page">Normal HTTPS</a>`
  - Non-standard port: `<a href="http://example.com:8080/login">Login</a>`
  - Non-standard high port: `<a href="https://suspicious.com:31337/verify">Verify account</a>`
  - Non-standard port on otherwise trusted domain: `<a href="https://google.com:9000/pay">Pay Now</a>`
- The message ID is known as `{msg_id}`

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Scan links in the message
   - **Target**: `POST http://127.0.0.1:3000/api/messages/{msg_id}/scan-links`
   - **Input**: Header `X-Session-Token: {token}`; no request body
   - **Expected**: 200 OK with a `links` array containing 4 entries

3. Verify standard-port link is not penalized for the port itself
   - **Target**: (inspect the `example.com:443` entry)
   - **Input**: link entry
   - **Expected**: port 443 is the standard HTTPS port — the entry should not have a port-related reason in `reasons`; overall safety may be `"caution"` for other factors but NOT due to the port

4. Verify non-standard port links are flagged
   - **Target**: (inspect remaining 3 entries)
   - **Input**: link entries with ports 8080, 31337, 9000
   - **Expected**:
     - Each entry: `safety` at least `"caution"`, `reasons` array contains a string referencing non-standard port
     - `suspicious.com:31337`: `safety` = `"caution"` or `"danger"`
     - `google.com:9000`: even if domain is trusted, non-standard port should raise safety to at least `"caution"` and `is_known_trusted` may be `false` due to the port anomaly

5. Verify the summary reflects elevated risk
   - **Target**: (inspect `summary` from step 2)
   - **Input**: `summary` field
   - **Expected**: `total_links` = 4, `safe_count` at most 1 (the :443 link), `caution_count` + `danger_count` >= 3, `overall_risk` = `"caution"` or `"danger"`

## Success Criteria
- [ ] Response status is 200
- [ ] `links` array contains exactly 4 entries
- [ ] Links with ports 8080, 31337, and 9000 each have a port-related entry in `reasons`
- [ ] Links with non-standard ports are classified as at minimum `"caution"`
- [ ] Standard port (443) does not have a port-related reason in its `reasons` array
- [ ] `summary.overall_risk` is `"caution"` or `"danger"`

## Failure Criteria
- Response status is not 200
- Non-standard port links are classified as `"safe"` with no port-related reason
- Standard port (443) is falsely penalized for the port itself
- `reasons` array is empty for non-standard port links
- Server returns 500 when parsing URLs with explicit port numbers
