# GC-499: Extract Structured Data from Message Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: extracted-data
- **Tags**: extraction, happy-path, POST, structured-data
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one synced email message exists with extractable content (dates, amounts, addresses)
- Note the message ID of a suitable message

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Get a message ID
   - **Target**: `GET http://localhost:3030/api/messages?limit=1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with at least one message, note its `id`

3. Extract structured data from message
   - **Target**: `POST http://localhost:3030/api/extract/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON body containing extracted data items (each with `type`, `value`, `confidence` fields)

4. Verify extracted data appears in list
   - **Target**: `GET http://localhost:3030/api/extracted-data?message_id={message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with array containing items from the extraction

## Success Criteria
- [ ] POST /api/extract/{message_id} returns 200 with extraction results
- [ ] Results contain structured items with type and value
- [ ] GET /api/extracted-data filtered by message_id returns matching items

## Failure Criteria
- POST returns non-200 status
- Response body has no extracted items or missing fields
- GET list does not reflect the extraction results
