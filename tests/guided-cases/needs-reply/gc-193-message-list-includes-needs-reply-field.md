# GC-193: MessageSummary Includes ai_needs_reply Field

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: needs-reply
- **Tags**: needs-reply, api, message-list, schema
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one email account synced with messages processed by the AI pipeline
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Fetch the message list
   - **Target**: `GET /api/messages?account_id={account_id}&folder=INBOX`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with JSON body containing a `messages` array

2. Inspect message objects for ai_needs_reply key
   - **Target**: Each object in the `messages` array
   - **Input**: n/a
   - **Expected**: Every message object has an `ai_needs_reply` key present (value is `true`, `false`, or `null` for unprocessed messages)

## Success Criteria
- [ ] Response status is 200
- [ ] Every message object in `messages` has an `ai_needs_reply` key
- [ ] Values are boolean (`true`/`false`) or `null` for unprocessed messages

## Failure Criteria
- `ai_needs_reply` key is absent from any message object
- Value is a non-boolean, non-null type (e.g., string or integer)
