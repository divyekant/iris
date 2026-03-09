# GC-124: Create alias with empty account_id

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

## Steps

1. **Obtain session token**
   - **Target**: `GET /api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. **Attempt to create alias with empty account_id**
   - **Target**: `POST /api/aliases`
   - **Input**:
     ```bash
     curl -4 -s -w "\n%{http_code}" -X POST http://127.0.0.1:3000/api/aliases \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d '{"account_id":"","email":"valid-gc124@example.com","display_name":"No Account","is_default":false}'
     ```
   - **Expected**: 400 Bad Request with error message indicating account_id is required

## Success Criteria
- [ ] POST /api/aliases with empty account_id returns 400
- [ ] Response body contains an error message about account_id

## Failure Criteria
- POST returns 201 (alias created despite empty account_id)
- POST returns status other than 400 (e.g., 500 server error)
