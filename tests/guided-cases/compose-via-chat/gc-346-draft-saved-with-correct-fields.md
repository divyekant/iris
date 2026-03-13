# GC-346: Draft saved with correct fields after confirm

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: compose-via-chat
- **Tags**: compose, chat, draft, fields, verification, positive
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available (ai_enabled = "true")
- At least one active email account configured

### Data
- Fresh session ID (e.g. `gc346-session`) — source: inline
- Known recipient: `frank@example.com`
- Known subject phrase: "Q2 planning session"

## Steps

1. Send a specific compose request
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc346-session", "message": "Draft a formal email to frank@example.com with subject Q2 planning session. Say that we need to align on Q2 priorities and ask if Tuesday works."}'
     ```
   - **Expected**: 200 OK. `message.proposed_action.action == "compose_email"`. `proposed_action.data.to` contains `frank@example.com`. `proposed_action.data.subject` contains the requested subject text. `proposed_action.data.body` is HTML.

2. Record the proposed action data before confirming
   - **Target**: `response.message.proposed_action.data`
   - **Expected**: Save `to`, `subject`, `body` values for field-by-field comparison post-confirm.

3. Confirm the proposal
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc346-session", "message_id": "<assistant_msg_id>"}'
     ```
   - **Expected**: 200 OK with `{ "executed": true, "updated": 1, "draft_id": "<uuid>" }`.

4. Fetch the full draft and verify every field
   - **Target**: `GET http://localhost:3000/api/messages/<draft_id>`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/messages/<draft_id>" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected** (field-by-field):
     - `is_draft: true`
     - `folder: "Drafts"`
     - `to_addresses` is a JSON array containing `"frank@example.com"`
     - `subject` contains the Q2 planning session text (or something close — AI may paraphrase)
     - `body_html` is non-empty HTML (contains `<p>` or similar tags)
     - `body_text` is non-empty plain text (HTML tags stripped by `strip_html_tags`)
     - `snippet` is non-empty (first 200 chars of `body_text`)
     - `is_read: true` (drafts are inserted as read)
     - `is_starred: false`
     - `has_attachments: false`
     - `date` is a recent Unix timestamp (within the last 60 seconds)

5. Verify the draft is linked to an active account
   - **Target**: `account_id` field on the draft message
   - **Expected**: `account_id` matches the first active account in the accounts table (as selected by `execute_compose_email`). It is not null.

## Success Criteria
- [ ] `is_draft: true`
- [ ] `folder: "Drafts"`
- [ ] `to_addresses` contains the specified recipient
- [ ] `subject` is non-empty
- [ ] `body_html` is non-empty HTML
- [ ] `body_text` is non-empty plain text (HTML stripped)
- [ ] `snippet` is non-empty (up to 200 chars of body_text)
- [ ] `is_read: true`
- [ ] `account_id` is set to a valid account ID
- [ ] `date` is a recent timestamp

## Failure Criteria
- Any of the listed fields is null when it should be populated
- `is_draft` is false
- `folder` is not "Drafts"
- `body_html` or `body_text` is empty
- `account_id` is null
- Draft `id` does not match the `draft_id` returned by confirm
