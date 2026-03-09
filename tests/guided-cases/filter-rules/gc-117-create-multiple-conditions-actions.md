# GC-117: Create rule with multiple conditions and actions

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: filter-rules
- **Tags**: filter-rules, api, crud, edge
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with Sec-Fetch-Site: same-origin)

## Steps

1. **Obtain session token**
   - **Target**: `GET /api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field
   ```bash
   TOKEN=$(curl -4 -s http://127.0.0.1:3000/api/auth/bootstrap -H "Sec-Fetch-Site: same-origin" | jq -r '.token')
   ```

2. **Create a rule with multiple conditions and multiple actions**
   - **Target**: `POST /api/filter-rules`
   - **Input**:
     ```json
     {
       "name": "Complex filter rule",
       "conditions": [
         {"field": "from", "operator": "ends_with", "value": "@marketing.com"},
         {"field": "subject", "operator": "contains", "value": "promo"},
         {"field": "has_attachments", "operator": "equals", "value": "false"}
       ],
       "actions": [
         {"type": "mark_read"},
         {"type": "label", "value": "promotions"},
         {"type": "archive"}
       ]
     }
     ```
   - **Expected**: 201 Created, response body contains all 3 conditions and all 3 actions
   ```bash
   curl -4 -s -w "\n%{http_code}" -X POST http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"name":"Complex filter rule","conditions":[{"field":"from","operator":"ends_with","value":"@marketing.com"},{"field":"subject","operator":"contains","value":"promo"},{"field":"has_attachments","operator":"equals","value":"false"}],"actions":[{"type":"mark_read"},{"type":"label","value":"promotions"},{"type":"archive"}]}'
   ```

3. **Verify all conditions stored correctly**
   - **Target**: Response body from step 2
   - **Input**: Parse JSON response, inspect `conditions` array
   - **Expected**:
     - `conditions` has exactly 3 elements
     - Condition 1: `field`="from", `operator`="ends_with", `value`="@marketing.com"
     - Condition 2: `field`="subject", `operator`="contains", `value`="promo"
     - Condition 3: `field`="has_attachments", `operator`="equals", `value`="false"

4. **Verify all actions stored correctly**
   - **Target**: Response body from step 2
   - **Input**: Parse JSON response, inspect `actions` array
   - **Expected**:
     - `actions` has exactly 3 elements
     - Action 1: `type`="mark_read"
     - Action 2: `type`="label", `value`="promotions"
     - Action 3: `type`="archive"

5. **Confirm via GET list**
   - **Target**: `GET /api/filter-rules`
   - **Input**: None (auth header only)
   - **Expected**: 200 OK, rule "Complex filter rule" present with 3 conditions and 3 actions
   ```bash
   curl -4 -s http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN" | jq '.[] | select(.name == "Complex filter rule")'
   ```

## Success Criteria
- [ ] POST returns HTTP 201
- [ ] `conditions` array has exactly 3 entries with correct field/operator/value each
- [ ] `actions` array has exactly 3 entries with correct type/value each
- [ ] Label action has `value` = "promotions"
- [ ] Rule appears in GET list with all conditions and actions intact

## Failure Criteria
- POST returns non-201 status code
- Any condition or action is missing from the response
- Condition or action fields are incorrect or reordered unexpectedly
- Label action is missing its `value` field
