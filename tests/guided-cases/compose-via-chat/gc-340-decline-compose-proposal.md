# GC-340: Decline compose proposal — no draft saved

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: compose-via-chat
- **Tags**: compose, chat, decline, draft, cancel
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available (ai_enabled = "true")
- At least one active email account configured

### Data
- Fresh session ID (e.g. `gc340-session`) — source: inline

## Steps

1. Send a compose request via chat to get a proposal
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc340-session", "message": "Draft an email to carol@example.com about the meeting cancellation"}'
     ```
   - **Expected**: 200 OK. Response contains `message.proposed_action.action == "compose_email"` and a `message.id`.

2. Note the assistant message ID — do NOT call confirm
   - **Target**: `response.message.id`
   - **Expected**: Value is a UUID string.

3. Verify no draft was created by listing drafts before confirming
   - **Target**: `GET http://localhost:3000/api/messages?is_draft=true`
   - **Input**:
     ```bash
     DRAFT_COUNT_BEFORE=$(curl -s "http://localhost:3000/api/messages?is_draft=true" \
       -H "X-Session-Token: $TOKEN" | jq '.messages | length')
     echo "Drafts before: $DRAFT_COUNT_BEFORE"
     ```
   - **Expected**: Captures a baseline draft count.

4. Explicitly decline by NOT calling the confirm endpoint — instead discard by sending a follow-up message in the same session
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc340-session", "message": "Never mind, cancel that draft"}'
     ```
   - **Expected**: 200 OK. AI responds acknowledging the cancellation or asking how to help. No new draft created.

5. Verify the draft count did not increase
   - **Target**: `GET http://localhost:3000/api/messages?is_draft=true`
   - **Input**:
     ```bash
     DRAFT_COUNT_AFTER=$(curl -s "http://localhost:3000/api/messages?is_draft=true" \
       -H "X-Session-Token: $TOKEN" | jq '.messages | length')
     echo "Drafts after: $DRAFT_COUNT_AFTER"
     [ "$DRAFT_COUNT_AFTER" -eq "$DRAFT_COUNT_BEFORE" ] && echo "PASS: no draft created"
     ```
   - **Expected**: `DRAFT_COUNT_AFTER == DRAFT_COUNT_BEFORE`. No new draft row was inserted.

## Success Criteria
- [ ] Initial compose chat returns a `compose_email` proposed action
- [ ] No draft is persisted before confirm is called
- [ ] Declining (not calling confirm) results in zero new draft rows
- [ ] Draft count after the session equals draft count before

## Failure Criteria
- A draft is created automatically without confirm being called
- Draft count increases before the confirm endpoint is hit
- The confirm endpoint is called despite the test explicitly not calling it
