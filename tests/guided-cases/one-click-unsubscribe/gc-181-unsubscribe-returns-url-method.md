# GC-181: Unsubscribe Endpoint Returns Method "url" for HTTP Link

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: one-click-unsubscribe
- **Tags**: unsubscribe, url-method, redirect
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- A synced message exists with `list_unsubscribe` set to an HTTPS URL and `list_unsubscribe_post` = false (i.e., no List-Unsubscribe-Post header); note its message ID

## Steps
1. POST to the unsubscribe endpoint for the message
   - **Target**: POST /api/messages/{id}/unsubscribe
   - **Input**: none (no request body required)
   - **Expected**: 200 OK with JSON body `{"success": true, "method": "url", "url": "<the unsubscribe URL>"}`

## Success Criteria
- [ ] Response status is 200
- [ ] `success` is true
- [ ] `method` equals `"url"`
- [ ] `url` is a non-empty string matching the stored `list_unsubscribe` value
- [ ] No external HTTP request is made server-side (frontend is expected to open the URL)

## Failure Criteria
- Response status is not 200
- `method` is not `"url"`
- `url` field is absent or empty
- Server error (500)
