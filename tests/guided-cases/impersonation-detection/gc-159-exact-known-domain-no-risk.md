# GC-159: Exact Known Domain Returns No Impersonation Risk

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: impersonation-detection
- **Tags**: impersonation, trust, safe-sender, exact-match, gmail
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose `from_address` is `sender@gmail.com`
- The message ID of that message is known (referred to as `{msg_id}` below)

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Fetch the message detail for the gmail.com sender
   - **Target**: GET http://127.0.0.1:3000/api/messages/{msg_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON body; the `impersonation_risk` field is either absent or `null`

## Success Criteria
- [ ] Response status is 200
- [ ] `impersonation_risk` is `null` or the field is absent from the response body
- [ ] No `lookalike_of` or `risk_level` sub-fields are present

## Failure Criteria
- Response status is not 200
- `impersonation_risk` is a non-null object (false positive against a legitimate gmail.com sender)
- Server returns 500
