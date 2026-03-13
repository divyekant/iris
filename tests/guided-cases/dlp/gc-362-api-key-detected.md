# GC-362: API Key Detected (sk-test1234567890abcdef1234)

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: dlp
- **Tags**: api-key, detection, masking, risk-low, secret
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- Email body containing an `sk-*` prefixed API key (source: manual input)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Submit an email containing a single API key
   - **Target**: `POST http://localhost:3030/api/compose/scan-dlp`
   - **Input**: Header `X-Session-Token: {token}`, Body:
     ```json
     {
       "subject": "API credentials",
       "body": "Here is the API key for the staging environment: sk-test1234567890abcdef1234",
       "to": ["dev@example.com"]
     }
     ```
   - **Expected**: 200 OK with JSON response

3. Validate API key finding is reported
   - **Target**: Response from step 2
   - **Input**: Inspect `findings` array
   - **Expected**: `findings` contains one entry with `type: "api_key"`, `location: "body"`, `match` masked (first 4 + `****` + last 4 of the key value)

4. Validate risk level for single API key
   - **Target**: Response from step 2
   - **Input**: Inspect `risk_level` and `allow_send`
   - **Expected**: `risk_level` is `"low"` (single API key = low risk), `allow_send` is `true`

## Success Criteria
- [ ] Response status is 200
- [ ] `findings` contains exactly one entry with `type: "api_key"`
- [ ] Finding `location` is `"body"`
- [ ] Finding `match` is masked and does not expose the full key
- [ ] `risk_level` is `"low"` (single API key qualifies as low risk)
- [ ] `allow_send` is `true`

## Failure Criteria
- API key is not detected
- `match` contains the full unmasked key
- `risk_level` is `"none"` (finding not counted)
- `risk_level` is `"high"` for a single API key (should be low per spec)

## Notes
A single `sk-*` key falls under the "low" risk tier. If multiple findings are present, risk escalates to "high" — that is tested in GC-364. Other recognized prefixes include `pk_live_*`, `pk_test_*`, `AKIA*`, `ghp_*`, `glpat-*`, `xoxb-*`, and `Bearer ey*`.
