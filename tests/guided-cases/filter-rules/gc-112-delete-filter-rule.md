# GC-112: Delete filter rule

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

2. **Create a filter rule to delete**
   - **Target**: `POST /api/filter-rules`
   - **Input**:
     ```json
     {
       "name": "Rule to delete",
       "conditions": [
         {"field": "subject", "operator": "starts_with", "value": "[SPAM]"}
       ],
       "actions": [
         {"type": "delete"}
       ]
     }
     ```
   - **Expected**: 201 Created, capture `id` from response
   ```bash
   RULE_ID=$(curl -4 -s -X POST http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"name":"Rule to delete","conditions":[{"field":"subject","operator":"starts_with","value":"[SPAM]"}],"actions":[{"type":"delete"}]}' | jq -r '.id')
   ```

3. **Delete the rule**
   - **Target**: `DELETE /api/filter-rules/$RULE_ID`
   - **Input**: None (auth header only)
   - **Expected**: 204 No Content
   ```bash
   curl -4 -s -w "%{http_code}" -X DELETE http://127.0.0.1:3000/api/filter-rules/$RULE_ID \
     -H "X-Session-Token: $TOKEN"
   ```

4. **Attempt to delete the same rule again**
   - **Target**: `DELETE /api/filter-rules/$RULE_ID`
   - **Input**: None (auth header only)
   - **Expected**: 404 Not Found
   ```bash
   curl -4 -s -w "\n%{http_code}" -X DELETE http://127.0.0.1:3000/api/filter-rules/$RULE_ID \
     -H "X-Session-Token: $TOKEN"
   ```

## Success Criteria
- [ ] POST returns HTTP 201 with a valid `id`
- [ ] First DELETE returns HTTP 204
- [ ] Second DELETE returns HTTP 404
- [ ] Deleted rule is no longer present in subsequent GET /api/filter-rules list

## Failure Criteria
- First DELETE returns non-204 status code
- Second DELETE does not return 404 (rule was not actually deleted)
- Rule still appears in GET /api/filter-rules after deletion
