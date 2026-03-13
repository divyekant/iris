# GC-359: Happy Path — Clean Email Returns No Findings

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: dlp
- **Tags**: happy-path, scan-dlp, no-findings, risk-none
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- An email body containing no sensitive data (source: manual input)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Submit a clean email for DLP scanning
   - **Target**: `POST http://localhost:3030/api/compose/scan-dlp`
   - **Input**: Header `X-Session-Token: {token}`, Body:
     ```json
     {
       "subject": "Team lunch next Friday",
       "body": "Hi all,\n\nJust a reminder that we have a team lunch scheduled for next Friday at noon. Please RSVP by Wednesday.\n\nThanks!",
       "to": ["team@example.com"]
     }
     ```
   - **Expected**: 200 OK with JSON response

3. Validate response structure and values
   - **Target**: Response from step 2
   - **Input**: Inspect `findings`, `risk_level`, `allow_send` fields
   - **Expected**: `findings` is an empty array, `risk_level` is `"none"`, `allow_send` is `true`

## Success Criteria
- [ ] Response status is 200
- [ ] `findings` array is empty (`[]`)
- [ ] `risk_level` is `"none"`
- [ ] `allow_send` is `true`

## Failure Criteria
- Response status is not 200
- `findings` array is non-empty for plain text with no sensitive data
- `risk_level` is anything other than `"none"`
- `allow_send` is `false`

## Notes
This is the baseline happy-path case. Any false positive here would indicate the detection patterns are too broad.
