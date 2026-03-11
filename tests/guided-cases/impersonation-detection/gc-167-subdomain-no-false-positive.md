# GC-167: Legitimate Subdomain of Known Domain Does Not False-Positive

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: impersonation-detection
- **Tags**: impersonation, trust, edge-case, subdomain, false-positive
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose `from_address` is `noreply@mail.gmail.com` — a legitimate subdomain of gmail.com used by Google for outgoing notifications
- The message ID is known as `{msg_id}`
- Note: `mail.gmail.com` has edit distance > 2 from all 26 known trusted domains when compared as full strings, so it should produce no risk signal

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Fetch the message detail for the subdomain sender
   - **Target**: GET http://127.0.0.1:3000/api/messages/{msg_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK; `impersonation_risk` is `null` or absent

## Success Criteria
- [ ] Response status is 200
- [ ] `impersonation_risk` is `null` or absent (no false positive on a legitimate subdomain)
- [ ] System does not flag `mail.gmail.com` as a lookalike of `gmail.com`

## Failure Criteria
- Response status is not 200
- `impersonation_risk` is non-null (false positive — legitimate google subdomain incorrectly flagged)
- Server returns 500
