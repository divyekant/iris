# GC-570: Urgency Language Detected as Phishing Signal

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: phishing-detection
- **Tags**: phishing, scan, urgency, signals
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message with urgency language: "URGENT: Your account will be suspended in 24 hours. Click immediately to verify your identity."

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Scan the message with urgency language
   - **Target**: `POST http://localhost:3030/api/security/phishing-scan/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `signals` array includes a signal with `type: "urgency_language"` or similar; `risk_level` is `"medium"` or higher

## Success Criteria
- [ ] Scan returns 200 OK
- [ ] `signals` array is non-empty
- [ ] At least one signal identifies urgency language
- [ ] `risk_level` is elevated (medium, high, or critical)

## Failure Criteria
- Urgency language not detected
- `signals` array empty despite obvious urgency content
- `risk_level` returned as `"none"`
