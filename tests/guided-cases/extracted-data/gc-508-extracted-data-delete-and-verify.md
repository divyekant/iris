# GC-508: Delete Extracted Data and Verify Removal

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: extracted-data
- **Tags**: extraction, delete, verify, DELETE, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one message with extractable data exists

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Run extraction on a message
   - **Target**: `GET http://localhost:3030/api/messages?limit=1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with message, note `id`

3. Extract data
   - **Target**: `POST http://localhost:3030/api/extract/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, note an extracted item `id`

4. Verify item exists in list
   - **Target**: `GET http://localhost:3030/api/extracted-data?message_id={message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with array containing the item

5. Delete the extracted item
   - **Target**: `DELETE http://localhost:3030/api/extracted-data/{item_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK or 204 No Content

6. Verify item removed from list
   - **Target**: `GET http://localhost:3030/api/extracted-data?message_id={message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, array no longer contains the deleted item

## Success Criteria
- [ ] DELETE returns success status
- [ ] Item no longer appears in GET list after deletion
- [ ] Other extracted items (if any) remain unaffected

## Failure Criteria
- DELETE returns error status
- Item still present in list after deletion
