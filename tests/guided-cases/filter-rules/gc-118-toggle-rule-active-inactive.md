# GC-118: Toggle rule active/inactive

## Metadata
- **Type**: edge
- **Priority**: P0
- **Surface**: api
- **Flow**: filter-rules
- **Tags**: filter-rules, api, state
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

2. **Create a filter rule (defaults to active)**
   - **Target**: `POST /api/filter-rules`
   - **Input**:
     ```json
     {
       "name": "Toggle test rule",
       "conditions": [
         {"field": "to", "operator": "contains", "value": "support@"}
       ],
       "actions": [
         {"type": "label", "value": "support"}
       ]
     }
     ```
   - **Expected**: 201 Created, `is_active` defaults to `true`, capture `id`
   ```bash
   RESPONSE=$(curl -4 -s -X POST http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"name":"Toggle test rule","conditions":[{"field":"to","operator":"contains","value":"support@"}],"actions":[{"type":"label","value":"support"}]}')
   RULE_ID=$(echo "$RESPONSE" | jq -r '.id')
   IS_ACTIVE=$(echo "$RESPONSE" | jq -r '.is_active')
   echo "Created rule $RULE_ID, is_active=$IS_ACTIVE"
   ```

3. **Verify rule was created as active**
   - **Target**: Response from step 2
   - **Input**: Check `is_active` field
   - **Expected**: `is_active` is `true`

4. **Deactivate the rule**
   - **Target**: `PUT /api/filter-rules/$RULE_ID`
   - **Input**:
     ```json
     {
       "name": "Toggle test rule",
       "conditions": [
         {"field": "to", "operator": "contains", "value": "support@"}
       ],
       "actions": [
         {"type": "label", "value": "support"}
       ],
       "is_active": false
     }
     ```
   - **Expected**: 200 OK, `is_active` is `false`
   ```bash
   curl -4 -s -w "\n%{http_code}" -X PUT http://127.0.0.1:3000/api/filter-rules/$RULE_ID \
     -H "X-Session-Token: $TOKEN" \
     -H "Content-Type: application/json" \
     -d "{\"name\":\"Toggle test rule\",\"conditions\":[{\"field\":\"to\",\"operator\":\"contains\",\"value\":\"support@\"}],\"actions\":[{\"type\":\"label\",\"value\":\"support\"}],\"is_active\":false}"
   ```

5. **Verify deactivation persisted via GET list**
   - **Target**: `GET /api/filter-rules`
   - **Input**: None (auth header only)
   - **Expected**: 200 OK, rule "Toggle test rule" has `is_active` = `false`
   ```bash
   curl -4 -s http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN" | jq '.[] | select(.name == "Toggle test rule") | .is_active'
   ```

6. **Reactivate the rule**
   - **Target**: `PUT /api/filter-rules/$RULE_ID`
   - **Input**:
     ```json
     {
       "name": "Toggle test rule",
       "conditions": [
         {"field": "to", "operator": "contains", "value": "support@"}
       ],
       "actions": [
         {"type": "label", "value": "support"}
       ],
       "is_active": true
     }
     ```
   - **Expected**: 200 OK, `is_active` is `true`
   ```bash
   curl -4 -s -w "\n%{http_code}" -X PUT http://127.0.0.1:3000/api/filter-rules/$RULE_ID \
     -H "X-Session-Token: $TOKEN" \
     -H "Content-Type: application/json" \
     -d "{\"name\":\"Toggle test rule\",\"conditions\":[{\"field\":\"to\",\"operator\":\"contains\",\"value\":\"support@\"}],\"actions\":[{\"type\":\"label\",\"value\":\"support\"}],\"is_active\":true}"
   ```

7. **Verify reactivation persisted via GET list**
   - **Target**: `GET /api/filter-rules`
   - **Input**: None (auth header only)
   - **Expected**: 200 OK, rule "Toggle test rule" has `is_active` = `true`
   ```bash
   curl -4 -s http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN" | jq '.[] | select(.name == "Toggle test rule") | .is_active'
   ```

## Success Criteria
- [ ] POST returns HTTP 201 with `is_active` = `true`
- [ ] First PUT (deactivate) returns HTTP 200 with `is_active` = `false`
- [ ] GET list confirms `is_active` = `false` after deactivation
- [ ] Second PUT (reactivate) returns HTTP 200 with `is_active` = `true`
- [ ] GET list confirms `is_active` = `true` after reactivation
- [ ] Other fields (`name`, `conditions`, `actions`) remain unchanged throughout

## Failure Criteria
- `is_active` does not change to `false` after deactivation PUT
- `is_active` does not change back to `true` after reactivation PUT
- Other fields are altered as a side effect of toggling `is_active`
- Any PUT returns non-200 status code
