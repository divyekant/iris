# GC-115: Create with empty actions

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

2. **Attempt to create a rule with empty actions array**
   - **Target**: `POST /api/filter-rules`
   - **Input**:
     ```json
     {
       "name": "No actions rule",
       "conditions": [
         {"field": "from", "operator": "contains", "value": "spam"}
       ],
       "actions": []
     }
     ```
   - **Expected**: 400 Bad Request, response body contains an error message indicating actions are required
   ```bash
   curl -4 -s -w "\n%{http_code}" -X POST http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"name":"No actions rule","conditions":[{"field":"from","operator":"contains","value":"spam"}],"actions":[]}'
   ```

3. **Verify no rule was created**
   - **Target**: `GET /api/filter-rules`
   - **Input**: None (auth header only)
   - **Expected**: 200 OK, response does not contain a rule named "No actions rule"
   ```bash
   curl -4 -s http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN"
   ```

## Success Criteria
- [ ] POST returns HTTP 400
- [ ] Response contains an error message about empty actions
- [ ] No rule named "No actions rule" exists in the GET list

## Failure Criteria
- POST returns 201 (rule was created despite empty actions)
- POST returns a status code other than 400
- A rule with empty actions appears in the list
