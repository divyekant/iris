# GC-341: Compose with CC recipients

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: compose-via-chat
- **Tags**: compose, chat, cc, draft, recipients
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available (ai_enabled = "true")
- At least one active email account configured

### Data
- Fresh session ID (e.g. `gc341-session`) — source: inline

## Steps

1. Send a compose request explicitly asking for CC recipients
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc341-session", "message": "Draft an email to alice@example.com and CC bob@example.com and carol@example.com about the Q1 budget review"}'
     ```
   - **Expected**: 200 OK. Response contains `message.proposed_action.action == "compose_email"`.

2. Inspect the proposed action data for CC fields
   - **Target**: `response.message.proposed_action.data`
   - **Expected**: `data.to` contains `"alice@example.com"`. `data.cc` is a non-empty array containing `"bob@example.com"` and `"carol@example.com"` (or the AI may assign them as best-effort; the key invariant is `cc` is populated).

3. Confirm the compose proposal
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc341-session", "message_id": "<assistant_msg_id>"}'
     ```
   - **Expected**: 200 OK with `{ "executed": true, "updated": 1, "draft_id": "<uuid>" }`.

4. Fetch the saved draft and verify CC addresses
   - **Target**: `GET http://localhost:3000/api/messages/<draft_id>`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/messages/<draft_id>" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: `is_draft: true`. `cc_addresses` field is non-null and contains at least one of the CC recipients. `to_addresses` contains `alice@example.com`.

## Success Criteria
- [ ] `proposed_action.data.cc` is a non-empty array
- [ ] Confirm returns `executed: true` with a `draft_id`
- [ ] Saved draft has non-null `cc_addresses` field
- [ ] `to_addresses` and `cc_addresses` are distinct — the primary recipient is not duplicated in CC

## Failure Criteria
- `proposed_action.data.cc` is empty or null despite explicit CC request
- Draft is saved with no CC recipients
- Confirm returns `executed: false`
- `cc_addresses` field is null or missing from the saved draft
