# GC-637: Delegation Agent — POST /api/delegation/playbooks Creates Playbook with Trigger Conditions

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api+ui
- **Flow**: showcase-features
- **Tags**: delegation, playbooks, automation, trigger, action
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- No precondition data required (creates new playbook)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Create a new delegation playbook
   - **Target**: `POST http://localhost:3030/api/delegation/playbooks`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     {
       "name": "Auto-reply to invoice confirmations",
       "trigger": {
         "conditions": [
           { "field": "subject", "operator": "contains", "value": "invoice" },
           { "field": "category", "operator": "equals", "value": "promotions" }
         ],
         "match": "all"
       },
       "action": {
         "type": "auto_reply",
         "template": "Thank you, we have received your invoice and will process it within 5 business days."
       },
       "enabled": true
     }
     ```
   - **Expected**: 201 Created, response contains `playbook` with `id` and all submitted fields

3. Verify playbook structure
   - **Target**: response `playbook` from step 2
   - **Input**: inspect all fields
   - **Expected**: `id` (integer), `name`, `trigger.conditions` (array, length 2), `trigger.match = "all"`, `action.type = "auto_reply"`, `action.template` (non-empty), `enabled = true`

4. Verify playbook is listed
   - **Target**: `GET http://localhost:3030/api/delegation/playbooks`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response `playbooks` array contains the newly created playbook

## Success Criteria
- [ ] POST /api/delegation/playbooks returns 201 Created
- [ ] Response `playbook.id` is a positive integer
- [ ] `trigger.conditions` has 2 entries matching input
- [ ] `action.type = "auto_reply"` and `action.template` is non-empty
- [ ] `enabled = true`
- [ ] Playbook appears in GET /api/delegation/playbooks

## Failure Criteria
- 400/422 for missing required fields (`name`, `trigger`, `action`)
- `trigger.conditions` empty or mismatched
- `enabled` defaulting to false without being set
- Playbook not returned in subsequent list
