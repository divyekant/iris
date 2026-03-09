# GC-125: Default toggle — setting new default clears previous

## Metadata
- **Type**: edge
- **Priority**: P0
- **Surface**: api
- **Flow**: aliases
- **Tags**: aliases, api, default, toggle
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
       -d '{"email":"test-gc125@example.com","provider":"gmail","display_name":"GC-125 Test"}'
     ```
   - **Expected**: 201 Created; save `id` as `$ACCOUNT_ID`

3. **Create alias1 with is_default=true**
   - **Target**: `POST /api/aliases`
   - **Input**:
     ```bash
     curl -4 -s -X POST http://127.0.0.1:3000/api/aliases \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d "{\"account_id\":\"$ACCOUNT_ID\",\"email\":\"alias1-gc125@example.com\",\"display_name\":\"Alias One\",\"is_default\":true}"
     ```
   - **Expected**: 201 Created with `is_default` = true; save `id` as `$ALIAS1_ID`

4. **Verify alias1 is the default**
   - **Target**: `GET /api/aliases?account_id=$ACCOUNT_ID`
   - **Input**:
     ```bash
     curl -4 -s http://127.0.0.1:3000/api/aliases?account_id=$ACCOUNT_ID \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 200 OK; alias1 has `is_default` = true

5. **Create alias2 with is_default=true (same account)**
   - **Target**: `POST /api/aliases`
   - **Input**:
     ```bash
     curl -4 -s -X POST http://127.0.0.1:3000/api/aliases \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d "{\"account_id\":\"$ACCOUNT_ID\",\"email\":\"alias2-gc125@example.com\",\"display_name\":\"Alias Two\",\"is_default\":true}"
     ```
   - **Expected**: 201 Created with `is_default` = true; save `id` as `$ALIAS2_ID`

6. **Verify only alias2 is now default**
   - **Target**: `GET /api/aliases?account_id=$ACCOUNT_ID`
   - **Input**:
     ```bash
     curl -4 -s http://127.0.0.1:3000/api/aliases?account_id=$ACCOUNT_ID \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 200 OK; alias1 has `is_default` = false, alias2 has `is_default` = true

## Success Criteria
- [ ] After creating alias1 with is_default=true, alias1 shows is_default=true
- [ ] After creating alias2 with is_default=true, alias2 shows is_default=true
- [ ] After creating alias2, alias1 now shows is_default=false
- [ ] Only one alias per account has is_default=true at any time

## Failure Criteria
- Both aliases show is_default=true simultaneously
- alias1 retains is_default=true after alias2 is created with is_default=true
- Either create call returns non-201 status
