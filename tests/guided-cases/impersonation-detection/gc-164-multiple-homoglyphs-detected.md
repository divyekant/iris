# GC-164: Multiple Homoglyph Substitutions (g00gle.com) Detected as High Risk

## Metadata
- **Type**: security
- **Priority**: P1
- **Surface**: api
- **Flow**: impersonation-detection
- **Tags**: impersonation, trust, homoglyph, high-risk, google, security, multi-substitution
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose `from_address` is `no-reply@g00gle.com` (two '0' digits substituted for the two 'o' letters in "google" — a multi-character homoglyph attack)
- The message ID is known as `{msg_id}`

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Fetch the message detail for the multi-homoglyph sender
   - **Target**: GET http://127.0.0.1:3000/api/messages/{msg_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with `impersonation_risk` set to `{"lookalike_of": "google.com", "risk_level": "high"}`

## Success Criteria
- [ ] Response status is 200
- [ ] `impersonation_risk` is a non-null object
- [ ] `impersonation_risk.risk_level` equals `"high"`
- [ ] `impersonation_risk.lookalike_of` equals `"google.com"`

## Failure Criteria
- Response status is not 200
- `impersonation_risk` is null (multiple homoglyphs not detected after normalization)
- `risk_level` is anything other than `"high"`
- `lookalike_of` does not match `"google.com"`
