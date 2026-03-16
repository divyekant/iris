# GC-638: Delegation Agent — POST /api/delegation/process Matches Playbook and Executes Action

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api+ui
- **Flow**: showcase-features
- **Tags**: delegation, process, playbook-match, execute, auto-reply
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A playbook exists that triggers on subject containing "invoice" with action `auto_reply` (created in GC-637)
- A message exists in the database with subject "Invoice #1042 attached" and `message_id` known

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Process the message through delegation engine
   - **Target**: `POST http://localhost:3030/api/delegation/process/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`, path param `message_id`
   - **Expected**: 200 OK, response contains `matched_playbook` and `action_result`

3. Verify playbook was matched
   - **Target**: `matched_playbook` from step 2
   - **Input**: inspect fields
   - **Expected**: `matched_playbook.id` matches the invoice playbook ID from GC-637, `matched_playbook.name = "Auto-reply to invoice confirmations"`

4. Verify action was executed
   - **Target**: `action_result` from step 2
   - **Input**: inspect fields
   - **Expected**: `action_result.type = "auto_reply"`, `action_result.status = "executed"`, `action_result.draft_id` or `sent_at` is present indicating the reply was drafted or sent

5. Verify no-match case
   - **Target**: `POST http://localhost:3030/api/delegation/process/{unrelated_message_id}` (message with unrelated subject)
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `matched_playbook = null`, `action_result = null`

## Success Criteria
- [ ] POST /api/delegation/process/{message_id} returns 200 OK
- [ ] `matched_playbook.id` references the correct playbook
- [ ] `action_result.status = "executed"`
- [ ] `action_result` contains either `draft_id` or `sent_at`
- [ ] Unrelated message returns `matched_playbook = null`

## Failure Criteria
- 404 if message_id not found
- `matched_playbook` is null for a message that clearly matches trigger conditions
- `action_result.status = "failed"` with no error details
- Action executed on a message that doesn't match trigger conditions (false positive)
