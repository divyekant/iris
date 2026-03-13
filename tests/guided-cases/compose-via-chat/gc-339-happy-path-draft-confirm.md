# GC-339: Happy path — ask AI to draft, get proposal, confirm, draft saved

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: compose-via-chat
- **Tags**: compose, chat, confirm, draft, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available (ai_enabled = "true")
- At least one active email account configured

### Data
- Fresh session ID (e.g. `gc339-session`) — source: inline
- No pre-existing draft for the target recipient

## Steps

1. Send a compose request via chat
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc339-session", "message": "Draft an email to bob@example.com about the project deadline being moved to next Friday"}'
     ```
   - **Expected**: 200 OK. Response JSON contains `message.proposed_action` with `action: "compose_email"`. The `message.content` does NOT contain raw `COMPOSE_PROPOSAL:` text.

2. Extract the assistant message ID and proposed action details from the response
   - **Target**: `response.message.id` and `response.message.proposed_action`
   - **Expected**: `proposed_action.action == "compose_email"`. `proposed_action.description` contains the recipient and subject. `proposed_action.data` contains `to`, `subject`, `body` fields.

3. Confirm the compose proposal to create a draft
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc339-session", "message_id": "<assistant_msg_id_from_step_1>"}'
     ```
   - **Expected**: 200 OK. Response JSON: `{ "executed": true, "updated": 1, "draft_id": "<uuid>" }`.

4. Verify the draft exists in the messages table
   - **Target**: `GET http://localhost:3000/api/messages/<draft_id>`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/messages/<draft_id_from_step_3>" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 200 OK. Message has `is_draft: true`, `folder: "Drafts"`, `to_addresses` contains `bob@example.com`, `subject` mentions the deadline.

## Success Criteria
- [ ] `POST /api/ai/chat` returns 200 with `message.proposed_action.action == "compose_email"`
- [ ] `proposed_action.data.to` includes `"bob@example.com"`
- [ ] `proposed_action.data.subject` is non-empty
- [ ] `proposed_action.data.body` is non-empty HTML
- [ ] `message.content` does not contain the raw `COMPOSE_PROPOSAL:` marker
- [ ] `POST /api/ai/chat/confirm` returns 200 with `executed: true`, `updated: 1`, `draft_id` set to a UUID
- [ ] Draft message is retrievable and has `is_draft: true`, `folder: "Drafts"`

## Failure Criteria
- Chat endpoint returns non-200 status
- `proposed_action` is null or missing from the response
- `proposed_action.action` is not `"compose_email"`
- Confirm endpoint returns non-200 status
- `executed` is false or `draft_id` is null
- Draft is not persisted in the database
