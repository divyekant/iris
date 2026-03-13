# GC-556: Stats Endpoint Reports Accurate Indexed Attachment Count

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: attachment-search
- **Tags**: attachments, stats, accuracy
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A known number of messages with indexable attachments (e.g., 3 messages, each with 1 supported attachment)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Retrieve stats before indexing
   - **Target**: `GET http://localhost:3030/api/attachments/search/stats`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `total_indexed` is N (record baseline)

3. Index 3 known messages
   - **Target**: `POST http://localhost:3030/api/attachments/index/{message_id_1}`, then `{message_id_2}`, then `{message_id_3}`
   - **Input**: Header `X-Session-Token: {token}` on each request
   - **Expected**: Each returns 200 OK with `indexed_count: 1`

4. Retrieve stats after indexing
   - **Target**: `GET http://localhost:3030/api/attachments/search/stats`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `total_indexed` = baseline + 3, response also includes `total_messages_with_attachments` and `last_indexed_at`

## Success Criteria
- [ ] Stats endpoint returns 200 OK
- [ ] `total_indexed` increases by exactly 3 after indexing
- [ ] Response includes at least `total_indexed` and `last_indexed_at` fields

## Failure Criteria
- Stats count does not match actual indexed count
- `last_indexed_at` not updated after indexing
- Response missing required fields
