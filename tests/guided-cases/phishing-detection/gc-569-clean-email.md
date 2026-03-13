# GC-569: Clean Email Returns risk_level "none"

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: phishing-detection
- **Tags**: phishing, scan, clean, none, risk
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A known-clean message: plain business email from a legitimate sender with no suspicious URLs, no urgency language, no credential requests

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Scan the clean message for phishing indicators
   - **Target**: `POST http://localhost:3030/api/security/phishing-scan/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response includes `risk_level: "none"`, `signals` array is empty or minimal, `is_phishing: false`

## Success Criteria
- [ ] Scan returns 200 OK
- [ ] `risk_level` is `"none"` or `"low"`
- [ ] `is_phishing` is `false`
- [ ] No false-positive signals reported for clean content

## Failure Criteria
- Clean email flagged as phishing
- `risk_level` returns `"high"` or `"critical"` for clean content
- 5xx error on valid message
