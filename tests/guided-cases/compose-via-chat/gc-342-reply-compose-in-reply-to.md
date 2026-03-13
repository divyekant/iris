# GC-342: Reply compose — in_reply_to specified

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: compose-via-chat
- **Tags**: compose, chat, reply, in-reply-to, thread, draft
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available (ai_enabled = "true")
- At least one active email account configured

### Data
- An existing email message in the database to reply to
  - Obtain a real message ID: `MSG_ID=$(curl -s "http://localhost:3000/api/messages?limit=1" -H "X-Session-Token: $TOKEN" | jq -r '.messages[0].id')`
  - The message must have a `subject` value (used for thread context) — source: synced via IMAP or seeded

## Steps

1. Obtain a real message ID to reply to
   - **Target**: `GET http://localhost:3000/api/messages?limit=1`
   - **Input**:
     ```bash
     MSG_ID=$(curl -s "http://localhost:3000/api/messages?limit=1" \
       -H "X-Session-Token: $TOKEN" | jq -r '.messages[0].id')
     echo "Replying to: $MSG_ID"
     ```
   - **Expected**: A valid UUID is obtained.

2. Send a compose request asking to reply to that message
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d "{\"session_id\": \"gc342-session\", \"message\": \"Draft a reply to message ID $MSG_ID thanking them for the update\"}"
     ```
   - **Expected**: 200 OK. `message.proposed_action.action == "compose_email"`. `proposed_action.data.reply_to_message_id` equals `$MSG_ID`.

3. Verify thread context is captured in the proposal data
   - **Target**: `response.message.proposed_action.data.thread_subject`
   - **Expected**: `thread_subject` is non-null and matches the subject of the original message (looked up from the DB at compose time by `handle_compose_email`).

4. Confirm the compose proposal
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc342-session", "message_id": "<assistant_msg_id>"}'
     ```
   - **Expected**: 200 OK with `{ "executed": true, "updated": 1, "draft_id": "<uuid>" }`.

5. Verify the saved draft has the reply context
   - **Target**: `GET http://localhost:3000/api/messages/<draft_id>`
   - **Expected**: `is_draft: true`, `folder: "Drafts"`. The subject typically starts with "Re:" or references the original subject.

## Success Criteria
- [ ] `proposed_action.data.reply_to_message_id` is set to the source message ID
- [ ] `proposed_action.data.thread_subject` matches the original message's subject
- [ ] Confirm returns `executed: true` with a valid `draft_id`
- [ ] Saved draft is in `Drafts` folder with `is_draft: true`

## Failure Criteria
- `reply_to_message_id` is null in the proposed action data
- `thread_subject` is null despite the source message having a subject
- Confirm fails or `draft_id` is not returned
- Saved draft has `is_draft: false`
