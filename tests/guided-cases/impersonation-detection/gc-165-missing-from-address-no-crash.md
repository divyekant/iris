# GC-165: Message with Empty or Missing From Address Does Not Crash

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: impersonation-detection
- **Tags**: impersonation, trust, edge-case, empty-from, robustness
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists with an empty or absent `from_address` (e.g., a MAILER-DAEMON bounce message, or a message with `From: <>`)
- The message ID is known as `{msg_id}`
- If no such real message exists: use any message ID and confirm the impersonation check does not panic, or verify the behavior with a MAILER-DAEMON message if one is available

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Fetch the message detail for the empty-from-address message
   - **Target**: GET http://127.0.0.1:3000/api/messages/{msg_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK (or 404 if message not found); crucially the server must NOT return 500; `impersonation_risk` should be `null` or absent

## Success Criteria
- [ ] Response status is 200 or 404 (not 500)
- [ ] Server process remains alive and responsive to subsequent requests
- [ ] `impersonation_risk` is `null` or absent when `from_address` has no domain to parse

## Failure Criteria
- Response status is 500 (server panic or unhandled error in domain extraction)
- Server process crashes or becomes unresponsive after the request
