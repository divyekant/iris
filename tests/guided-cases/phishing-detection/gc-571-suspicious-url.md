# GC-571: Suspicious URL in Email Body Is Flagged

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: phishing-detection
- **Tags**: phishing, scan, url, suspicious, signals
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message containing a URL that redirects through a URL shortener or has mismatched anchor text (e.g., anchor text says "paypal.com" but link href points to "http://192.168.1.1/login")

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Scan the message with the suspicious URL
   - **Target**: `POST http://localhost:3030/api/security/phishing-scan/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `signals` includes a signal for URL mismatch or suspicious link; `risk_level` elevated

3. Verify response structure
   - **Expected**: Each signal in `signals` array includes `type`, `description`, and optionally `evidence` (e.g., the suspicious URL)

## Success Criteria
- [ ] Suspicious URL generates a signal in `signals` array
- [ ] Signal type references URL or link analysis
- [ ] `risk_level` reflects the URL risk
- [ ] Signal includes enough detail to identify the offending URL

## Failure Criteria
- Suspicious URL not detected
- Signal missing `type` or `description` fields
