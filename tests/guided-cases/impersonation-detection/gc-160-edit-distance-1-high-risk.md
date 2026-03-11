# GC-160: Edit Distance 1 Lookalike Domain Flagged as High Risk

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: impersonation-detection
- **Tags**: impersonation, trust, levenshtein, edit-distance, high-risk
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose `from_address` is `noreply@gmai1.com` (digit '1' replacing letter 'l' — edit distance 1 from gmail.com)
- The message ID is known as `{msg_id}`

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Fetch the message detail for the lookalike sender
   - **Target**: GET http://127.0.0.1:3000/api/messages/{msg_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON body containing `impersonation_risk` object with `risk_level: "high"` and `lookalike_of: "gmail.com"`

## Success Criteria
- [ ] Response status is 200
- [ ] `impersonation_risk` is a non-null object
- [ ] `impersonation_risk.risk_level` equals `"high"`
- [ ] `impersonation_risk.lookalike_of` equals `"gmail.com"`

## Failure Criteria
- Response status is not 200
- `impersonation_risk` is null or absent (failed to detect the lookalike)
- `risk_level` is `"medium"` instead of `"high"` (wrong severity classification)
- `lookalike_of` identifies the wrong trusted domain
