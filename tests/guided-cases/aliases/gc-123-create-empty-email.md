# GC-123: Create alias with empty email

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: aliases
- **Tags**: aliases, api, validation
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
       -d '{"email":"test-gc123@example.com","provider":"gmail","display_name":"GC-123 Test"}'
     ```
   - **Expected**: 201 Created; save `id` as `$ACCOUNT_ID`

3. **Attempt to create alias with empty email**
   - **Target**: `POST /api/aliases`
   - **Input**:
     ```bash
     curl -4 -s -w "\n%{http_code}" -X POST http://127.0.0.1:3000/api/aliases \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d "{\"account_id\":\"$ACCOUNT_ID\",\"email\":\"\",\"display_name\":\"Empty Email\",\"is_default\":false}"
     ```
   - **Expected**: 400 Bad Request with error message indicating email is required

4. **Verify no alias was created**
   - **Target**: `GET /api/aliases?account_id=$ACCOUNT_ID`
   - **Input**:
     ```bash
     curl -4 -s http://127.0.0.1:3000/api/aliases?account_id=$ACCOUNT_ID \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 200 OK with empty JSON array (no aliases created)

## Success Criteria
- [ ] POST /api/aliases with empty email returns 400
- [ ] Response body contains an error message
- [ ] No alias was persisted (GET returns empty list)

## Failure Criteria
- POST returns 201 (alias created despite empty email)
- POST returns status other than 400 (e.g., 500 server error)
- Alias appears in subsequent GET listing
