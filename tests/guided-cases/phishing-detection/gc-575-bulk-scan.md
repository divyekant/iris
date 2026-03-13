# GC-575: Bulk Phishing Scan Processes Multiple Messages

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: phishing-detection
- **Tags**: phishing, bulk-scan, POST, multiple-messages
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- 3 message IDs: one clean email, one with urgency language, one with suspicious URL

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Submit bulk phishing scan
   - **Target**: `POST http://localhost:3030/api/security/phishing-bulk-scan`
   - **Input**: Header `X-Session-Token: {token}`, body `{"message_ids": ["{clean_id}", "{urgency_id}", "{url_id}"]}`
   - **Expected**: 200 OK, `results` array with 3 entries, each with `message_id` and `risk_level`

3. Verify per-message differentiation
   - **Expected**: clean message has `risk_level: "none"`, urgency message has elevated risk, URL message has elevated risk

## Success Criteria
- [ ] Bulk scan returns 200 OK
- [ ] `results` array has one entry per input message_id
- [ ] Each result includes `message_id`, `risk_level`, and `signals`
- [ ] Results differentiate between clean and suspicious messages

## Failure Criteria
- Fewer results returned than messages submitted
- All messages assigned same risk_level regardless of content
- 5xx error for valid message IDs
