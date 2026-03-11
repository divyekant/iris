# GC-179: Message Detail Includes list_unsubscribe When Present

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: one-click-unsubscribe
- **Tags**: unsubscribe, message-detail, list-unsubscribe
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- A synced message exists that has a `List-Unsubscribe` header (e.g., a promotional email from a mailing list); note its message ID

## Steps
1. Fetch the message detail for a message known to have a List-Unsubscribe header
   - **Target**: GET /api/messages/{id}
   - **Expected**: 200 OK with JSON body containing a non-null `list_unsubscribe` field (a string starting with `https://`, `http://`, or `mailto:`)

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains `list_unsubscribe` as a non-empty string
- [ ] The value is a valid URL or mailto URI (no raw angle-bracket wrapping)
- [ ] `list_unsubscribe_post` field is present (boolean or null)

## Failure Criteria
- Response status is not 200
- `list_unsubscribe` field is absent from response body
- `list_unsubscribe` is null for a message that has the header
- Server error (500)
