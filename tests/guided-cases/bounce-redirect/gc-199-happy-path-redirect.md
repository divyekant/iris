# GC-199: Happy path — redirect email to valid recipient

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: bounce-redirect
- **Tags**: redirect, happy-path, resent-headers, smtp
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- SMTP configured and reachable for the test account

### Data
- Existing message ID with a valid `from_address` (source: seed or prior sync)
- Active account linked to the message
- Valid recipient email: `colleague@example.com`

## Steps
1. Send redirect request
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "colleague@example.com" }`
   - **Expected**: 200 OK, response body `{ "redirected": true, "to": "colleague@example.com" }`

2. Verify Resent-* headers in outbound email
   - **Target**: SMTP capture / server logs
   - **Input**: Inspect the sent message
   - **Expected**: Message contains `Resent-From` (original account address), `Resent-To: colleague@example.com`, `Resent-Date` (current timestamp), and original `From`, `Subject`, `Date` are preserved unchanged

## Success Criteria
- [ ] Response status is 200
- [ ] Response JSON has `redirected: true` and `to: "colleague@example.com"`
- [ ] Original From, Subject, Date headers are preserved in the sent message
- [ ] Resent-From, Resent-To, Resent-Date headers are present and correct

## Failure Criteria
- Non-200 status code
- Missing or incorrect Resent-* headers
- Original headers modified or stripped

## Notes
This is the primary happy path. Confirms the full redirect flow from API to SMTP send with correct RFC 2822 Resent-* header construction.
