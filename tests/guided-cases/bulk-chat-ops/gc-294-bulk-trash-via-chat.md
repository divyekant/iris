# GC-294: Bulk trash via chat — move emails from a sender to Trash after confirmation

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: bulk-chat-ops
- **Tags**: chat, bulk, batch-operations, trash, delete
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available

### Data
- At least 2 emails from a distinct sender present in the database (e.g., `noreply@example-promo.com`) (source: synced inbox or seed data)
- Those emails are currently in the Inbox folder (not already trashed)

## Steps

1. Send a bulk trash request via AI Chat
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc294-session", "message": "Trash all emails from noreply@example-promo.com"}'
     ```
   - **Expected**: 200 OK, `message.proposed_action` is non-null with `action: "trash"` and a non-empty `message_ids` array; `message.content` describes the emails that will be trashed

2. Confirm the proposed trash action
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc294-session", "message_id": "<message_id_from_step_1>"}'
     ```
   - **Expected**: 200 OK, `{ "executed": true, "updated": N }` where N >= 1

3. Verify emails are now in the Trash folder
   - **Target**: `GET http://localhost:3000/api/messages?folder=Trash`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/messages?folder=Trash" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: The affected emails appear in the Trash folder with `folder = 'Trash'` in the database; they no longer appear in the Inbox

## Success Criteria
- [ ] Chat response status 200
- [ ] `message.proposed_action.action` equals `"trash"`
- [ ] `message.proposed_action.message_ids` is non-empty
- [ ] Confirm returns `{ "executed": true, "updated": N }` (N >= 1)
- [ ] Trashed emails have `folder = 'Trash'` in the database

## Failure Criteria
- `proposed_action` is null or has `action != "trash"`
- Confirm returns `executed: false`
- Emails remain in Inbox after confirmation
- Server returns 500 at any step
