# GC-119: Create alias happy path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: aliases
- **Tags**: aliases, api, create
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with Sec-Fetch-Site: same-origin)
- Test account created (source: local-db, setup: POST /api/accounts)

## Steps

1. **Obtain session token**
   - **Target**: `GET /api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. **Create a test account**
   - **Target**: `POST /api/accounts`
   - **Input**:
     ```bash
     curl -4 -s -X POST http://127.0.0.1:3000/api/accounts \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d '{"email":"test-gc119@example.com","provider":"gmail","display_name":"GC-119 Test"}'
     ```
   - **Expected**: 201 Created with JSON body containing `id` field; save as `$ACCOUNT_ID`

3. **Create an alias**
   - **Target**: `POST /api/aliases`
   - **Input**:
     ```bash
     curl -4 -s -X POST http://127.0.0.1:3000/api/aliases \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d "{\"account_id\":\"$ACCOUNT_ID\",\"email\":\"alias-gc119@example.com\",\"display_name\":\"GC-119 Alias\",\"is_default\":false}"
     ```
   - **Expected**: 201 Created with JSON body containing `id`, `email` = "alias-gc119@example.com", `display_name` = "GC-119 Alias", `is_default` = false

4. **Verify alias exists by listing**
   - **Target**: `GET /api/aliases?account_id=$ACCOUNT_ID`
   - **Input**:
     ```bash
     curl -4 -s http://127.0.0.1:3000/api/aliases?account_id=$ACCOUNT_ID \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 200 OK with JSON array containing the created alias

## Success Criteria
- [ ] POST /api/aliases returns 201
- [ ] Response contains `id`, `email`, `display_name`, `account_id`, `is_default` fields
- [ ] GET /api/aliases returns the newly created alias in the list

## Failure Criteria
- POST /api/aliases returns non-201 status
- Response body missing required fields
- Alias not found in subsequent GET request
