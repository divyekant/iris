# GC-296: move_to_category action — recategorize matching emails via chat

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: bulk-chat-ops
- **Tags**: chat, bulk, batch-operations, move-to-category, category
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available

### Data
- At least 2 emails with `ai_category` other than `"promotions"` present in the database (source: synced inbox or seed data)
- Known sender or search term to target those emails

## Steps

1. Send a category move request via AI Chat
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc296-session", "message": "Move all emails from deals@shop.example.com to the Promotions category"}'
     ```
   - **Expected**: 200 OK, `message.proposed_action` is non-null with `action: "move_to_category"` and a non-empty `message_ids` array; `message.content` describes which emails will be recategorized and to which category

2. Verify proposed action contains category information
   - **Target**: Response JSON `message.proposed_action`
   - **Expected**: The `action` field equals `"move_to_category"`; `message_ids` is a non-empty array; the description in `message.content` mentions "promotions"

3. Confirm the category move
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc296-session", "message_id": "<message_id_from_step_1>"}'
     ```
   - **Expected**: 200 OK, `{ "executed": true, "updated": N }` where N >= 1

4. Verify category updated in the database
   - **Target**: `GET http://localhost:3000/api/messages?category=promotions`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/messages?category=promotions" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: The affected emails now appear in the promotions category; their `ai_category` field is `"promotions"`

## Success Criteria
- [ ] Chat response status 200
- [ ] `message.proposed_action.action` equals `"move_to_category"`
- [ ] `message.proposed_action.message_ids` is non-empty
- [ ] Confirm returns `{ "executed": true, "updated": N }` (N >= 1)
- [ ] Affected emails have `ai_category = 'promotions'` in the database after confirmation

## Failure Criteria
- `proposed_action.action` is not `"move_to_category"`
- Confirm returns `executed: false` when matching emails exist
- Database `ai_category` unchanged after confirmation
- Server returns 500 at any step
