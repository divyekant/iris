# GC-572: Display Name / From Address Mismatch Detected

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: phishing-detection
- **Tags**: phishing, scan, sender-mismatch, from, display-name
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message where the display name says "PayPal Security" but the actual From address is "noreply@random-domain-xyz.ru"

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Scan the message with sender mismatch
   - **Target**: `POST http://localhost:3030/api/security/phishing-scan/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `signals` includes a signal for sender mismatch or display name spoofing; `risk_level` is `"medium"` or higher

## Success Criteria
- [ ] Scan detects the display name / From address mismatch
- [ ] `signals` includes a sender-related signal
- [ ] `risk_level` elevated appropriately
- [ ] Signal includes evidence identifying the mismatched fields

## Failure Criteria
- Sender mismatch not detected
- risk_level remains "none" despite spoofed sender
