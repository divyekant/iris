# GC-196: Needs-Reply Empty Queue Returns Empty Array

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: needs-reply
- **Tags**: needs-reply, api, edge, empty
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- No messages with `ai_needs_reply = true` exist (either fresh database or all messages have been processed with needs_reply=false)
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Fetch needs-reply messages when none are flagged
   - **Target**: `GET /api/messages/needs-reply`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with `{"messages": [], "total": 0}`

## Success Criteria
- [ ] Response status is 200 (not 404 or 204)
- [ ] `messages` is an empty array `[]`
- [ ] `total` is `0`

## Failure Criteria
- Response status is 404 or 204 instead of 200
- `messages` is `null` instead of an empty array
- `total` is missing or non-zero
