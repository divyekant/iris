# GC-109: Create filter rule happy path

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

2. **Create a filter rule with valid data**
   - **Target**: `POST /api/filter-rules`
   - **Input**:
     ```json
     {
       "name": "Archive newsletters",
       "conditions": [
         {"field": "subject", "operator": "contains", "value": "newsletter"}
       ],
       "actions": [
         {"type": "archive"}
       ]
     }
     ```
   - **Expected**: 201 Created, response body contains the created rule with an `id`, `name` matching "Archive newsletters", `conditions` array with 1 entry, `actions` array with 1 entry, and `is_active` defaulting to `true`
   ```bash
   curl -4 -s -w "\n%{http_code}" -X POST http://127.0.0.1:3000/api/filter-rules \
     -H "X-Session-Token: $TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"name":"Archive newsletters","conditions":[{"field":"subject","operator":"contains","value":"newsletter"}],"actions":[{"type":"archive"}]}'
   ```

3. **Verify returned rule structure**
   - **Target**: Response body from step 2
   - **Input**: Parse JSON response
   - **Expected**:
     - `id` is a non-empty string
     - `name` equals "Archive newsletters"
     - `conditions` has exactly 1 element with `field`="subject", `operator`="contains", `value`="newsletter"
     - `actions` has exactly 1 element with `type`="archive"
     - `is_active` is `true`

## Success Criteria
- [ ] POST returns HTTP 201
- [ ] Response contains a non-empty `id`
- [ ] `name` matches "Archive newsletters"
- [ ] `conditions` array has 1 entry with correct field/operator/value
- [ ] `actions` array has 1 entry with type "archive"
- [ ] `is_active` defaults to `true`

## Failure Criteria
- POST returns non-201 status code
- Response body is missing `id`, `name`, `conditions`, or `actions`
- `is_active` does not default to `true`
- Response is not valid JSON
