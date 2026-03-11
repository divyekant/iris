# GC-163: Completely Unrelated Domain Returns No Impersonation Risk

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: impersonation-detection
- **Tags**: impersonation, trust, safe-sender, unrelated-domain
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose `from_address` is `hello@mycustomdomain.com` — a domain with no lexical similarity to any of the 26 known trusted domains
- The message ID is known as `{msg_id}`

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Fetch the message detail for the unrelated sender
   - **Target**: GET http://127.0.0.1:3000/api/messages/{msg_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK; `impersonation_risk` is absent or `null`

## Success Criteria
- [ ] Response status is 200
- [ ] `impersonation_risk` is `null` or the field is absent
- [ ] No false-positive risk label is attached to a legitimate unrelated domain

## Failure Criteria
- Response status is not 200
- `impersonation_risk` is non-null (false positive — unrelated domain incorrectly flagged)
- Server returns 500
