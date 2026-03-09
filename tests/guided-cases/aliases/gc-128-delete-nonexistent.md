# GC-128: Delete nonexistent alias

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: aliases
- **Tags**: aliases, api, delete, error-handling
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

2. **Attempt to delete a nonexistent alias**
   - **Target**: `DELETE /api/aliases/nonexistent-id-gc128`
   - **Input**:
     ```bash
     curl -4 -s -o /dev/null -w "%{http_code}" -X DELETE http://127.0.0.1:3000/api/aliases/nonexistent-id-gc128 \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] DELETE /api/aliases/nonexistent-id-gc128 returns 404
- [ ] Server does not return 500 (proper error handling, not a crash)

## Failure Criteria
- DELETE returns 204 (false success)
- DELETE returns 500 (unhandled error)
