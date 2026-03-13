# GC-343: Missing required 'to' in compose request

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: compose-via-chat
- **Tags**: compose, chat, validation, missing-field, to, negative
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available (ai_enabled = "true")

### Data
- Fresh session ID (e.g. `gc343-session`) — source: inline

## Steps

1. Send a compose request that omits any recipient
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc343-session", "message": "Draft an email about the product launch but do not tell me who to send it to"}'
     ```
   - **Expected**: 200 OK. The AI may ask for a recipient or produce a draft proposal. This step establishes baseline behavior.

2. Attempt to directly invoke the tool layer with a missing `to` field via a crafted COMPOSE_PROPOSAL (simulates what would happen if the LLM produces an incomplete proposal)
   - **Note**: The `handle_compose_email` function in `src/ai/tools.rs` validates that `to` is non-empty. Simulate the internal validation path by constructing a scenario where the AI outputs an incomplete COMPOSE_PROPOSAL with an empty `to` array.
   - **Verification approach**: If the AI returned a `proposed_action` from step 1 without a recipient, attempt to call confirm:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc343-session", "message_id": "<assistant_msg_id_from_step_1>"}'
     ```
   - **Expected**: If `proposed_action` had an empty `to` array, confirm returns `400 Bad Request` or `500 Internal Server Error` (per `handle_compose_email` validation: "At least one recipient is required in the 'to' field.").

3. Verify no draft is created when validation fails
   - **Target**: `GET http://localhost:3000/api/messages?is_draft=true`
   - **Expected**: Draft count is unchanged from before this test case.

4. Verify the AI is prompted to request missing information
   - **Target**: `response.message.content` from step 1
   - **Expected**: The AI response either asks for a recipient or does not produce a `proposed_action` — it does not silently produce a draft to a null or empty recipient.

## Success Criteria
- [ ] AI does not produce a `compose_email` proposed action with an empty `to` array
- [ ] If a compose proposal with an empty `to` were somehow confirmed, confirm returns a non-200 error
- [ ] No draft is created with a null or empty `to_addresses` field
- [ ] Server does not panic or return 500 due to null recipient

## Failure Criteria
- Server accepts and saves a draft with no recipients
- Confirm returns 200 and `executed: true` for an empty-recipient compose
- A draft row is inserted with `to_addresses` as null or empty JSON array
