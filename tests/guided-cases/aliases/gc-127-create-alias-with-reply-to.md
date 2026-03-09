# GC-127: Create alias with reply_to

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: aliases
- **Tags**: aliases, api, create, reply-to
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
       -d '{"email":"test-gc127@example.com","provider":"gmail","display_name":"GC-127 Test"}'
     ```
   - **Expected**: 201 Created; save `id` as `$ACCOUNT_ID`

3. **Create an alias with reply_to set**
   - **Target**: `POST /api/aliases`
   - **Input**:
     ```bash
     curl -4 -s -X POST http://127.0.0.1:3000/api/aliases \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d "{\"account_id\":\"$ACCOUNT_ID\",\"email\":\"alias-gc127@example.com\",\"display_name\":\"GC-127 Alias\",\"reply_to\":\"replies-gc127@example.com\",\"is_default\":false}"
     ```
   - **Expected**: 201 Created with `reply_to` = "replies-gc127@example.com"

4. **Verify reply_to is persisted**
   - **Target**: `GET /api/aliases?account_id=$ACCOUNT_ID`
   - **Input**:
     ```bash
     curl -4 -s http://127.0.0.1:3000/api/aliases?account_id=$ACCOUNT_ID \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 200 OK with alias showing `reply_to` = "replies-gc127@example.com"

## Success Criteria
- [ ] POST /api/aliases returns 201
- [ ] Response contains `reply_to` field set to "replies-gc127@example.com"
- [ ] GET confirms reply_to value is persisted correctly

## Failure Criteria
- POST returns non-201 status
- reply_to field is null or missing in response
- reply_to value does not match what was sent

## Notes
- This verifies the optional `reply_to` field is properly stored and returned, confirming that the alias will use a different reply-to address than the from address.
