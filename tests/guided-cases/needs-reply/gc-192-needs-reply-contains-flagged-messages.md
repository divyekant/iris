# GC-192: Needs-Reply Returns Messages with ai_needs_reply=true

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: needs-reply
- **Tags**: needs-reply, api, ai-classification, happy-path
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one message processed by the AI pipeline with `ai_needs_reply = true`
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Fetch needs-reply messages
   - **Target**: `GET /api/messages/needs-reply`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with a non-empty `messages` array

2. Verify all returned messages have ai_needs_reply=true
   - **Target**: Each object in the `messages` array
   - **Input**: n/a
   - **Expected**: Every message object has `ai_needs_reply` set to `true`

3. Cross-check with general message list
   - **Target**: `GET /api/messages?account_id={account_id}&folder=INBOX`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: Messages with `ai_needs_reply=true` from the general list are all present in the needs-reply endpoint result

## Success Criteria
- [ ] All messages in the needs-reply list have `ai_needs_reply` equal to `true`
- [ ] No messages with `ai_needs_reply=false` or `null` appear
- [ ] Needs-reply list is a subset of the full message list

## Failure Criteria
- A message with `ai_needs_reply=false` or `null` appears in the needs-reply list
- Messages with `ai_needs_reply=true` are missing from the needs-reply list
