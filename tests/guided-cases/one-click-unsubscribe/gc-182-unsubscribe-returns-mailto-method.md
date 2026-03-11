# GC-182: Unsubscribe Endpoint Returns Method "mailto" for Mailto Link

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: one-click-unsubscribe
- **Tags**: unsubscribe, mailto-method
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- A synced message exists with `list_unsubscribe` set to a `mailto:` URI (e.g., `mailto:unsubscribe@example.com`); note its message ID

## Steps
1. POST to the unsubscribe endpoint for the message
   - **Target**: POST /api/messages/{id}/unsubscribe
   - **Input**: none
   - **Expected**: 200 OK with JSON body `{"success": true, "method": "mailto", "url": "mailto:unsubscribe@example.com"}`

## Success Criteria
- [ ] Response status is 200
- [ ] `success` is true
- [ ] `method` equals `"mailto"`
- [ ] `url` starts with `mailto:`
- [ ] No server-side email is sent (frontend handles mailto URI)

## Failure Criteria
- Response status is not 200
- `method` is not `"mailto"`
- `url` does not start with `mailto:`
- Server error (500)
