# GC-208: Redirect with excessively large email address

## Metadata
- **Type**: edge
- **Priority**: P2
- **Surface**: api
- **Flow**: bounce-redirect
- **Tags**: redirect, edge-case, input-length, validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- Existing message ID with a valid `from_address` (source: seed or prior sync)
- Active account linked to the message

## Steps
1. Send redirect request with 255-character local part (RFC 5321 max)
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "<254 'a' chars + 'a'>@example.com" }` (local part = 255 characters)
   - **Expected**: 400 Bad Request — local part exceeds practical limits, or server handles gracefully

2. Send redirect request with 64-character local part (RFC 5321 compliant max)
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "<64 'a' chars>@example.com" }` (local part = 64 characters)
   - **Expected**: 200 OK if server follows RFC 5321 local part limit of 64 chars, or 400 if stricter

3. Send redirect request with total address exceeding 320 characters
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "<64 'a' chars>@<252 char domain>.com" }` (total > 320 characters)
   - **Expected**: 400 Bad Request — total address length exceeds RFC limits

4. Send redirect request with 10,000 character email address
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "<9990 'a' chars>@example.com" }`
   - **Expected**: 400 Bad Request — rejected before any SMTP processing, no server crash or excessive memory use

## Success Criteria
- [ ] Oversized addresses (Steps 1, 3, 4) return 400 status
- [ ] No server crash, timeout, or OOM on the 10,000 char input
- [ ] No email is sent via SMTP for oversized addresses
- [ ] Response time for the 10,000 char input is under 1 second
- [ ] Step 2 (64-char local part) either succeeds or fails gracefully depending on implementation strictness

## Failure Criteria
- Server crashes or hangs on large input
- 500 status returned instead of 400
- Email is dispatched with an oversized address
- Response time exceeds 5 seconds for any input

## Notes
Tests input boundary handling for email length. RFC 5321 limits local part to 64 octets and total path to 256 octets, but many implementations are stricter. The key concern is that the server does not crash, allocate excessive memory, or attempt SMTP delivery with obviously invalid addresses.
