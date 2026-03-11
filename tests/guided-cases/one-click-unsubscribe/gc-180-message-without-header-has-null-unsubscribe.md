# GC-180: Message Without Unsubscribe Header Has Null list_unsubscribe

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: one-click-unsubscribe
- **Tags**: unsubscribe, message-detail, null
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- A synced message exists that does NOT have a `List-Unsubscribe` header (e.g., a regular personal email); note its message ID

## Steps
1. Fetch the message detail for a message with no List-Unsubscribe header
   - **Target**: GET /api/messages/{id}
   - **Expected**: 200 OK with JSON body where `list_unsubscribe` is null

## Success Criteria
- [ ] Response status is 200
- [ ] `list_unsubscribe` field is present in the response body
- [ ] `list_unsubscribe` value is null (not an empty string, not absent)
- [ ] `list_unsubscribe_post` is also null or false

## Failure Criteria
- Response status is not 200
- `list_unsubscribe` field is absent from the response body
- `list_unsubscribe` is a non-null value for a message without the header
- Server error (500)
