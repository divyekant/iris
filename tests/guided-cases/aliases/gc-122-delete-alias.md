# GC-122: Delete alias

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: aliases
- **Tags**: aliases, api, delete
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
       -d '{"email":"test-gc122@example.com","provider":"gmail","display_name":"GC-122 Test"}'
     ```
   - **Expected**: 201 Created; save `id` as `$ACCOUNT_ID`

3. **Create an alias**
   - **Target**: `POST /api/aliases`
   - **Input**:
     ```bash
     curl -4 -s -X POST http://127.0.0.1:3000/api/aliases \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d "{\"account_id\":\"$ACCOUNT_ID\",\"email\":\"delete-gc122@example.com\",\"display_name\":\"To Delete\",\"is_default\":false}"
     ```
   - **Expected**: 201 Created; save `id` as `$ALIAS_ID`

4. **Delete the alias**
   - **Target**: `DELETE /api/aliases/$ALIAS_ID`
   - **Input**:
     ```bash
     curl -4 -s -o /dev/null -w "%{http_code}" -X DELETE http://127.0.0.1:3000/api/aliases/$ALIAS_ID \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 204 No Content

5. **Attempt to delete again (idempotency check)**
   - **Target**: `DELETE /api/aliases/$ALIAS_ID`
   - **Input**:
     ```bash
     curl -4 -s -o /dev/null -w "%{http_code}" -X DELETE http://127.0.0.1:3000/api/aliases/$ALIAS_ID \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 404 Not Found

6. **Verify alias is gone from listing**
   - **Target**: `GET /api/aliases?account_id=$ACCOUNT_ID`
   - **Input**:
     ```bash
     curl -4 -s http://127.0.0.1:3000/api/aliases?account_id=$ACCOUNT_ID \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 200 OK with JSON array that does NOT contain the deleted alias

## Success Criteria
- [ ] First DELETE returns 204
- [ ] Second DELETE returns 404
- [ ] GET /api/aliases no longer includes the deleted alias

## Failure Criteria
- First DELETE returns non-204 status
- Second DELETE returns non-404 status (e.g., 204 again)
- Deleted alias still appears in GET listing
