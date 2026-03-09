# GC-126: Update nonexistent alias

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: aliases
- **Tags**: aliases, api, update, error-handling
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

2. **Attempt to update a nonexistent alias**
   - **Target**: `PUT /api/aliases/nonexistent-id-gc126`
   - **Input**:
     ```bash
     curl -4 -s -w "\n%{http_code}" -X PUT http://127.0.0.1:3000/api/aliases/nonexistent-id-gc126 \
       -H "Content-Type: application/json" \
       -H "X-Session-Token: $TOKEN" \
       -d '{"email":"updated@example.com","display_name":"Updated","is_default":false}'
     ```
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] PUT /api/aliases/nonexistent-id-gc126 returns 404
- [ ] Response body contains an error message (not a server crash / 500)

## Failure Criteria
- PUT returns 200 or 201 (phantom update)
- PUT returns 500 (unhandled error instead of proper 404)
