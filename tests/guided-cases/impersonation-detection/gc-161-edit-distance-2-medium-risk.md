# GC-161: Edit Distance 2 Lookalike Domain Flagged as Medium Risk

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: impersonation-detection
- **Tags**: impersonation, trust, levenshtein, edit-distance, medium-risk
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose `from_address` is `noreply@gmial.com` (transposition of 'a' and 'i', edit distance 2 from gmail.com — specifically "gmial" differs from "gmail" by two substitutions when split at the TLD)
- The message ID is known as `{msg_id}`
- Note: the Levenshtein distance between "gmial.com" and "gmail.com" is 2 (swap of 'i' and 'a' counts as two operations in standard edit distance)

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Fetch the message detail for the edit-distance-2 lookalike sender
   - **Target**: GET http://127.0.0.1:3000/api/messages/{msg_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON body containing `impersonation_risk` object with `risk_level: "medium"` and `lookalike_of: "gmail.com"`

## Success Criteria
- [ ] Response status is 200
- [ ] `impersonation_risk` is a non-null object
- [ ] `impersonation_risk.risk_level` equals `"medium"`
- [ ] `impersonation_risk.lookalike_of` equals `"gmail.com"`

## Failure Criteria
- Response status is not 200
- `impersonation_risk` is null or absent (domain not detected)
- `risk_level` is `"high"` (over-classified)
- `lookalike_of` identifies the wrong trusted domain
