# GC-116: Update nonexistent rule

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: filter-rules
- **Tags**: filter-rules, api, validation
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

2. **Attempt to update a rule with a nonexistent ID**
   - **Target**: `PUT /api/filter-rules/nonexistent-id-00000000`
   - **Input**:
     ```json
     {
       "name": "Ghost rule",
       "conditions": [
         {"field": "from", "operator": "contains", "value": "ghost"}
       ],
       "actions": [
         {"type": "archive"}
       ],
       "is_active": true
     }
     ```
   - **Expected**: 404 Not Found
   ```bash
   curl -4 -s -w "\n%{http_code}" -X PUT http://127.0.0.1:3000/api/filter-rules/nonexistent-id-00000000 \
     -H "X-Session-Token: $TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"name":"Ghost rule","conditions":[{"field":"from","operator":"contains","value":"ghost"}],"actions":[{"type":"archive"}],"is_active":true}'
   ```

3. **Verify no rule was created as a side effect**
   - **Target**: `GET /api/filter-rules`
   - **Input**: None (auth header only)
   - **Expected**: 200 OK, response does not contain a rule named "Ghost rule"
   ```bash
   curl -4 -s http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN"
   ```

## Success Criteria
- [ ] PUT returns HTTP 404
- [ ] No rule named "Ghost rule" exists in the GET list
- [ ] No side effects (no new rules created)

## Failure Criteria
- PUT returns 200 or 201 (upsert behavior instead of strict update)
- PUT returns a status code other than 404
- A "Ghost rule" appears in the list after the PUT
