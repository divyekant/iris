# GC-110: List filter rules

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

2. **Create first filter rule**
   - **Target**: `POST /api/filter-rules`
   - **Input**:
     ```json
     {
       "name": "Star from boss",
       "conditions": [
         {"field": "from", "operator": "contains", "value": "boss@example.com"}
       ],
       "actions": [
         {"type": "star"}
       ]
     }
     ```
   - **Expected**: 201 Created
   ```bash
   curl -4 -s -X POST http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"name":"Star from boss","conditions":[{"field":"from","operator":"contains","value":"boss@example.com"}],"actions":[{"type":"star"}]}'
   ```

3. **Create second filter rule**
   - **Target**: `POST /api/filter-rules`
   - **Input**:
     ```json
     {
       "name": "Mark read promotions",
       "conditions": [
         {"field": "category", "operator": "equals", "value": "promotions"}
       ],
       "actions": [
         {"type": "mark_read"}
       ]
     }
     ```
   - **Expected**: 201 Created
   ```bash
   curl -4 -s -X POST http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"name":"Mark read promotions","conditions":[{"field":"category","operator":"equals","value":"promotions"}],"actions":[{"type":"mark_read"}]}'
   ```

4. **List all filter rules**
   - **Target**: `GET /api/filter-rules`
   - **Input**: None (auth header only)
   - **Expected**: 200 OK, response body is an array containing at least 2 rules, including both "Star from boss" and "Mark read promotions"
   ```bash
   curl -4 -s -w "\n%{http_code}" http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN"
   ```

5. **Verify list contents**
   - **Target**: Response body from step 4
   - **Input**: Parse JSON array
   - **Expected**:
     - Array length >= 2
     - At least one entry with `name` = "Star from boss"
     - At least one entry with `name` = "Mark read promotions"
     - Each entry has `id`, `name`, `conditions`, `actions`, `is_active`

## Success Criteria
- [ ] Both POST requests return HTTP 201
- [ ] GET returns HTTP 200
- [ ] Response is a JSON array with at least 2 entries
- [ ] Both created rules appear in the list by name
- [ ] Each rule in the list has `id`, `name`, `conditions`, `actions`, `is_active` fields

## Failure Criteria
- GET returns non-200 status code
- Response array has fewer than 2 entries
- Either created rule is missing from the list
- Response is not a valid JSON array
