# GC-578: Combined Signals Escalate to Critical Risk Level

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: phishing-detection
- **Tags**: phishing, scan, combined-signals, critical, escalation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message combining multiple phishing signals: urgency language + credential request + suspicious URL + sender mismatch

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Scan the multi-signal phishing message
   - **Target**: `POST http://localhost:3030/api/security/phishing-scan/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `signals` array has ≥ 3 entries; `risk_level` is `"high"` or `"critical"`; `is_phishing: true`

3. Verify all signal types are reported
   - **Expected**: `signals` array includes distinct types for urgency, credential request, URL, and sender issues; `risk_score` (if present) is high

## Success Criteria
- [ ] Multiple signals detected and reported separately
- [ ] `risk_level` escalates to `"high"` or `"critical"` with combined signals
- [ ] `is_phishing` is `true`
- [ ] Each signal has a distinct `type` value

## Failure Criteria
- Only one signal detected despite multiple present
- `risk_level` does not escalate beyond single-signal level
- `is_phishing` remains `false`
