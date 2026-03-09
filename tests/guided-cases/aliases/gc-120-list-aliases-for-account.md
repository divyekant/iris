# GC-120: List aliases for account

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: aliases
- **Tags**: aliases, api, list
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
       -d '{"email":"test-gc120@example.com","provider":"gmail","display_name":"GC-120 Test"}'
     ```
   - **Expected**: 201 Created; save `id` as `$ACCOUNT_ID`

3. **Create first alias**
   - **Target**: `POST /api/aliases`
   - **Input**:
     ```bash
     curl -4 -s -X POST http://127.0.0.1:3000/api/aliases \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d "{\"account_id\":\"$ACCOUNT_ID\",\"email\":\"alias1-gc120@example.com\",\"display_name\":\"Alias One\",\"is_default\":false}"
     ```
   - **Expected**: 201 Created

4. **Create second alias**
   - **Target**: `POST /api/aliases`
   - **Input**:
     ```bash
     curl -4 -s -X POST http://127.0.0.1:3000/api/aliases \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d "{\"account_id\":\"$ACCOUNT_ID\",\"email\":\"alias2-gc120@example.com\",\"display_name\":\"Alias Two\",\"is_default\":false}"
     ```
   - **Expected**: 201 Created

5. **List aliases for the account**
   - **Target**: `GET /api/aliases?account_id=$ACCOUNT_ID`
   - **Input**:
     ```bash
     curl -4 -s http://127.0.0.1:3000/api/aliases?account_id=$ACCOUNT_ID \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 200 OK with JSON array containing exactly 2 aliases matching the emails created

6. **List all aliases (no account_id filter)**
   - **Target**: `GET /api/aliases`
   - **Input**:
     ```bash
     curl -4 -s http://127.0.0.1:3000/api/aliases \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 200 OK with JSON array containing at least 2 aliases

## Success Criteria
- [ ] Both POST /api/aliases calls return 201
- [ ] GET /api/aliases?account_id=$ACCOUNT_ID returns exactly 2 aliases
- [ ] Both alias emails appear in the response
- [ ] GET /api/aliases (no filter) returns at least 2 aliases

## Failure Criteria
- GET returns fewer or more aliases than expected for the account
- Alias data does not match what was created
- Non-200 status on GET request
