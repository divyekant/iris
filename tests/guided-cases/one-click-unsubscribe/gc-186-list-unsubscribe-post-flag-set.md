# GC-186: list_unsubscribe_post Flag Correctly Set for One-Click Messages

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: one-click-unsubscribe
- **Tags**: unsubscribe, list-unsubscribe-post, rfc8058, message-detail
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- A synced message exists that has BOTH `List-Unsubscribe` (an HTTPS URL) AND `List-Unsubscribe-Post: List-Unsubscribe=One-Click` headers (RFC 8058 compliant); note its message ID

## Steps
1. Fetch the message detail for the RFC 8058 compliant message
   - **Target**: GET /api/messages/{id}
   - **Expected**: 200 OK with `list_unsubscribe` set to the HTTPS URL and `list_unsubscribe_post` set to true

2. POST to the unsubscribe endpoint for the same message
   - **Target**: POST /api/messages/{id}/unsubscribe
   - **Input**: none
   - **Expected**: 200 OK with JSON body `{"success": true, "method": "one-click", "url": "<the URL>"}`

## Success Criteria
- [ ] GET response has `list_unsubscribe_post` = true
- [ ] POST response has `method` = `"one-click"`
- [ ] `success` is true in the POST response
- [ ] Server sends the POST request to the unsubscribe URL on behalf of the user

## Failure Criteria
- `list_unsubscribe_post` is false or null when the header was present
- POST response method is `"url"` instead of `"one-click"` for an RFC 8058 message
- Server error (500)
