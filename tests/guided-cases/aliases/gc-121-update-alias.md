# GC-121: Update alias

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: aliases
- **Tags**: aliases, api, update
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
       -d '{"email":"test-gc121@example.com","provider":"gmail","display_name":"GC-121 Test"}'
     ```
   - **Expected**: 201 Created; save `id` as `$ACCOUNT_ID`

3. **Create an alias**
   - **Target**: `POST /api/aliases`
   - **Input**:
     ```bash
     curl -4 -s -X POST http://127.0.0.1:3000/api/aliases \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d "{\"account_id\":\"$ACCOUNT_ID\",\"email\":\"original-gc121@example.com\",\"display_name\":\"Original Name\",\"is_default\":false}"
     ```
   - **Expected**: 201 Created; save `id` as `$ALIAS_ID`

4. **Update the alias**
   - **Target**: `PUT /api/aliases/$ALIAS_ID`
   - **Input**:
     ```bash
     curl -4 -s -X PUT http://127.0.0.1:3000/api/aliases/$ALIAS_ID \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d '{"email":"updated-gc121@example.com","display_name":"Updated Name","is_default":false}'
     ```
   - **Expected**: 200 OK with updated alias data showing `email` = "updated-gc121@example.com" and `display_name` = "Updated Name"

5. **Verify update by listing**
   - **Target**: `GET /api/aliases?account_id=$ACCOUNT_ID`
   - **Input**:
     ```bash
     curl -4 -s http://127.0.0.1:3000/api/aliases?account_id=$ACCOUNT_ID \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 200 OK with alias showing updated email and display_name

## Success Criteria
- [ ] PUT /api/aliases/$ALIAS_ID returns 200
- [ ] Response shows updated email "updated-gc121@example.com"
- [ ] Response shows updated display_name "Updated Name"
- [ ] GET confirms the alias reflects the update

## Failure Criteria
- PUT returns non-200 status
- Alias fields not updated in response
- GET still shows original values
