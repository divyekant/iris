# GC-574: Credential Request Language Detected as High-Risk Signal

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: phishing-detection
- **Tags**: phishing, scan, credential-request, password, risk-high
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message body: "Please reply with your username, password, and bank account number to complete verification."

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Scan the message requesting credentials
   - **Target**: `POST http://localhost:3030/api/security/phishing-scan/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `signals` includes a `credential_request` type signal; `risk_level` is `"high"` or `"critical"`; `is_phishing: true`

## Success Criteria
- [ ] Credential request language detected
- [ ] `risk_level` is `"high"` or `"critical"`
- [ ] `is_phishing` is `true`
- [ ] Signal type is specific to credential harvesting

## Failure Criteria
- `is_phishing` is `false` for explicit credential requests
- `risk_level` below `"high"`
