# GC-111: Update filter rule

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: filter-rules
- **Tags**: filter-rules, api, crud
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

2. **Create a filter rule**
   - **Target**: `POST /api/filter-rules`
   - **Input**:
     ```json
     {
       "name": "Original rule name",
       "conditions": [
         {"field": "from", "operator": "equals", "value": "test@example.com"}
       ],
       "actions": [
         {"type": "archive"}
       ]
     }
     ```
   - **Expected**: 201 Created, capture `id` from response
   ```bash
   RULE_ID=$(curl -4 -s -X POST http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"name":"Original rule name","conditions":[{"field":"from","operator":"equals","value":"test@example.com"}],"actions":[{"type":"archive"}]}' | jq -r '.id')
   ```

3. **Update the rule — change name and deactivate**
   - **Target**: `PUT /api/filter-rules/$RULE_ID`
   - **Input**:
     ```json
     {
       "name": "Updated rule name",
       "conditions": [
         {"field": "from", "operator": "equals", "value": "test@example.com"}
       ],
       "actions": [
         {"type": "archive"}
       ],
       "is_active": false
     }
     ```
   - **Expected**: 200 OK, response body reflects updated `name` and `is_active` = `false`
   ```bash
   curl -4 -s -w "\n%{http_code}" -X PUT http://127.0.0.1:3000/api/filter-rules/$RULE_ID \
     -H "X-Session-Token: $TOKEN" \
     -H "Content-Type: application/json" \
     -d "{\"name\":\"Updated rule name\",\"conditions\":[{\"field\":\"from\",\"operator\":\"equals\",\"value\":\"test@example.com\"}],\"actions\":[{\"type\":\"archive\"}],\"is_active\":false}"
   ```

4. **Verify updates persisted**
   - **Target**: Response body from step 3
   - **Input**: Parse JSON response
   - **Expected**:
     - `id` matches the original rule ID
     - `name` equals "Updated rule name"
     - `is_active` is `false`
     - `conditions` and `actions` remain unchanged

## Success Criteria
- [ ] POST returns HTTP 201 with a valid `id`
- [ ] PUT returns HTTP 200
- [ ] Returned `name` is "Updated rule name"
- [ ] Returned `is_active` is `false`
- [ ] `conditions` and `actions` are preserved unchanged
- [ ] `id` matches the originally created rule

## Failure Criteria
- PUT returns non-200 status code
- `name` was not updated in the response
- `is_active` is not `false` in the response
- `conditions` or `actions` were altered unexpectedly
