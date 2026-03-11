# GC-197: Needs-Reply Persists After Mark Read

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: needs-reply
- **Tags**: needs-reply, api, edge, mark-read, independence
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one unread message with `ai_needs_reply = true`
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Confirm the message appears in the needs-reply list
   - **Target**: `GET /api/messages/needs-reply`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK; the target message is present in the `messages` array

2. Mark the message as read
   - **Target**: `PUT /api/messages/batch`
   - **Input**: `{"ids": ["{message_id}"], "action": "mark_read"}`
   - **Expected**: 200 OK

3. Fetch the needs-reply list again
   - **Target**: `GET /api/messages/needs-reply`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK; the same message is still present (needs-reply is independent of read status)

## Success Criteria
- [ ] Message appears in needs-reply list before marking read
- [ ] Mark-read operation succeeds
- [ ] Message still appears in needs-reply list after marking read
- [ ] `ai_needs_reply` remains `true` on the message

## Failure Criteria
- Message disappears from the needs-reply list after being marked read
- `ai_needs_reply` changes to `false` or `null` after mark-read
