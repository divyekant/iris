# GC-344: Missing 'subject' in compose request

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: compose-via-chat
- **Tags**: compose, chat, validation, missing-field, subject, negative
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available (ai_enabled = "true")

### Data
- Fresh session ID (e.g. `gc344-session`) — source: inline

## Steps

1. Send a compose request that explicitly asks for no subject
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc344-session", "message": "Draft an email to dave@example.com with an empty subject line and just say hello"}'
     ```
   - **Expected**: 200 OK. The AI either asks for a subject or produces a proposal with a synthesized subject. The key invariant is that a COMPOSE_PROPOSAL with a blank subject does not reach the confirm path.

2. Inspect the response for the subject field
   - **Target**: `response.message.proposed_action.data.subject`
   - **Expected**: If a `proposed_action` is returned, `data.subject` must be a non-empty string. The tool layer validates this in `handle_compose_email` ("Subject cannot be empty.").

3. Simulate a confirm call targeting a message whose compose data has an empty subject
   - **Note**: Construct a test where the AI (in a degraded scenario) emits `COMPOSE_PROPOSAL:{"to":["dave@example.com"],"cc":[],"subject":"","body":"<p>Hello</p>","reply_to_message_id":null,"tone":"formal"}`. If such a message is stored, attempt confirm:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc344-session", "message_id": "<assistant_msg_id>"}'
     ```
   - **Expected**: Confirm returns 400 Bad Request or 500 (deserialization/validation failure). No draft is saved with an empty subject.

4. Verify no draft with an empty subject was created
   - **Target**: Query for drafts with empty subjects
     ```bash
     curl -s "http://localhost:3000/api/messages?is_draft=true" \
       -H "X-Session-Token: $TOKEN" | jq '[.messages[] | select(.subject == "" or .subject == null)]'
     ```
   - **Expected**: Empty array — no drafts with null or blank subject exist.

## Success Criteria
- [ ] AI does not emit a `compose_email` proposal with a blank `subject` field
- [ ] If a blank-subject compose proposal were confirmed, the server returns a non-200 error
- [ ] No draft is saved with an empty or null subject
- [ ] Server remains stable (no 500 from empty subject string)

## Failure Criteria
- A draft is persisted with `subject: ""` or `subject: null`
- Confirm returns 200 and `executed: true` for a blank-subject compose
- AI silently generates a proposal with `subject: ""` and proceeds
