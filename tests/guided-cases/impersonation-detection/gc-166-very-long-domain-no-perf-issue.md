# GC-166: Very Long Domain Name Does Not Cause Performance Degradation

## Metadata
- **Type**: edge
- **Priority**: P2
- **Surface**: api
- **Flow**: impersonation-detection
- **Tags**: impersonation, trust, edge-case, performance, long-domain
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose `from_address` ends in an unusually long domain name, e.g.:
  `sender@averylongandcompletelymadeupdomainnamewithlotsofcharacters-thatshouldnotmatch-anything.com`
  (approximately 80+ characters in the domain portion)
- The message ID is known as `{msg_id}`

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Fetch the message detail for the long-domain sender
   - **Target**: GET http://127.0.0.1:3000/api/messages/{msg_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK within 2 seconds; `impersonation_risk` is `null` or absent (the length difference with known domains exceeds 2, so the Levenshtein quick-reject path fires)

## Success Criteria
- [ ] Response status is 200
- [ ] Response time is under 2000 ms
- [ ] `impersonation_risk` is `null` or absent (no false match; quick-reject optimizes away)
- [ ] Server process remains stable

## Failure Criteria
- Response status is 500
- Response takes more than 2000 ms (indicates the levenshtein quick-reject is not firing)
- Server process hangs or becomes unresponsive
