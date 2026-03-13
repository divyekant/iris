# GC-364: Multiple Findings — risk_level "high"

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: dlp
- **Tags**: multiple-findings, risk-high, api-key, password, escalation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- Email body containing both an API key and a password credential (source: manual input)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Submit an email containing multiple sensitive findings
   - **Target**: `POST http://localhost:3030/api/compose/scan-dlp`
   - **Input**: Header `X-Session-Token: {token}`, Body:
     ```json
     {
       "subject": "Deployment credentials",
       "body": "API key: sk-prod9876543210fedcba9876\npassword:MyS3cr3tP@ss!",
       "to": ["devops@example.com"]
     }
     ```
   - **Expected**: 200 OK with JSON response

3. Validate multiple findings are reported
   - **Target**: Response from step 2
   - **Input**: Inspect `findings` array
   - **Expected**: `findings` contains at least 2 entries — one with `type: "api_key"` and one with `type: "password"`, each with masked `match` values

4. Validate risk escalation to high
   - **Target**: Response from step 2
   - **Input**: Inspect `risk_level` and `allow_send`
   - **Expected**: `risk_level` is `"high"` (multiple findings escalate from low to high), `allow_send` is `false`

5. Validate line numbers are present
   - **Target**: Response from step 2
   - **Input**: Inspect `line` field on each finding
   - **Expected**: Each finding has a `line` integer corresponding to the line in the body where the match was found

## Success Criteria
- [ ] Response status is 200
- [ ] `findings` contains at least 2 entries
- [ ] At least one finding has `type: "api_key"`
- [ ] At least one finding has `type: "password"`
- [ ] Each finding has a masked `match` (raw secret not exposed)
- [ ] Each finding has a `line` integer
- [ ] `risk_level` is `"high"`
- [ ] `allow_send` is `false`

## Failure Criteria
- Only one finding is returned when two distinct sensitive patterns are present
- `risk_level` is `"low"` despite multiple findings (escalation not applied)
- `allow_send` is `true`
- `match` contains any unmasked secret value

## Notes
The risk escalation rule: a single API key or password is `"low"` risk; multiple findings of any types escalate to `"high"`. This case uses an API key + password combination to trigger that escalation. The `line` field provides location context to help the user locate and remove the sensitive data before sending.
